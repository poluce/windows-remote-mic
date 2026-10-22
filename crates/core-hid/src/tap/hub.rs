//! 旁路 IPC：localhost 服务、Frida 消息分发、心跳超时。
//!
//! 只负责「与 Frida gadget 通信 + 把消息转给看门狗」；注入流程在 `inject.rs`，
//! 注入生命周期在 `runtime.rs`。

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{hogp_ioctl_payload, hogp_payload_usages, lock_or_recover, panic_payload_text};

use super::config::{save_injected_pid, tap_port};
use super::inject::{find_rc003_host_pid, request_inject};
use super::runtime::{INJECTED_PID, INJECT_ATTEMPTED_PID};
use super::status::{arm_grace, status, TapState};
use super::watchdog::{on_ioctl_bytes, reset_active_usages};

/// 旁路 IPC 读超时。Frida 脚本每 5 秒发一次心跳，超过 20 秒没有任何数据
/// 说明对端已卡死（socket 未关但不再工作），必须断开重连/重新注入。
const TAP_READ_TIMEOUT: Duration = Duration::from_secs(20);

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
pub(super) fn hub_loop() {
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
        reset_active_usages();
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
