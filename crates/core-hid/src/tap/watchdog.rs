//! 看门狗与「按住状态」跟踪：HOGP 原始报告 → usage 状态机 → 按键事件。
//!
//! 独立于状态上报（`status.rs`）与注入流程（`inject.rs`）：
//! - `on_ioctl_bytes` 把 Frida 旁路上报的 IOCTL 原始报告解析成 usage 变化；
//! - 看门狗兜底释放「松开报告丢失」的按键，避免系统卡键；
//! - `release_mic_on_audio_stop` / `clear_active_usages` 供语音桥与退出路径调用。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::{
    hogp_ioctl_payload, hogp_payload_usages, lock_or_recover, panic_payload_text, raw_input,
    usage_to_vkey,
};

use super::status::in_grace;

/// 看门狗自动释放超时：仅作为「松开报告丢失」的兜底。
///
/// 实测 RC003 所有按键都会发送显式松开报告；按住期间遥控器不重复发报告，
/// 因此该值必须大于长按阈值（550ms），否则长按会被看门狗截断成单击。
/// 可通过环境变量 `REMOTE_MIC_HID_TAP_RELEASE_MS` 覆盖调参。
const AUTO_RELEASE_TIMEOUT: Duration = Duration::from_millis(2000);
/// 麦克风按住说话超时兜底（遥控器硬件推流上限约 15s，日常说话通常不超过 10s）。
/// 超过该时间必然是松开报告丢失（无线丢包），由看门狗兜底释放。
const MIC_AUTO_RELEASE_TIMEOUT: Duration = Duration::from_millis(10_000);
/// 看门狗轮询检查间隔。
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(30);

fn auto_release_timeout() -> Duration {
    std::env::var("REMOTE_MIC_HID_TAP_RELEASE_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(AUTO_RELEASE_TIMEOUT)
}

fn mic_release_timeout() -> Duration {
    std::env::var("REMOTE_MIC_HID_TAP_MIC_RELEASE_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(MIC_AUTO_RELEASE_TIMEOUT)
}

/// 记录最近一次收到遥控器音频包的 Unix 毫秒时间戳。
///
/// 遥控器在长按语音期间不重复发送 HID 按键报文，但会持续推流音频数据。
/// 只要音频包还在到达，就证明蓝牙链路通畅、用户仍在按住说话，
/// 看门狗会持续给麦克风按键续期，绝不在说话推流途中强行掐断。
static LAST_AUDIO_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 收到音频包时被调用，记录音频活跃时间戳，实现音频流对麦克风按键看门狗的保活。
pub fn touch_mic_activity() {
    LAST_AUDIO_ACTIVITY_MS.store(now_millis(), Ordering::Relaxed);
}

/// 麦克风键的 HID 兜底 usage（F5）。拦截开启时由旁路投递；
/// 拦截关闭时让 Raw Input 投递，避免 Voice toggle 被双路翻转。
const MIC_FALLBACK_USAGE: u16 = 0x3E;
/// 当前处于按下状态的 usage 及其最近一次活跃的时间戳。
static ACTIVE_USAGES: Mutex<Option<HashMap<u16, Instant>>> = Mutex::new(None);
static WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);

fn ensure_watchdog() {
    if WATCHDOG_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("hogp-watchdog".into())
        .spawn(|| loop {
            std::thread::sleep(WATCHDOG_INTERVAL);
            // 看门狗是卡键兜底的最后一道闸，绝不能被一次 panic 整体带走。
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(watchdog_tick));
            if let Err(payload) = outcome {
                core_log::log_error(&format!(
                    "[hid-tap] 看门狗 panic 已隔离（继续运行）：{}",
                    panic_payload_text(payload.as_ref())
                ));
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .ok();
}

/// 看门狗单次巡检：把超时未松开的按键补发松开。
fn watchdog_tick() {
    let now = Instant::now();
    let now_ms = now_millis();
    let last_audio_ms = LAST_AUDIO_ACTIVITY_MS.load(Ordering::Relaxed);
    // 只要最近 2.5s 内仍有音频包流动，说明语音处于活跃推流中，不能超时截断麦克风
    let is_voice_streaming = now_ms.saturating_sub(last_audio_ms) < 2500;

    let mut expired = Vec::new();
    {
        let mut guard = lock_or_recover(&ACTIVE_USAGES);
        if let Some(active) = guard.as_mut() {
            active.retain(|&usage, last_seen| {
                if usage == MIC_FALLBACK_USAGE {
                    // 若语音推流处于活跃中，不断续期麦克风按键存活时间，彻底解除长语音限制
                    if is_voice_streaming {
                        *last_seen = now;
                        return true;
                    }
                    if now.duration_since(*last_seen) > mic_release_timeout() {
                        expired.push(usage);
                        false
                    } else {
                        true
                    }
                } else if now.duration_since(*last_seen) > auto_release_timeout() {
                    expired.push(usage);
                    false
                } else {
                    true
                }
            });
        }
    }
    if in_grace() {
        return;
    }
    for usage in expired {
        core_log::log_debug(&format!(
            "[hid-tap] 看门狗自动释放 usage=0x{usage:04X}（松开报告丢失兜底）"
        ));
        emit_usage(usage, false);
    }
}

pub(super) fn on_ioctl_bytes(data: &[u8], eaten: bool) {
    ensure_watchdog();

    core_log::log_debug(&format!(
        "[hid-tap] HOGP 原始数据: {} 字节, hex={}",
        data.len(),
        hex_bytes(data)
    ));

    let Some(payload) = hogp_ioctl_payload(data) else {
        core_log::log_debug(&format!(
            "[hid-tap] HOGP 数据格式不匹配，已忽略: {}",
            hex_bytes(data)
        ));
        return;
    };

    let raw = hogp_payload_usages(payload);
    let mut next = raw.clone();
    next.sort_unstable();
    next.dedup();
    // 拦截关闭时，麦克风 HID 兜底键（0x3E/F5）由 Raw Input 路径投递；
    // 旁路再投一次会让 Voice toggle 一秒内开又关。只在拦截开启
    // （Raw Input 看不到该键）时由旁路投递 0x3E。
    if !eaten {
        next.retain(|&u| u != MIC_FALLBACK_USAGE);
    }

    // 记录所有解析出的 usage，包括未知 usage，便于校准真实信号。
    if !next.is_empty() {
        let usages: Vec<String> = next.iter().map(|u| format!("0x{u:04X}")).collect();
        core_log::log_debug(&format!("[hid-tap] HOGP 载荷 usage: {}", usages.join(", ")));
    }

    let now = Instant::now();
    let mut newly_pressed = Vec::new();
    let mut explicitly_released = Vec::new();

    {
        let mut guard = ACTIVE_USAGES.lock().unwrap_or_else(|e| e.into_inner());
        let active = guard.get_or_insert_with(HashMap::new);

        // 1. 如果收到了全 0 的报告（显式松开）
        if raw.is_empty() {
            explicitly_released.extend(active.keys().copied());
            active.clear();
        } else {
            // 2. 检查此前按下但本次报告中不再存在的键
            let current_keys: Vec<u16> = active.keys().copied().collect();
            for k in current_keys {
                if !next.contains(&k) {
                    explicitly_released.push(k);
                    active.remove(&k);
                }
            }
            // 3. 记录新按键或刷新活跃时间戳
            for usage in next {
                if active.insert(usage, now).is_none() {
                    newly_pressed.push(usage);
                }
            }
        }
    }

    if in_grace() {
        return;
    }
    for usage in newly_pressed {
        emit_usage(usage, true);
    }
    for usage in explicitly_released {
        emit_usage(usage, false);
    }
}

fn emit_usage(usage: u16, pressed: bool) {
    let Some(vkey) = usage_to_vkey(u32::from(usage)) else {
        core_log::log_warn(&format!(
            "[hid-tap] 未知 usage=0x{usage:04X} 按下={pressed}，无对应虚拟键，已记录但不转发"
        ));
        return;
    };
    core_log::log_debug(&format!(
        "[hid-tap] 特殊按键 usage=0x{usage:02X} vkey={vkey} 按下={pressed}"
    ));
    raw_input::emit(raw_input::RawInputEvent {
        vkey,
        make_code: 0,
        pressed,
    });
}

/// 遥控器推流停止（ATVV AudioStopped）时联动释放麦克风键。
///
/// 正常情况下 HID 0x3E 松开报告与 AudioStopped 几乎同一毫秒到达；
/// 若因 BLE 丢包或驱动钩子遗漏导致 HID 松开报告丢失，AudioStopped
/// 能作为权威的硬件松手信号将麦克风键及时释放，避免修饰键或语音输入卡死。
pub fn release_mic_on_audio_stop() {
    let held = {
        let mut guard = match ACTIVE_USAGES.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard
            .as_mut()
            .map(|m| m.remove(&MIC_FALLBACK_USAGE).is_some())
            .unwrap_or(false)
    };
    if held {
        core_log::log_info("[hid-tap] 收到 AudioStopped，联动释放活跃中的麦克风键 (0x3E)");
        emit_usage(MIC_FALLBACK_USAGE, false);
    } else {
        // 即使旁路活跃表中没有 0x3E（例如走 Raw Input 路径或已被释放），
        // 也向 raw_input 补发一条麦克风松开事件；调度器对未按下的按键会天然幂等忽略。
        raw_input::emit(raw_input::RawInputEvent {
            vkey: 116,
            make_code: 0,
            pressed: false,
        });
    }
}

/// 清空所有驱动层正在按住的按键，并向调度器补发松开事件。
///
/// 兜底路径专用（BLE 断链、应用退出、调度器重置）：
/// 避免驱动层 ACTIVE_USAGES 滞留旧状态导致断链后或重连后按键错乱。
pub fn clear_active_usages() {
    let mut released = Vec::new();
    if let Ok(mut guard) = ACTIVE_USAGES.lock() {
        if let Some(active) = guard.as_mut() {
            released.extend(active.keys().copied());
            active.clear();
        }
    }
    if !released.is_empty() {
        core_log::log_warn(&format!(
            "[hid-tap] 清空驱动层活跃按键并补发松开: {:?}",
            released
        ));
    }
    for usage in released {
        emit_usage(usage, false);
    }
}

fn hex_bytes(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect()
}

/// hub 连接断开时清空按住状态（不补发松开：调用方随后会走自己的兜底路径）。
pub(super) fn reset_active_usages() {
    if let Ok(mut guard) = ACTIVE_USAGES.lock() {
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_active_usages_clear_and_mic_release() {
        // 单个测试内串行验证，避免多线程并行测试竞争全局静态变量
        {
            let mut g = ACTIVE_USAGES.lock().unwrap_or_else(|e| e.into_inner());
            let m = g.get_or_insert_with(HashMap::new);
            m.clear();
            m.insert(0x0052, Instant::now());
            m.insert(MIC_FALLBACK_USAGE, Instant::now());
        }

        // 验证 release_mic_on_audio_stop 只移除麦克风键 0x3E
        release_mic_on_audio_stop();
        {
            let g = ACTIVE_USAGES.lock().unwrap_or_else(|e| e.into_inner());
            let m = g.as_ref().unwrap();
            assert!(!m.contains_key(&MIC_FALLBACK_USAGE));
            assert!(m.contains_key(&0x0052));
        }

        // 验证 clear_active_usages 清空剩余按键
        clear_active_usages();
        {
            let g = ACTIVE_USAGES.lock().unwrap_or_else(|e| e.into_inner());
            assert!(g.as_ref().map(|m| m.is_empty()).unwrap_or(true));
        }

        // 验证 touch_mic_activity 在看门狗巡检时能为超时麦克风续期
        let old_time = Instant::now() - Duration::from_secs(15);
        {
            let mut g = ACTIVE_USAGES.lock().unwrap_or_else(|e| e.into_inner());
            let m = g.get_or_insert_with(HashMap::new);
            m.clear();
            m.insert(MIC_FALLBACK_USAGE, old_time);
        }

        // 模拟音频包活跃
        touch_mic_activity();

        // 执行看门狗巡检
        watchdog_tick();

        // 验证麦克风键未被移除，且时间戳已被刷新为最新
        {
            let g = ACTIVE_USAGES.lock().unwrap_or_else(|e| e.into_inner());
            let m = g.as_ref().unwrap();
            assert!(
                m.contains_key(&MIC_FALLBACK_USAGE),
                "语音活跃期间不应被看门狗释放"
            );
            let refreshed = m.get(&MIC_FALLBACK_USAGE).copied().unwrap();
            assert!(refreshed > old_time, "时间戳应被自动续期");
        }

        // 清理现场
        clear_active_usages();
    }
}
