use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager};

use crate::find_install_script;

/// 已 spawn 的语音桥重连循环数（每次调用 start_voice_bridge +1，永不减）。
static BRIDGE_LOOP_SEQ: AtomicU64 = AtomicU64::new(0);
/// 语音桥**监督线程**是否正在运行（互斥：同一时刻只允许一条监督线程，
/// 防止并发双桥争用 GATT）。注意：与 `core_voice::bridge_session_active()`
/// （单次 ATVV 会话是否运行）是两个不同的事实，不要混用。
static BRIDGE_RUNNING: AtomicBool = AtomicBool::new(false);
/// 最近一次语音桥状态快照（供前端恢复状态 / 事件推送）。
static BRIDGE_STATE: Mutex<Option<BridgeStatus>> = Mutex::new(None);
/// 连续「链路通但零数据」会话计数（供诊断页展示）。
static SILENT_SESSIONS: AtomicU64 = AtomicU64::new(0);

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
    /// ATVV 端点是否已就绪（audio + control 均存在）。
    ///
    /// 前端不再自己维护一份「端点就绪」状态，而是随 bridge-status 快照同步，
    /// 避免出现「页面显示未就绪、实际会话已在跑」的陈旧副本。
    #[serde(default)]
    pub endpoints_ready: bool,
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
            endpoints_ready: false,
        }
    }
}

fn set_bridge_status(app: &tauri::AppHandle, mut status: BridgeStatus) {
    // ATVV 端点就绪是后端事实，统一在这一处注入快照，前端只消费不维护。
    status.endpoints_ready = core_voice::atvv_endpoints_ready();
    if let Ok(mut guard) = BRIDGE_STATE.lock() {
        *guard = Some(status.clone());
    }
    let _ = app.emit("bridge-status", &status);
}

/// 连续「链路通但零数据」会话数（诊断页用）。
pub fn current_silent_sessions() -> u64 {
    SILENT_SESSIONS.load(Ordering::Relaxed)
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

/// 恢复遥控器按键接管（连接语音桥时调用）。
fn resume_key_passthrough(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        if state.dispatcher.set_enabled(true) {
            core_log::log_info("[dispatch] 遥控器按键接管已恢复（语音桥已连接）");
        }
    }
}

/// 暂停遥控器按键接管（断开语音桥时调用）。
///
/// 「断开」如果只停语音桥、不停按键接管，用户会疑惑：明明显示未连接，
/// 按遥控器麦克风键却还能唤起豆包等输入法（HID 旁路 + 映射注入仍在工作）。
fn pause_key_passthrough(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        if state.dispatcher.set_enabled(false) {
            core_log::log_info("[dispatch] 遥控器按键接管已暂停（语音桥已断开）");
        }
    }
}

/// 启动真实设备的语音桥（仅 Windows）。
///
/// 采用**事件驱动**而不是定时盲重试：
/// 1. 先打开一个轻量链路监视器（只监听 BLE 连接状态，不打开任何 GATT 服务）；
/// 2. 等 Windows 把遥控器链路连上（遥控器回到本机、或本机是当前主机）后，
///    才按需建立一次 ATVV 会话；
/// 3. 会话随链路断开而结束 → 回到「等待链路」，系统重连后自动恢复；
/// 4. 只有「链路仍在但会话失败」才做有限次重试，连续失败进入 failed 等待，
///    不再无限空转。
///
/// 这样遥控器被另一台电脑占用时，本机收不到链路事件，也就不会去抢设备。
#[tauri::command]
pub fn start_voice_bridge(
    app: tauri::AppHandle,
    device_id: String,
    output_device: String,
) -> String {
    #[cfg(target_os = "windows")]
    {
        if BRIDGE_RUNNING.swap(true, Ordering::SeqCst) {
            core_log::log_warn("[commands/audio] 拒绝重复启动：语音桥已在运行");
            return "语音桥已在运行（请勿重复启动）".to_string();
        }
        core_voice::clear_bridge_stop_request();
        // 断开时暂停过按键接管，重新连接/重试时恢复，避免「连上了但按键没反应」。
        resume_key_passthrough(&app);
        let loop_id = BRIDGE_LOOP_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
        core_log::log_info(&format!(
            "[commands/audio] 收到启动语音桥请求：loop_id={loop_id}, 设备 ID='{device_id}'，输出='{output_device}'"
        ));
        set_bridge_status(&app, BridgeStatus::new("starting", 0, 0, 0, String::new()));

        let app_for_thread = app.clone();
        std::thread::spawn(move || {
            run_bridge_supervisor(app_for_thread, loop_id, &device_id, &output_device);
        });
        "语音桥已启动（系统连上遥控器后会自动接续）".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, device_id, output_device);
        "语音桥仅在 Windows 可用".to_string()
    }
}

/// 重试启动（失败/等待状态下用户点「重试」）：先停旧监督线程，再重新启动。
#[tauri::command]
pub fn retry_voice_bridge(
    app: tauri::AppHandle,
    device_id: String,
    output_device: String,
) -> String {
    #[cfg(target_os = "windows")]
    {
        if BRIDGE_RUNNING.load(Ordering::SeqCst) {
            core_voice::request_bridge_stop();
            for _ in 0..50 {
                if !BRIDGE_RUNNING.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if BRIDGE_RUNNING.load(Ordering::SeqCst) {
                return "语音桥仍在停止中，请稍后重试".to_string();
            }
        }
        start_voice_bridge(app, device_id, output_device)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, device_id, output_device);
        "语音桥仅在 Windows 可用".to_string()
    }
}

/// 语音桥监督线程：等链路 → 建会话 → 会话结束 → 回到等待 / 有限重试。
#[cfg(target_os = "windows")]
fn run_bridge_supervisor(
    app: tauri::AppHandle,
    loop_id: u64,
    device_id: &str,
    output_device: &str,
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
        if core_voice::bridge_stop_requested() {
            core_voice::set_connection_active(false);
            BRIDGE_RUNNING.store(false, Ordering::SeqCst);
            set_bridge_status(&app, BridgeStatus::new("stopped", 0, 0, 0, String::new()));
            core_log::log_line(&format!(
                "[commands/audio] 语音桥监督线程已停止（loop_id={loop_id}，启动阶段被取消）"
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
                    core_log::log_warn(&format!("[commands/audio] {message}"));
                }
                set_bridge_status(&app, BridgeStatus::new("waiting", 0, 0, 0, message));
                if open_attempts == 1 {
                    let _ = app.emit("ble-connection-status", false);
                }
                for _ in 0..5 {
                    if core_voice::bridge_stop_requested() {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
    };

    core_log::log_info("[commands/audio] 语音桥监督已就绪：等待遥控器链路（不再定时盲试）");
    set_bridge_status(&app, BridgeStatus::new("waiting", 0, 0, 0, String::new()));
    // 把真实链路状态同步给 UI：连接页之前可能因「缓存发现成功」把设备显示成已连接，
    // 但此刻链路其实没连上（例如遥控器在另一台电脑上）。以系统状态为准。
    let initial_connected = watcher.is_connected();
    let _ = app.emit("ble-connection-status", initial_connected);
    core_log::log_info(&format!(
        "[commands/audio] 启动时链路状态: connected={initial_connected}"
    ));

    'supervisor: loop {
        if core_voice::bridge_stop_requested() {
            break;
        }

        // 1) 等链路连接：Windows 负责 HOGP 自动重连，这里只等事件/每 500ms 查一次停止标志。
        if !watcher.is_connected() {
            core_log::log_line(
                "[commands/audio] 遥控器当前不在本机链路上，等待系统连接（期间不会抢占设备）",
            );
            loop {
                if core_voice::bridge_stop_requested() {
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
        if core_voice::bridge_stop_requested() {
            break;
        }

        // 2) 链路已连接：建立一次 ATVV 会话
        attempt += 1;
        set_bridge_status(
            &app,
            BridgeStatus::new("starting", attempt, failures, 0, last_error.clone()),
        );
        core_log::log_line(&format!(
            "[commands/audio] 链路已连接，开始建立 ATVV 语音会话（loop_id={loop_id}, attempt={attempt}）"
        ));

        let app_cb = app.clone();
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

        // 单次会话 panic 不能带走监督线程。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            core_voice::run_bridge(device_id, output_device, on_status)
        }));
        if core_voice::bridge_stop_requested() {
            break;
        }

        // 会话结束兜底释放（防卡键）：链路已断时物理松开事件永远到不了。
        crate::force_release_held_keys(
            &app,
            &format!("语音桥会话结束（loop_id={loop_id}, attempt={attempt}）"),
        );

        let session_err = match result {
            Ok(Ok(())) => None,
            Ok(Err(e)) => {
                core_log::log_error(&format!(
                    "[commands/audio] 语音桥会话出错（attempt={attempt}）：{e}"
                ));
                Some(e)
            }
            Err(payload) => {
                let detail = core_hid::panic_payload_text(payload.as_ref());
                core_log::log_error(&format!(
                    "[commands/audio] 语音桥会话 panic 已隔离（attempt={attempt}）：{detail}"
                ));
                Some(format!("语音桥异常：{detail}"))
            }
        };

        // 会话健康检查（仅正常结束判断「链路通但零数据」）。
        if session_err.is_none() {
            if let Some(report) = core_voice::last_session_report() {
                if is_silent_session(report.duration_secs, report.control_pkts) {
                    silent_sessions = silent_sessions.saturating_add(1);
                    SILENT_SESSIONS.store(silent_sessions, Ordering::Relaxed);
                    core_log::log_warn(&format!(
                        "[commands/audio] 链路健康告警：会话存活 {:.0}s 却未收到任何控制通知（连续 {silent_sessions} 次），遥控器语音服务可能挂起",
                        report.duration_secs
                    ));
                } else {
                    silent_sessions = 0;
                    SILENT_SESSIONS.store(0, Ordering::Relaxed);
                }
            }
        }

        // 3a) 链路已断开（或用户断开）→ 正常路径：回到等待，不盲重试。
        if !watcher.is_connected() {
            failures = 0;
            last_error.clear();
            set_bridge_status(
                &app,
                BridgeStatus::new("waiting", attempt, 0, 0, String::new()),
            );
            core_log::log_line(
                "[commands/audio] 会话随链路断开结束，回到等待状态（系统重连后自动恢复）",
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
        if silent_sessions >= 3 {
            set_bridge_status(
                &app,
                BridgeStatus::new(
                    "failed",
                    attempt,
                    failures,
                    0,
                    "链路已连接但遥控器连续多次没有任何语音数据（语音固件可能挂起）：请给遥控器断电重启（抠电池 10 秒）后重试".to_string(),
                ),
            );
            core_log::log_error(
                "[commands/audio] 连续多次零数据会话，进入失败等待（不再主动重试）",
            );
            wait_for_link_change_or_stop(&watcher);
            silent_sessions = 0;
            SILENT_SESSIONS.store(0, Ordering::Relaxed);
            failures = 0;
            last_error.clear();
            continue;
        }

        if failures >= FAILED_AFTER_FAILURES {
            set_bridge_status(
                &app,
                BridgeStatus::new("failed", attempt, failures, 0, last_error.clone()),
            );
            core_log::log_error(&format!(
                "[commands/audio] 语音会话连续失败 {failures} 次，进入失败等待（等链路变化或用户操作）：{last_error}"
            ));
            wait_for_link_change_or_stop(&watcher);
            failures = 0;
            continue;
        }

        let delay = retry_delay_secs(failures);
        set_bridge_status(
            &app,
            BridgeStatus::new("retrying", attempt, failures, delay, last_error.clone()),
        );
        core_log::log_line(&format!(
            "[commands/audio] {delay} 秒后重试建立语音会话（连续失败 {failures} 次）"
        ));
        let mut slept = 0u64;
        while slept < delay {
            if core_voice::bridge_stop_requested() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
            slept += 1;
        }
    }

    core_voice::set_connection_active(false);
    BRIDGE_RUNNING.store(false, Ordering::SeqCst);
    set_bridge_status(
        &app,
        BridgeStatus::new("stopped", attempt, failures, 0, String::new()),
    );
    core_log::log_line(&format!(
        "[commands/audio] 语音桥监督线程已停止（loop_id={loop_id}），可重新启动"
    ));
}

/// 阻塞到链路状态变化或用户请求停止。
///
/// 用于 failed / 零数据会话后的「失败等待」：不主动重试，避免无限空转与抢占。
#[cfg(target_os = "windows")]
fn wait_for_link_change_or_stop(watcher: &core_ble::LinkWatcher) {
    loop {
        if core_voice::bridge_stop_requested() {
            return;
        }
        if watcher.wait_change(Duration::from_millis(500)).is_some() {
            return;
        }
    }
}

/// 判断一次会话是否属于「链路通但零数据」的可疑会话。
///
/// 正常会话至少会收到 CAPS 响应（控制包 ≥1）；存活足够久却一个控制包
/// 都没有，说明遥控器的 ATVV 语音服务没有响应（实测固件挂起时如此，
/// 需要给遥控器断电重启）。
fn is_silent_session(duration_secs: f32, control_pkts: u64) -> bool {
    duration_secs >= 15.0 && control_pkts == 0
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
        pause_key_passthrough(&app);
        set_bridge_status(&app, BridgeStatus::new("stopped", 0, 0, 0, String::new()));
        return BridgeStopResult {
            stopped: true,
            message: "语音桥未在运行；遥控器按键接管已暂停，点「连接」恢复".to_string(),
        };
    }
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
        // 断开 = 停止语音桥 + 暂停按键接管：
        // 否则遥控器麦克风键仍会经 HOGP 旁路 + 映射注入唤起豆包等输入法。
        pause_key_passthrough(&app);
        set_bridge_status(&app, BridgeStatus::new("stopped", 0, 0, 0, String::new()));
    }
    core_log::log_info(&format!("[commands/audio] 停止语音桥：stopped={stopped}"));
    BridgeStopResult {
        stopped,
        message: if stopped {
            "已断开连接（遥控器按键接管已暂停，点「连接」恢复）".to_string()
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
                return "找不到 VB-CABLE 安装助手脚本：安装包可能过旧或安装不完整。\n请安装最新版 Remote Mic 后重试，或从 VB-Audio 官网手动下载安装 VB-CABLE。".to_string();
            }
        };

        core_log::log_info(&format!(
            "[commands/audio] 启动 VB-CABLE 下载/安装助手：{}",
            script.display()
        ));
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
                let stdout = String::from_utf8_lossy(&out.stdout);
                core_log::log_info(&format!(
                    "[commands/audio] VB-CABLE 助手脚本输出：{}",
                    stdout.trim()
                ));
                "已启动 VB-CABLE 下载与官方安装程序（请留意 UAC 弹窗）。\n\
                 1) 在官方安装窗口点击安装；\n\
                 2) 安装完成后必须重启电脑；\n\
                 3) 重启后回到本页点「重新检测」。"
                    .to_string()
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                core_log::log_error(&format!(
                    "[commands/audio] VB-CABLE 助手脚本失败：{}",
                    stderr.trim()
                ));
                format!(
                    "安装失败：{}\n如仍失败，请从 VB-Audio 官网手动下载安装 VB-CABLE。",
                    stderr.trim()
                )
            }
            Err(e) => format!("无法启动安装助手：{e}"),
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
