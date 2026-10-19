use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

use crate::find_install_script;

/// 已 spawn 的语音桥重连循环数（每次调用 start_voice_bridge +1，永不减）。
static BRIDGE_LOOP_SEQ: AtomicU64 = AtomicU64::new(0);
/// 语音桥重连循环是否正在运行（互斥：同一时刻只允许一条循环，防止并发双桥争用 GATT）。
static BRIDGE_RUNNING: AtomicBool = AtomicBool::new(false);
/// 请求停止当前语音桥循环（stop_voice_bridge 置位，循环在安全点检查后退出）。
static BRIDGE_STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
/// 最近一次语音桥状态快照（供前端恢复状态 / 事件推送）。
static BRIDGE_STATE: Mutex<Option<BridgeStatus>> = Mutex::new(None);

/// 连续失败多少次后把状态标为 `failed`（仍会按 30s 上限继续重试，用户可手动断开）。
const FAILED_AFTER_FAILURES: u64 = 5;

/// 语音桥状态（「连接」页用来展示重连进度与失败原因）。
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    /// `idle` / `starting` / `running` / `retrying` / `failed` / `stopped`
    pub state: String,
    /// 累计第几次会话尝试（从 1 开始）。
    pub attempt: u64,
    /// 连续失败次数（成功建立会话后清零）。
    pub failures: u64,
    /// 下一次重连等待秒数（非重试状态为 0）。
    pub next_retry_secs: u64,
    /// 最近一次失败原因（无则空串）。
    pub last_error: String,
}

impl BridgeStatus {
    fn new(
        state: &str,
        attempt: u64,
        failures: u64,
        next_retry_secs: u64,
        last_error: String,
    ) -> Self {
        Self {
            state: state.into(),
            attempt,
            failures,
            next_retry_secs,
            last_error,
        }
    }
}

fn set_bridge_status(app: &tauri::AppHandle, status: BridgeStatus) {
    if let Ok(mut guard) = BRIDGE_STATE.lock() {
        *guard = Some(status.clone());
    }
    let _ = app.emit("bridge-status", &status);
}

/// 读取最近一次语音桥状态（供 `get_runtime_status` 快照）。
pub fn current_bridge_status() -> Option<BridgeStatus> {
    BRIDGE_STATE.lock().ok().and_then(|g| g.clone())
}

/// 指数退避重连间隔（秒）：2/4/8/15，之后 30 封顶，带 ±25% 抖动。
///
/// 旧实现是「每 10 秒无限重试」，遥控器不在身边时会空转上千次且日志刷屏；
/// 现在前几次快速重试，连续失败后退避到 30s，并让 UI 能显示重试进度。
fn retry_delay_secs(failures: u64) -> u64 {
    let base = match failures {
        0 | 1 => 2,
        2 => 4,
        3 => 8,
        4 => 15,
        _ => 30,
    };
    let jitter_window = (base / 4).max(1);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as u64;
    base + nanos % (jitter_window + 1)
}

/// 启动真实设备的语音桥（仅 Windows）。在工作线程中运行。
/// 互斥：已有重连循环在跑时拒绝新请求，避免并发双桥争用同一 GATT 会话。
#[tauri::command]
pub fn start_voice_bridge(
    app: tauri::AppHandle,
    device_id: String,
    output_device: String,
) -> String {
    #[cfg(target_os = "windows")]
    {
        if BRIDGE_RUNNING.swap(true, Ordering::SeqCst) {
            core_log::log_warn("[commands/audio] 拒绝重复启动：语音桥循环已在运行");
            return "语音桥已在运行（请勿重复启动）".to_string();
        }
        BRIDGE_STOP_REQUESTED.store(false, Ordering::SeqCst);
        core_voice::clear_bridge_stop_request();
        let loop_id = BRIDGE_LOOP_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
        core_log::log_info(&format!(
            "[commands/audio] 收到启动语音桥请求：loop_id={loop_id}, 设备 ID='{device_id}'，输出='{output_device}'"
        ));
        set_bridge_status(&app, BridgeStatus::new("starting", 1, 0, 0, String::new()));

        let app_for_thread = app.clone();
        std::thread::spawn(move || {
            let mut attempt: u64 = 0;
            let mut failures: u64 = 0;
            loop {
                if BRIDGE_STOP_REQUESTED.load(Ordering::SeqCst) {
                    break;
                }
                attempt += 1;
                let app_cb = app_for_thread.clone();
                let attempt_cb = attempt;
                let failures_cb = failures;
                let on_status = move |connected: bool| {
                    let _ = app_cb.emit("ble-connection-status", connected);
                    if connected {
                        set_bridge_status(
                            &app_cb,
                            BridgeStatus::new("running", attempt_cb, failures_cb, 0, String::new()),
                        );
                    }
                };

                core_log::log_line(&format!(
                    "[commands/audio] 语音桥启动: loop_id={loop_id}, attempt={attempt}"
                ));
                // 单次会话 panic 不能带走重连循环（否则 BRIDGE_RUNNING 永远为 true，
                // 用户只能重启应用才能恢复）。
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    core_voice::run_bridge(&device_id, &output_device, on_status)
                }));

                let last_error = match result {
                    Ok(Ok(())) => {
                        core_log::log_line(&format!(
                            "[commands/audio] 语音桥已停止，准备重连: loop_id={loop_id}, attempt={attempt}"
                        ));
                        None
                    }
                    Ok(Err(e)) => {
                        core_log::log_error(&format!(
                            "[commands/audio] 语音桥错误: loop_id={loop_id}, attempt={attempt}, err={e}"
                        ));
                        Some(e)
                    }
                    Err(payload) => {
                        let detail = core_hid::panic_payload_text(payload.as_ref());
                        core_log::log_error(&format!(
                            "[commands/audio] 语音桥线程 panic 已隔离: loop_id={loop_id}, attempt={attempt}, {detail}"
                        ));
                        Some(format!("语音桥异常：{detail}"))
                    }
                };

                // 本次语音桥会话结束（BLE 断链 / 用户断开 / 音频通道断开）：
                // 遥控器已经不在链路上了，物理松开事件永远不会到达，必须先把
                // 映射层按住的动作和虚拟 HID 的残留按住状态一起松开，否则修饰键
                // 会一直卡在系统里（实测遥控器断链后 rctrl 卡死，物理键盘失灵）。
                crate::force_release_held_keys(
                    &app_for_thread,
                    &format!("语音桥会话结束（loop_id={loop_id}, attempt={attempt}）"),
                );

                // 区分「正常会话自然结束」与「连接失败」：前者重置失败计数，
                // 后者累计用于退避与 failed 状态判定。
                let session_ok = last_error.is_none();
                if session_ok {
                    failures = 0;
                } else {
                    failures = failures.saturating_add(1);
                }

                if BRIDGE_STOP_REQUESTED.load(Ordering::SeqCst) {
                    break;
                }

                let delay = retry_delay_secs(failures);
                let state = if failures >= FAILED_AFTER_FAILURES {
                    "failed"
                } else {
                    "retrying"
                };
                set_bridge_status(
                    &app_for_thread,
                    BridgeStatus::new(
                        state,
                        attempt,
                        failures,
                        delay,
                        last_error.clone().unwrap_or_default(),
                    ),
                );
                core_log::log_line(&format!(
                    "[commands/audio] 将在 {delay} 秒后重连（loop_id={loop_id}, 第 {attempt} 次尝试，连续失败 {failures} 次）"
                ));

                // 分秒睡眠，期间响应停止请求（最迟 1 秒内退出）。
                let mut slept: u64 = 0;
                while slept < delay {
                    if BRIDGE_STOP_REQUESTED.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                    slept += 1;
                }
                if BRIDGE_STOP_REQUESTED.load(Ordering::SeqCst) {
                    break;
                }
            }
            BRIDGE_RUNNING.store(false, Ordering::SeqCst);
            set_bridge_status(
                &app_for_thread,
                BridgeStatus::new("stopped", attempt, failures, 0, String::new()),
            );
            core_log::log_line(&format!(
                "[commands/audio] 语音桥循环已停止（loop_id={loop_id}），可重新启动"
            ));
        });
        "语音桥已启动（断线后自动重连）".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, device_id, output_device);
        "语音桥仅在 Windows 可用".to_string()
    }
}

/// `stop_voice_bridge` 的结果：前端据此决定是否真的标记为「已断开」。
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStopResult {
    pub stopped: bool,
    pub message: String,
}

/// 停止语音桥重连循环（当前 run_bridge 会话结束后、或最迟约 10 秒内生效）。
///
/// 之前无论是否真的停下都返回「已断开连接」，用户会以为断了但实际仍在重连。
#[tauri::command]
pub fn stop_voice_bridge(app: tauri::AppHandle) -> BridgeStopResult {
    if !BRIDGE_RUNNING.load(Ordering::SeqCst) {
        core_voice::set_connection_active(false);
        set_bridge_status(&app, BridgeStatus::new("stopped", 0, 0, 0, String::new()));
        return BridgeStopResult {
            stopped: true,
            message: "未在连接".to_string(),
        };
    }
    BRIDGE_STOP_REQUESTED.store(true, Ordering::SeqCst);
    core_voice::request_bridge_stop();
    core_voice::set_connection_active(false);
    // 最长等 10s：WinRT 操作已有超时，正常会在数秒内退出。
    for _ in 0..100 {
        if !BRIDGE_RUNNING.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let stopped = !BRIDGE_RUNNING.load(Ordering::SeqCst);
    if stopped {
        set_bridge_status(&app, BridgeStatus::new("stopped", 0, 0, 0, String::new()));
    }
    core_log::log_info(&format!("[commands/audio] 停止语音桥：stopped={stopped}"));
    BridgeStopResult {
        stopped,
        message: if stopped {
            "已断开连接".to_string()
        } else {
            "停止超时：语音桥仍在运行（可能卡在蓝牙操作，请稍后重试）".to_string()
        },
    }
}

/// 在没有真实遥控器的情况下模拟完整语音链路。
#[tauri::command]
pub fn simulate_voice_chain(
    output_device: String,
    test_audio_path: Option<String>,
) -> Result<core_voice::SimulatedVoiceResult, String> {
    #[cfg(target_os = "windows")]
    {
        core_voice::simulate_voice_chain(&output_device, test_audio_path.as_deref())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (output_device, test_audio_path);
        Err("仅限 Windows".to_string())
    }
}

#[derive(Serialize)]
pub struct VbCableStatus {
    pub input: bool,
    pub output: bool,
    pub ready: bool,
}

#[tauri::command]
pub fn vb_cable_status() -> VbCableStatus {
    let d = core_audio::diagnostics::run();
    VbCableStatus {
        input: d.cable_input_present,
        output: d.cable_output_present,
        ready: d.has_vb_cable,
    }
}

/// 一键安装 VB-CABLE：运行官方安装辅助脚本。
#[tauri::command]
pub fn install_vb_cable() -> String {
    let d = core_audio::diagnostics::run();
    if d.has_vb_cable {
        return "已安装 VB-CABLE，无需重复安装".to_string();
    }

    #[cfg(target_os = "windows")]
    {
        let script = match find_install_script() {
            Some(s) => s,
            None => {
                return format!(
                    "找不到安装脚本（从 {} 向上搜索 scripts/install-vb-cable.ps1 均失败）",
                    std::env::current_dir()
                        .map(|c| c.display().to_string())
                        .unwrap_or_default()
                );
            }
        };

        match std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap_or_default(),
            ])
            .output()
        {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            Ok(out) => format!("安装失败：{}", String::from_utf8_lossy(&out.stderr).trim()),
            Err(e) => format!("无法启动安装程序：{e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = ();
        "VB-CABLE 安装仅在 Windows 上可用".to_string()
    }
}

/// 运行音频诊断：端点 + VB-CABLE 是否存在。
#[tauri::command]
pub fn audio_diagnostics() -> core_audio::diagnostics::AudioDiagnostics {
    core_audio::diagnostics::run()
}

/// 向所选端点循环播放数次测试音。
#[tauri::command]
pub fn play_test_tone_loop(device_name: Option<String>, repetitions: Option<u32>) -> String {
    let reps = repetitions.unwrap_or(3).clamp(1, 10);
    #[cfg(target_os = "windows")]
    {
        match core_audio::playback::play_test_tone_loop(device_name.as_deref(), reps, 500) {
            Ok(()) => format!("已循环播放 {reps} 次"),
            Err(e) => format!("播放失败: {e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = device_name;
        let _ = reps;
        "测试音循环仅在 Windows 可用".to_string()
    }
}

/// 触发 Windows 语音输入（Win+H），帮助用户在引导流程中配置 CABLE Output。
#[tauri::command]
pub fn trigger_voice_typing() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        core_input::press_win_h().map_err(|e| e.to_string())?;
        Ok("已唤出 Windows 语音输入条，请点击 ⚙️ 齿轮将麦克风选为 CABLE Output".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("仅限 Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_grows_then_caps() {
        let d1 = retry_delay_secs(1);
        let d2 = retry_delay_secs(2);
        let d3 = retry_delay_secs(3);
        let d4 = retry_delay_secs(4);
        let d5 = retry_delay_secs(5);
        let d99 = retry_delay_secs(99);
        assert!((2..=3).contains(&d1), "第 1 次失败应约 2s，实际 {d1}");
        assert!(d2 >= 4, "第 2 次失败应 ≥4s，实际 {d2}");
        assert!(d3 >= 8, "第 3 次失败应 ≥8s，实际 {d3}");
        assert!(d4 >= 15, "第 4 次失败应 ≥15s，实际 {d4}");
        assert!(d5 >= 30, "第 5 次失败应封顶 30s，实际 {d5}");
        assert!(d99 <= 35, "退避必须有上限，实际 {d99}");
    }

    #[test]
    fn bridge_status_serializes_for_frontend() {
        let status = BridgeStatus::new("retrying", 3, 2, 8, "no device".into());
        let json = serde_json::to_value(&status).expect("应可序列化");
        assert_eq!(json["state"], "retrying");
        assert_eq!(json["attempt"], 3);
        assert_eq!(json["failures"], 2);
        assert_eq!(json["next_retry_secs"], 8);
        assert_eq!(json["last_error"], "no device");
    }
}
