//! 语音桥监督线程：等链路 → 建立 ATVV 会话 → 会话结束 → 回到等待 / 有限重试。
//!
//! 设计目标：
//! - **与 Tauri 解耦**：UI 事件、按键释放通过 [`SupervisorHost`] 端口回调，
//!   本模块不依赖 `tauri`；
//! - **与平台解耦**：真实会话由 [`crate::run_bridge`] 执行，本模块只做状态机；
//! - **可单测**：退避策略、静默会话判定等纯逻辑在本文件内直接测。
//!
//! 之前这套状态机写在 `src-tauri/src/commands/audio.rs` 里（694 行巨型文件、
//! 4 个全局 static、无法脱离 Tauri 测试），现在下沉到这里，命令层只做薄适配。

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

#[cfg(target_os = "windows")]
use std::sync::Arc;
#[cfg(target_os = "windows")]
use std::time::Duration;

/// 连续失败多少次后把状态标为 `failed`（仍会按 30s 上限继续重试，用户可手动断开）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const FAILED_AFTER_FAILURES: u64 = 5;

/// 「链路通但零数据」连续多少次后进入失败等待（提示用户给遥控器断电重启）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const SILENT_SESSIONS_BEFORE_FAILED: u64 = 3;

/// 语音桥状态（「连接」页用来展示重连进度与失败原因）。
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    /// `idle` / `waiting` / `starting` / `running` / `retrying` / `failed` / `stopped`
    pub state: String,
    /// 累计第几次会话尝试（从 1 开始）。
    pub attempt: u64,
    /// 连续失败次数（成功建立会话后清零）。
    pub failures: u64,
    /// 下一次重连等待秒数（非重试状态为 0）。
    pub next_retry_secs: u64,
    /// 最近一次失败原因（无则空串）。
    pub last_error: String,
    /// ATVV 端点是否已就绪（audio + control 均存在）。
    ///
    /// 前端不再自己维护一份「端点就绪」状态，而是随 bridge-status 快照同步，
    /// 避免出现「页面显示未就绪、实际会话已在跑」的陈旧副本。
    #[serde(default)]
    pub endpoints_ready: bool,
}

impl BridgeStatus {
    pub fn new(
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
            endpoints_ready: false,
        }
    }
}

/// 监督线程的宿主端口。
///
/// 由 UI 层实现：发状态事件、发链路状态事件、会话结束时兜底释放按键。
/// 把这些副作用抽成 trait 后，状态机本身可以在没有 Tauri/窗口的环境里测试与复用。
pub trait SupervisorHost: Send + Sync + 'static {
    /// 推送语音桥状态快照（前端 `bridge-status` 事件）。
    fn emit_status(&self, status: &BridgeStatus);
    /// 推送 BLE 链路状态（前端 `ble-connection-status` 事件）。
    fn emit_connection(&self, connected: bool);
    /// 会话结束兜底释放按键：链路已断时物理松开事件永远到不了。
    fn release_held_keys(&self, reason: &str);
}

/// 监督线程是否正在运行（互斥：同一时刻只允许一条监督线程，防止双桥争用 GATT）。
///
/// 注意：与 [`crate::bridge_session_active`]（单次 ATVV 会话是否运行）是两个不同的事实。
static SUPERVISOR_RUNNING: AtomicBool = AtomicBool::new(false);
/// 最近一次语音桥状态快照（供前端恢复状态 / 事件推送）。
static SUPERVISOR_STATUS: Mutex<Option<BridgeStatus>> = Mutex::new(None);
/// 连续「链路通但零数据」会话计数（供诊断页展示）。
static SILENT_SESSIONS: AtomicU64 = AtomicU64::new(0);
/// 已 spawn 的语音桥监督线程数（每次启动 +1，永不减）。
static LOOP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 尝试占用监督线程。返回 `true` 表示抢到，可以启动；`false` 表示已在运行。
pub fn try_start() -> bool {
    !SUPERVISOR_RUNNING.swap(true, Ordering::SeqCst)
}

/// 监督线程是否在运行。
pub fn is_running() -> bool {
    SUPERVISOR_RUNNING.load(Ordering::SeqCst)
}

/// 取下一个监督线程序号（日志对照用）。
pub fn next_loop_id() -> u64 {
    LOOP_SEQ.fetch_add(1, Ordering::SeqCst) + 1
}

/// 读取最近一次语音桥状态（供 `get_runtime_status` 快照）。
pub fn current_status() -> Option<BridgeStatus> {
    SUPERVISOR_STATUS.lock().ok().and_then(|g| g.clone())
}

/// 连续「链路通但零数据」会话数（诊断页用）。
pub fn silent_sessions() -> u64 {
    SILENT_SESSIONS.load(Ordering::Relaxed)
}

/// 发布状态：注入「端点就绪」这一后端事实 → 缓存快照 → 通知宿主。
pub fn publish(host: &dyn SupervisorHost, mut status: BridgeStatus) {
    status.endpoints_ready = crate::atvv_endpoints_ready();
    if let Ok(mut guard) = SUPERVISOR_STATUS.lock() {
        *guard = Some(status.clone());
    }
    host.emit_status(&status);
}

fn set_silent_sessions(value: u64) {
    SILENT_SESSIONS.store(value, Ordering::Relaxed);
}

/// 指数退避重连间隔（秒）：2/4/8/15，之后 30 封顶，带 ±25% 抖动。
///
/// 旧实现是「每 10 秒无限重试」，遥控器不在身边时会空转上千次且日志刷屏；
/// 现在前几次快速重试，连续失败后退避到 30s，并让 UI 能显示重试进度。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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

/// 判断一次会话是否属于「链路通但零数据」的可疑会话。
///
/// 正常会话至少会收到 CAPS 响应（控制包 ≥1）；存活足够久却一个控制包
/// 都没有，说明遥控器的 ATVV 语音服务没有响应（实测固件挂起时如此，
/// 需要给遥控器断电重启）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn is_silent_session(duration_secs: f32, control_pkts: u64) -> bool {
    duration_secs >= 15.0 && control_pkts == 0
}

/// 运行监督线程（阻塞，直到停止请求）。
///
/// `loop_id` 仅用于日志对照；`host` 负责所有对外副作用。
#[cfg(target_os = "windows")]
pub fn run_supervisor(
    device_id: &str,
    output_device: &str,
    loop_id: u64,
    host: Arc<dyn SupervisorHost>,
) {
    let mut attempt: u64 = 0;
    let mut failures: u64 = 0;
    // 连续「会话正常但一个控制包都没有」的次数（遥控器语音固件挂起特征）。
    let mut silent_sessions: u64 = 0;
    let mut last_error = String::new();

    // 只监听链路，不打开 GATT：不会主动触发连接，也就不会和别的电脑抢设备。
    //
    // 打开监视器可能失败（蓝牙无线电被关、设备被删除、驱动异常）：这类失败
    // 多为可恢复，不能直接判死。这里每 5 秒重试，期间保持「等待遥控器上线」。
    let mut open_attempts: u64 = 0;
    let watcher = loop {
        if crate::bridge_stop_requested() {
            crate::set_connection_active(false);
            SUPERVISOR_RUNNING.store(false, Ordering::SeqCst);
            publish(
                host.as_ref(),
                BridgeStatus::new("stopped", 0, 0, 0, String::new()),
            );
            core_log::log_line(&format!(
                "[voice/supervisor] 监督线程已停止（loop_id={loop_id}，启动阶段被取消）"
            ));
            return;
        }
        match core_ble::LinkWatcher::open(device_id) {
            Ok(w) => break w,
            Err(e) => {
                open_attempts += 1;
                let message =
                    format!("暂时无法访问遥控器（蓝牙可能未开启，或已在系统设置中删除配对）：{e}");
                if open_attempts == 1 || open_attempts.is_multiple_of(12) {
                    core_log::log_warn(&format!("[voice/supervisor] {message}"));
                }
                publish(
                    host.as_ref(),
                    BridgeStatus::new("waiting", 0, 0, 0, message),
                );
                if open_attempts == 1 {
                    host.emit_connection(false);
                }
                for _ in 0..5 {
                    if crate::bridge_stop_requested() {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    };

    core_log::log_info("[voice/supervisor] 语音桥监督已就绪：等待遥控器链路（不再定时盲试）");
    publish(
        host.as_ref(),
        BridgeStatus::new("waiting", 0, 0, 0, String::new()),
    );
    // 把真实链路状态同步给 UI：连接页可能因「缓存发现成功」把设备显示成已连接，
    // 但此刻链路其实没连上（例如遥控器在另一台电脑上）。以系统状态为准。
    let initial_connected = watcher.is_connected();
    host.emit_connection(initial_connected);
    core_log::log_info(&format!(
        "[voice/supervisor] 启动时链路状态: connected={initial_connected}"
    ));

    'supervisor: loop {
        if crate::bridge_stop_requested() {
            break;
        }

        // 1) 等链路连接：Windows 负责 HOGP 自动重连，这里只等事件/每 500ms 查一次停止标志。
        if !watcher.is_connected() {
            core_log::log_line(
                "[voice/supervisor] 遥控器当前不在本机链路上，等待系统连接（期间不会抢占设备）",
            );
            loop {
                if crate::bridge_stop_requested() {
                    break 'supervisor;
                }
                // 事件 + 轮询双保险：蓝牙无线电被关/事件丢失时，链路状态
                // 变化事件可能不送达，这里每 500ms 主动查一次。
                if watcher.is_connected() {
                    break;
                }
                if watcher.wait_change(Duration::from_millis(500)) == Some(true) {
                    break;
                }
            }
        }
        if crate::bridge_stop_requested() {
            break;
        }

        // 2) 链路已连接：建立一次 ATVV 会话
        attempt += 1;
        publish(
            host.as_ref(),
            BridgeStatus::new("starting", attempt, failures, 0, last_error.clone()),
        );
        core_log::log_line(&format!(
            "[voice/supervisor] 链路已连接，开始建立 ATVV 语音会话（loop_id={loop_id}, attempt={attempt}）"
        ));

        let host_cb = host.clone();
        let attempt_cb = attempt;
        let failures_cb = failures;
        let on_status = move |connected: bool| {
            host_cb.emit_connection(connected);
            if connected {
                publish(
                    host_cb.as_ref(),
                    BridgeStatus::new("running", attempt_cb, failures_cb, 0, String::new()),
                );
            }
        };

        // 单次会话 panic 不能带走监督线程。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::run_bridge(device_id, output_device, on_status)
        }));
        if crate::bridge_stop_requested() {
            break;
        }

        // 会话结束兜底释放（防卡键）：链路已断时物理松开事件永远到不了。
        host.release_held_keys(&format!(
            "语音桥会话结束（loop_id={loop_id}, attempt={attempt}）"
        ));

        let session_err = match result {
            Ok(Ok(())) => None,
            Ok(Err(e)) => {
                core_log::log_error(&format!(
                    "[voice/supervisor] 语音桥会话出错（attempt={attempt}）：{e}"
                ));
                Some(e)
            }
            Err(payload) => {
                let detail = core_hid::panic_payload_text(payload.as_ref());
                core_log::log_error(&format!(
                    "[voice/supervisor] 语音桥会话 panic 已隔离（attempt={attempt}）：{detail}"
                ));
                Some(format!("语音桥异常：{detail}"))
            }
        };

        // 会话健康检查（仅正常结束判断「链路通但零数据」）。
        if session_err.is_none() {
            if let Some(report) = crate::last_session_report() {
                if is_silent_session(report.duration_secs, report.control_pkts) {
                    silent_sessions = silent_sessions.saturating_add(1);
                    set_silent_sessions(silent_sessions);
                    core_log::log_warn(&format!(
                        "[voice/supervisor] 链路健康告警：会话存活 {:.0}s 却未收到任何控制通知（连续 {silent_sessions} 次），遥控器语音服务可能挂起",
                        report.duration_secs
                    ));
                } else {
                    silent_sessions = 0;
                    set_silent_sessions(0);
                }
            }
        }

        // 3a) 链路已断开（或用户断开）→ 正常路径：回到等待，不盲重试。
        if !watcher.is_connected() {
            failures = 0;
            last_error.clear();
            publish(
                host.as_ref(),
                BridgeStatus::new("waiting", attempt, 0, 0, String::new()),
            );
            core_log::log_line(
                "[voice/supervisor] 会话随链路断开结束，回到等待状态（系统重连后自动恢复）",
            );
            continue;
        }

        // 3b) 链路仍在但会话失败 → 有限重试 / 失败等待。
        if let Some(e) = session_err {
            last_error = e;
        }
        failures = failures.saturating_add(1);

        // 「链路通但零数据」连续 3 次：不再空转，提示用户重启遥控器，
        // 等链路变化（掉线/重连）或用户操作后再继续。
        if silent_sessions >= SILENT_SESSIONS_BEFORE_FAILED {
            publish(
                host.as_ref(),
                BridgeStatus::new(
                    "failed",
                    attempt,
                    failures,
                    0,
                    "链路已连接但遥控器连续多次没有任何语音数据（语音固件可能挂起）：请给遥控器断电重启（抠电池 10 秒）后重试".to_string(),
                ),
            );
            core_log::log_error(
                "[voice/supervisor] 连续多次零数据会话，进入失败等待（不再主动重试）",
            );
            wait_for_link_change_or_stop(&watcher);
            silent_sessions = 0;
            set_silent_sessions(0);
            failures = 0;
            last_error.clear();
            continue;
        }

        if failures >= FAILED_AFTER_FAILURES {
            publish(
                host.as_ref(),
                BridgeStatus::new("failed", attempt, failures, 0, last_error.clone()),
            );
            core_log::log_error(&format!(
                "[voice/supervisor] 语音会话连续失败 {failures} 次，进入失败等待（等链路变化或用户操作）：{last_error}"
            ));
            wait_for_link_change_or_stop(&watcher);
            failures = 0;
            continue;
        }

        let delay = retry_delay_secs(failures);
        publish(
            host.as_ref(),
            BridgeStatus::new("retrying", attempt, failures, delay, last_error.clone()),
        );
        core_log::log_line(&format!(
            "[voice/supervisor] {delay} 秒后重试建立语音会话（连续失败 {failures} 次）"
        ));
        let mut slept = 0u64;
        while slept < delay {
            if crate::bridge_stop_requested() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
            slept += 1;
        }
    }

    crate::set_connection_active(false);
    SUPERVISOR_RUNNING.store(false, Ordering::SeqCst);
    publish(
        host.as_ref(),
        BridgeStatus::new("stopped", attempt, failures, 0, String::new()),
    );
    core_log::log_line(&format!(
        "[voice/supervisor] 监督线程已停止（loop_id={loop_id}），可重新启动"
    ));
}

/// 阻塞到链路状态变化或用户请求停止。
///
/// 用于 failed / 零数据会话后的「失败等待」：不主动重试，避免无限空转与抢占。
#[cfg(target_os = "windows")]
fn wait_for_link_change_or_stop(watcher: &core_ble::LinkWatcher) {
    loop {
        if crate::bridge_stop_requested() {
            return;
        }
        if watcher.wait_change(Duration::from_millis(500)).is_some() {
            return;
        }
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
    fn silent_session_detection() {
        // 连接够久却零控制包 → 可疑（遥控器语音固件可能挂起）。
        assert!(is_silent_session(20.0, 0));
        // 短会话不判定（用户可能刚连上就断开）。
        assert!(!is_silent_session(3.0, 0));
        // 收到过 CAPS 等控制包 → 正常。
        assert!(!is_silent_session(60.0, 1));
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
