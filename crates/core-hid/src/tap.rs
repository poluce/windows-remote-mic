//! 可选的 HOGP 旁路，仅用于返回 / 音量加 / 音量减。
//!
//! 在 ATVV 通知就绪后启动。缺少 gadget 或用户拒绝 UAC 时，
//! 语音和 Raw Input 按键仍可正常工作。

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

mod inject;

use inject::{find_rc003_host_pid, inject_into, request_inject, write_last_inject_error};

mod status;

use status::{arm_grace, status};
pub use status::{last_status, set_status_callback, TapState, TapStatusEvent};

mod watchdog;

use watchdog::on_ioctl_bytes;
pub use watchdog::{clear_active_usages, release_mic_on_audio_stop};

use crate::{hogp_ioctl_payload, hogp_payload_usages, lock_or_recover, panic_payload_text};

const TAP_PORT: u16 = 17331;
const INPUT_GRACE: Duration = Duration::from_millis(800);
/// 旁路 IPC 读超时。Frida 脚本每 5 秒发一次心跳，超过 20 秒没有任何数据
/// 说明对端已卡死（socket 未关但不再工作），必须断开重连/重新注入。
const TAP_READ_TIMEOUT: Duration = Duration::from_secs(20);
const GADGET_SCRIPT: &str = include_str!("rc003_hid_tap.js");

static RUNNING: AtomicBool = AtomicBool::new(false);

static INJECTED_PID: Mutex<Option<u32>> = Mutex::new(None);
/// PID we already asked UAC / inject for. Never prompt again for the same host.
static INJECT_ATTEMPTED_PID: Mutex<Option<u32>> = Mutex::new(None);
/// hook 统计去重键：(calls, captured, success, pending, other_status)。
type HookStatsKey = (u64, u64, u64, u64, u64);
/// 上次已记录的 hook 统计，避免心跳每 5 秒重复刷屏。
static LAST_HOOK_STATS: Mutex<Option<HookStatsKey>> = Mutex::new(None);
/// 「吃掉报告」日志的限流状态：最多每秒 1 条，其余合并计数。
static EATEN_LOG_LAST: Mutex<Option<Instant>> = Mutex::new(None);
static EATEN_LOG_SUPPRESSED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 写一条「吃掉报告」日志，但最多每秒一条，避免按住说话时数百行/秒刷屏。
fn log_eaten_report(message: &str) {
    let now = Instant::now();
    let mut last = lock_or_recover(&EATEN_LOG_LAST);
    let should_log = last
        .map(|t| now.duration_since(t) >= Duration::from_secs(1))
        .unwrap_or(true);
    if !should_log {
        EATEN_LOG_SUPPRESSED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let suppressed = EATEN_LOG_SUPPRESSED.swap(0, Ordering::Relaxed);
    *last = Some(now);
    if suppressed > 0 {
        core_log::log_info(&format!(
            "[hid-tap] 吃掉报告：{message}（期间另有 {suppressed} 条同类日志被限流合并）"
        ));
    } else {
        core_log::log_info(&format!("[hid-tap] 吃掉报告：{message}"));
    }
}

#[derive(Deserialize)]
struct HubMessage {
    kind: String,
    #[serde(default)]
    raw: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    stats: Option<HookStats>,
    #[serde(default)]
    eat_enabled: bool,
    #[serde(default)]
    eaten: bool,
    #[serde(default)]
    trace_enabled: bool,
    #[serde(default)]
    func: String,
    #[serde(default)]
    dst: String,
    #[serde(default)]
    src: String,
    #[serde(default)]
    len: u32,
    #[serde(default)]
    hex: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    target_resolved: String,
    #[serde(default)]
    ret: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    dst_range: String,
}

/// Frida 脚本随心跳上报的 hook 统计，用于确认 hook 是否持续被调用。
#[derive(Deserialize)]
struct HookStats {
    #[serde(default)]
    calls: u64,
    #[serde(default)]
    captured: u64,
    #[serde(default)]
    success: u64,
    #[serde(default)]
    pending: u64,
    #[serde(default)]
    other_status: u64,
}

fn tap_port() -> u16 {
    std::env::var("REMOTE_MIC_HID_TAP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(TAP_PORT)
}

/// 是否开启「吃掉」模式：驱动层把 HOGP 报告清零，系统不响应遥控器按键，
/// 只由本应用注入映射动作。
///
/// 优先级：`eat-mode.txt`（设置页写入，Frida 脚本热更新）>
/// 环境变量 `REMOTE_MIC_HID_TAP_EAT`（开发覆盖）> 默认开启。
fn eat_enabled() -> bool {
    if let Some(v) = read_eat_mode_file() {
        return v;
    }
    std::env::var("REMOTE_MIC_HID_TAP_EAT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(true)
}

/// 吃掉模式热更新文件：设置页/启动时写入，Frida 脚本每秒轮询，
/// 无需重新注入 WUDFHost 即可生效。
fn eat_mode_file() -> PathBuf {
    gadget_dir().join("eat-mode.txt")
}

fn read_eat_mode_file() -> Option<bool> {
    let text = std::fs::read_to_string(eat_mode_file()).ok()?;
    let text = text.trim();
    if text == "1" || text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text == "0" || text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// 写入吃掉模式热更新文件（设置页切换与启动时调用）。
pub fn write_eat_mode_file(enabled: bool) {
    let _ = std::fs::create_dir_all(gadget_dir());
    let _ = std::fs::write(eat_mode_file(), if enabled { "1" } else { "0" });
}

/// 是否开启 IOCTL/模块追踪：枚举 WUDFHost 的 HID/BTH 模块导出并记录所有
/// IOCTL 码与缓冲区，用于定位报告转发点。
/// 通过环境变量 `REMOTE_MIC_HID_TAP_TRACE=1` 开启，默认关闭。
fn trace_enabled() -> bool {
    std::env::var("REMOTE_MIC_HID_TAP_TRACE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn gadget_dir() -> PathBuf {
    // 运行时文件放在 ProgramData：
    // - SYSTEM（WUDFHost）一定能读取/加载 DLL；
    // - 通过 ensure_runtime_acl 给 Users 加 Modify 权限，普通用户也能更新 JS/config；
    // - 避免 LocalAppData 下 SYSTEM 访问不确定的问题。
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(base).join("RemoteMic").join("hid-tap")
}

fn gadget_dll_path() -> PathBuf {
    gadget_dir().join("frida-gadget.dll")
}

/// 已注入的 WUDFHost PID 持久化文件。应用重启后据此跳过重复注入，
/// 避免每次连接都弹 UAC（同一 WUDFHost 只需注入一次）。
fn injected_pid_file() -> PathBuf {
    gadget_dir().join("injected-pid.txt")
}

fn load_injected_pid() -> Option<u32> {
    std::fs::read_to_string(injected_pid_file())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

fn save_injected_pid(pid: u32) {
    let _ = std::fs::write(injected_pid_file(), pid.to_string());
}

pub fn gadget_available() -> bool {
    gadget_dll_path().is_file()
}

/// 如果本进程以 `--hid-tap-inject --pid N` 方式启动，则注入后退出。
/// 返回 true 时调用方不应启动 UI。
pub fn maybe_run_injector() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == "--hid-tap-inject") {
        return false;
    }
    let pid = args
        .windows(2)
        .find(|pair| pair[0] == "--pid")
        .and_then(|pair| pair[1].parse::<u32>().ok());
    let Some(pid) = pid else {
        core_log::log_error("[hid-tap] 注入器缺少 --pid 参数");
        std::process::exit(2);
    };
    match inject_into(pid) {
        Ok(()) => {
            core_log::log_info(&format!("[hid-tap] 已注入 pid={pid}"));
            std::process::exit(0);
        }
        Err(e) => {
            core_log::log_error(&format!("[hid-tap] 注入 pid={pid} 失败: {e}"));
            write_last_inject_error(&e);
            std::process::exit(1);
        }
    }
}

/// 启动 localhost 服务，并在需要时请求提权以执行注入。
/// 可安全地多次调用。绝不打开 HID GATT 服务。
///
/// 缺少 Frida Gadget 时会在后台线程自动下载（不阻塞语音桥初始化）。
pub fn start_after_atvv() {
    if RUNNING.swap(true, Ordering::SeqCst) {
        core_log::log_info("[hid-tap] 旁路服务已在运行");
        return;
    }
    // 恢复上次已注入的 WUDFHost PID：若宿主进程未变，则跳过注入，不再弹 UAC。
    if let Some(pid) = load_injected_pid() {
        *INJECTED_PID.lock().unwrap_or_else(|e| e.into_inner()) = Some(pid);
    }

    if gadget_available() {
        status(TapState::Pending, "正在准备返回/音量旁路…");
    } else {
        status(
            TapState::Pending,
            "正在自动下载 Frida Gadget（首次需要联网）…",
        );
    }

    std::thread::Builder::new()
        .name("rc003-hid-tap".into())
        .spawn(|| {
            // 旁路线程是「守护型」服务：任何一次 panic 都不能让它静默消失，
            // 这里兜住并自动重启（UAC/注入都带幂等重试，重启是安全的）。
            loop {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Err(e) = prepare_runtime() {
                        status(TapState::Unavailable, &format!("返回/音量旁路未启用：{e}"));
                        return false;
                    }
                    if !gadget_available() {
                        status(
                            TapState::Unavailable,
                            "返回/音量旁路未启用：自动安装 Frida Gadget 后仍未找到 DLL",
                        );
                        return false;
                    }
                    status(TapState::Pending, "返回/音量旁路已启动（首次可能弹出 UAC）");
                    hub_loop();
                    true
                }));
                match outcome {
                    Ok(true) => break,
                    Ok(false) => {
                        RUNNING.store(false, Ordering::SeqCst);
                        break;
                    }
                    Err(payload) => {
                        core_log::log_error(&format!(
                            "[hid-tap] 旁路线程 panic 已隔离，2s 后自动重启：{}",
                            panic_payload_text(payload.as_ref())
                        ));
                        std::thread::sleep(Duration::from_secs(2));
                    }
                }
            }
        })
        .ok();
}

fn prepare_runtime() -> Result<(), String> {
    let dir = gadget_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // 先把目录 ACL 设好，确保普通用户可写入下载的 DLL / JS / config，
    // SYSTEM（WUDFHost）也能加载。
    ensure_runtime_acl(&dir)?;

    let mut wrote_new_file = false;

    // DLL 缺失时自动从 GitHub 下载官方 Gadget（版本/哈希与 fetch 脚本一致）。
    if !gadget_dll_path().is_file() {
        crate::gadget_fetch::ensure_frida_gadget(&dir)
            .map_err(|e| format!("自动获取 Frida Gadget 失败：{e}"))?;
        wrote_new_file = true;
    }

    // 写脚本。ProgramData 已给 Users Modify 权限，正常可写；失败则说明环境异常，阻断并提示。
    let script_path = dir.join("rc003-hid-tap.js");
    if !file_has_content(&script_path, GADGET_SCRIPT.as_bytes()) {
        std::fs::write(&script_path, GADGET_SCRIPT)
            .map_err(|e| format!("写入 rc003-hid-tap.js 失败: {e}"))?;
        wrote_new_file = true;
    }

    // 写 config（文件名必须与 DLL 名匹配：frida-gadget.config）
    // 吃掉模式热更新文件也一并就位（设置页可随时覆盖）。
    if !eat_mode_file().is_file() {
        write_eat_mode_file(eat_enabled());
    }
    let cfg = serde_json::json!({
        "interaction": {
            "type": "script",
            "path": "rc003-hid-tap.js",
            "parameters": {
                "host": "127.0.0.1",
                "port": tap_port(),
                "eat": eat_enabled(),
                "trace": trace_enabled(),
                "eat_file": eat_mode_file().to_string_lossy()
            },
            "on_change": "ignore"
        },
        "runtime": "qjs",
        "teardown": "minimal"
    });
    let cfg_bytes = serde_json::to_vec_pretty(&cfg).unwrap();
    let cfg_path = dir.join("frida-gadget.config");
    if !file_has_content(&cfg_path, &cfg_bytes) {
        std::fs::write(&cfg_path, cfg_bytes)
            .map_err(|e| format!("写入 frida-gadget.config 失败: {e}"))?;
        wrote_new_file = true;
    }

    // 新写入文件后再巩固一次 ACL（下载/写脚本可能改变继承）。
    if wrote_new_file {
        ensure_runtime_acl(&dir)?;
    }

    Ok(())
}

/// 文件存在且内容与给定字节一致时返回 true（用于跳过不必要的写入）。
fn file_has_content(path: &std::path::Path, expected: &[u8]) -> bool {
    match std::fs::read(path) {
        Ok(existing) => existing == expected,
        Err(_) => false,
    }
}

/// 确保运行时目录权限：
/// - SYSTEM / Administrators 完全控制（保证 WUDFHost 加载 DLL）
/// - Users 具备 Modify（保证普通用户能更新 JS/config）
///
/// 使用 `/grant` 追加，不破坏已有权限。
fn ensure_runtime_acl(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = std::process::Command::new("icacls.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .arg(path)
            .args([
                "/grant",
                "*S-1-5-18:(OI)(CI)F",
                "/grant",
                "*S-1-5-32-544:(OI)(CI)F",
                "/grant",
                "*S-1-5-32-545:(OI)(CI)M",
                "/C",
                "/Q",
            ])
            .output()
            .map_err(|e| format!("icacls 启动失败: {e}"))?;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!("设置 Gadget 目录 ACL 失败: {}", stdout.trim()));
        }
    }
    Ok(())
}

fn hub_loop() {
    let port = tap_port();
    let retry = Duration::from_secs(2);
    let mut last_miss = Instant::now()
        .checked_sub(Duration::from_secs(30))
        .unwrap_or_else(Instant::now);
    loop {
        let Some(pid) = find_rc003_host_pid() else {
            if last_miss.elapsed() >= Duration::from_secs(10) {
                status(
                    TapState::Pending,
                    "未找到 HOGP 宿主进程（HostPid），返回/音量旁路等待中",
                );
                last_miss = Instant::now();
            }
            std::thread::sleep(retry);
            continue;
        };

        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => {
                let _ = l.set_nonblocking(true);
                l
            }
            Err(e) => {
                core_log::log_warn(&format!("[hid-tap] 绑定端口 {port} 失败: {e}"));
                std::thread::sleep(retry);
                continue;
            }
        };

        {
            let mut attempted = INJECT_ATTEMPTED_PID
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let mut injected = INJECTED_PID.lock().unwrap_or_else(|e| e.into_inner());
            if *attempted == Some(pid) && *injected != Some(pid) {
                drop(listener);
                std::thread::sleep(retry);
                continue;
            }
            if *injected != Some(pid) {
                *attempted = Some(pid);
                match request_inject(pid) {
                    Ok(true) => {
                        *injected = Some(pid);
                        save_injected_pid(pid);
                        status(
                            TapState::Pending,
                            &format!("已注入 HOGP 宿主 pid={pid}，正在等待旁路连接"),
                        );
                    }
                    Ok(false) => {
                        status(
                            TapState::Unavailable,
                            "UAC 被拒绝，返回/音量键仍不可用；普通键与语音不受影响。重新连接前不会再次弹窗。",
                        );
                        drop(listener);
                        std::thread::sleep(retry);
                        continue;
                    }
                    Err(e) => {
                        status(
                            TapState::Unavailable,
                            &format!("注入失败：{e}。普通键与语音不受影响，本次会话不再弹 UAC。"),
                        );
                        drop(listener);
                        std::thread::sleep(retry);
                        continue;
                    }
                }
            }
        }

        let deadline = Instant::now() + Duration::from_secs(15);
        let mut client = None;
        while Instant::now() < deadline {
            if find_rc003_host_pid() != Some(pid) {
                let mut attempted = INJECT_ATTEMPTED_PID
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let mut injected = INJECTED_PID.lock().unwrap_or_else(|e| e.into_inner());
                *attempted = None;
                *injected = None;
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    // Frida 脚本每 5 秒发一次心跳；设 20s 读超时用于检测
                    // 「socket 还开着但对端已卡死」的情况，避免 serve_client 永久阻塞。
                    let _ = stream.set_read_timeout(Some(TAP_READ_TIMEOUT));
                    client = Some(stream);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
        let Some(stream) = client else {
            drop(listener);
            continue;
        };
        drop(listener);
        status(
            TapState::Attached,
            &format!("返回/音量旁路已附着 pid={pid}，请按返回或音量键验证"),
        );
        arm_grace();
        {
            // Frida 侧消息处理同样不能 panic 出去：出问题只断这条连接，
            // hub_loop 会重新 accept / 重新注入。
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve_client(stream)));
            if let Err(payload) = outcome {
                core_log::log_error(&format!(
                    "[hid-tap] 旁路连接处理 panic 已隔离：{}",
                    panic_payload_text(payload.as_ref())
                ));
            }
        }
        watchdog::reset_active_usages();
        // 连接关闭也通过 status() 通知前端，避免只有日志没有状态。
        // 内部会重试，但对用户显示为“等待旁路重连”而不是“已关闭”。
        status(TapState::Pending, "返回/音量旁路连接已断开，正在等待重连");
    }
}

fn serve_client(stream: std::net::TcpStream) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let exit_reason = loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break "对端关闭（EOF）".to_string(),
            Ok(_) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // 心跳超时：对端可能仍在连接但已停止工作（WUDFHost 挂起等）。
                // 断开后 hub_loop 会重新 accept，必要时重新注入。
                break format!("心跳超时（{}s 无数据）", TAP_READ_TIMEOUT.as_secs());
            }
            Err(e) => break format!("读取错误: {e}"),
        }
        let Ok(msg) = serde_json::from_str::<HubMessage>(line.trim()) else {
            continue;
        };
        match msg.kind.as_str() {
            "gatt_read" => {
                if let Some(bytes) = decode_hex(&msg.raw) {
                    let has_usage = hogp_ioctl_payload(&bytes)
                        .map(|p| !hogp_payload_usages(p).is_empty())
                        .unwrap_or(false);
                    if msg.eaten && has_usage {
                        core_log::log_info("[hid-tap] 已吃掉按键（系统不可见）");
                    }
                    on_ioctl_bytes(&bytes, msg.eaten);
                }
            }
            "heartbeat" => {
                if let Some(stats) = &msg.stats {
                    let key = (
                        stats.calls,
                        stats.captured,
                        stats.success,
                        stats.pending,
                        stats.other_status,
                    );
                    let mut last = LAST_HOOK_STATS.lock().unwrap_or_else(|e| e.into_inner());
                    if *last != Some(key) {
                        *last = Some(key);
                        core_log::log_debug(&format!(
                            "[hid-tap] hook 统计: calls={}, captured={}, success={}, pending={}, other_status={}",
                            stats.calls, stats.captured, stats.success, stats.pending, stats.other_status
                        ));
                    }
                }
            }
            "ready" => {
                core_log::log_info(&format!(
                    "[hid-tap] Frida 脚本已连接并上报 ready（吃掉模式={}，追踪模式={}）",
                    if msg.eat_enabled { "开启" } else { "关闭" },
                    if msg.trace_enabled {
                        "开启"
                    } else {
                        "关闭"
                    }
                ));
            }
            "memcpy_trace" => {
                core_log::log_info(&format!(
                    "[hid-tap] memcpy 追踪: func={} dst={} src={} len={} hex={} dst_range={}",
                    msg.func, msg.dst, msg.src, msg.len, msg.hex, msg.dst_range
                ));
            }
            "report_path" => {
                core_log::log_info(&format!(
                    "[hid-tap] 报告处理: func={} src={} len={} hex={}",
                    msg.func, msg.src, msg.len, msg.hex
                ));
            }
            "eaten_report" => {
                log_eaten_report(&msg.message);
            }
            "eat_mode_changed" => {
                core_log::log_info(&format!(
                    "[hid-tap] 吃掉模式热更新为：{}",
                    if msg.eat_enabled { "开启" } else { "关闭" }
                ));
            }
            "vcall_trace" => {
                core_log::log_info(&format!(
                    "[hid-tap] 间接调用: target={} ({}) ret={} args=[{}]",
                    msg.target,
                    msg.target_resolved,
                    msg.ret,
                    msg.args.join(", ")
                ));
            }
            "error" => core_log::log_warn(&format!("[hid-tap] 旁路错误: {}", msg.message)),
            _ => {}
        }
    };
    core_log::log_info(&format!("[hid-tap] 旁路连接结束: {exit_reason}"));
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let hi = from_hex(bytes[i])?;
        let lo = from_hex(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn from_hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
