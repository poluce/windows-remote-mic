//! 可选的 HOGP 旁路，仅用于返回 / 音量加 / 音量减。
//!
//! 在 ATVV 通知就绪后启动。缺少 gadget 或用户拒绝 UAC 时，
//! 语音和 Raw Input 按键仍可正常工作。

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{hogp_ioctl_payload, hogp_payload_usages, raw_input, usage_to_vkey};

const TAP_PORT: u16 = 17331;
const INPUT_GRACE: Duration = Duration::from_millis(800);
const GADGET_SCRIPT: &str = include_str!("rc003_hid_tap.js");

static RUNNING: AtomicBool = AtomicBool::new(false);
type StatusCallback = Box<dyn Fn(String) + Send>;

static STATUS_CB: Mutex<Option<StatusCallback>> = Mutex::new(None);
static LAST_STATUS: Mutex<Option<String>> = Mutex::new(None);
static INJECTED_PID: Mutex<Option<u32>> = Mutex::new(None);
/// PID we already asked UAC / inject for. Never prompt again for the same host.
static INJECT_ATTEMPTED_PID: Mutex<Option<u32>> = Mutex::new(None);
static GRACE_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
static ACTIVE: Mutex<Vec<u16>> = Mutex::new(Vec::new());

#[derive(Deserialize)]
struct HubMessage {
    kind: String,
    #[serde(default)]
    raw: String,
    #[serde(default)]
    message: String,
}

fn tap_port() -> u16 {
    std::env::var("REMOTE_MIC_HID_TAP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(TAP_PORT)
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

pub fn gadget_available() -> bool {
    gadget_dll_path().is_file()
}

pub fn set_status_callback(cb: impl Fn(String) + Send + 'static) {
    *STATUS_CB.lock().unwrap() = Some(Box::new(cb));
}

/// 返回最近一次旁路状态消息，供页面挂载时恢复显示。
pub fn last_status() -> Option<String> {
    LAST_STATUS.lock().unwrap().clone()
}

fn status(msg: &str) {
    core_log::log_info(&format!("[hid-tap] {msg}"));
    if let Ok(mut last) = LAST_STATUS.lock() {
        *last = Some(msg.to_string());
    }
    if let Ok(cb) = STATUS_CB.lock() {
        if let Some(f) = cb.as_ref() {
            f(msg.to_string());
        }
    }
}

fn arm_grace() {
    *GRACE_UNTIL.lock().unwrap() = Some(Instant::now() + INPUT_GRACE);
}

fn in_grace() -> bool {
    match *GRACE_UNTIL.lock().unwrap() {
        Some(until) if Instant::now() < until => true,
        Some(_) => {
            *GRACE_UNTIL.lock().unwrap() = None;
            false
        }
        None => false,
    }
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
pub fn start_after_atvv() {
    // 先准备运行时（从旧位置复制 DLL 到 ProgramData、写 config/script、设置 ACL），
    // 再检查 Gadget 是否可用。
    if let Err(e) = prepare_runtime() {
        status(&format!("返回/音量旁路未启用：{e}"));
        return;
    }
    if !gadget_available() {
        status("返回/音量旁路未启用：缺少 Frida Gadget，请运行 scripts/fetch-frida-gadget.ps1");
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        core_log::log_info("[hid-tap] 旁路服务已在运行");
        return;
    }
    std::thread::Builder::new()
        .name("rc003-hid-tap".into())
        .spawn(hub_loop)
        .ok();
    status("返回/音量旁路已启动（首次可能弹出 UAC）");
}

fn prepare_runtime() -> Result<(), String> {
    let dir = gadget_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mut wrote_new_file = false;

    // 运行时目录固定为 ProgramData（SYSTEM 可读）。DLL 缺失时直接提示运行下载脚本，
    // 不再从 LocalAppData 等历史位置复制兜底。
    let dll_path = gadget_dll_path();
    if !dll_path.is_file() {
        return Err(
            "frida-gadget.dll 未找到（请先运行 scripts/fetch-frida-gadget.ps1）".into(),
        );
    }

    // 写脚本。ProgramData 已给 Users Modify 权限，正常可写；失败则说明环境异常，阻断并提示。
    let script_path = dir.join("rc003-hid-tap.js");
    if !file_has_content(&script_path, GADGET_SCRIPT.as_bytes()) {
        std::fs::write(&script_path, GADGET_SCRIPT)
            .map_err(|e| format!("写入 rc003-hid-tap.js 失败: {e}"))?;
        wrote_new_file = true;
    }

    // 写 config（文件名必须与 DLL 名匹配：frida-gadget.config）
    let cfg = serde_json::json!({
        "interaction": {
            "type": "script",
            "path": "rc003-hid-tap.js",
            "parameters": { "host": "127.0.0.1", "port": tap_port() },
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

    // 给目录添加 SYSTEM/Administrators 读权限（不改变当前用户写权限），
    // 确保 WUDFHost（SYSTEM）能加载 DLL/脚本。只在实际写入/新建后执行。
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
                status("未找到 HOGP 宿主进程（HostPid），返回/音量旁路等待中");
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
            let mut attempted = INJECT_ATTEMPTED_PID.lock().unwrap();
            let mut injected = INJECTED_PID.lock().unwrap();
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
                        status(&format!("已注入 HOGP 宿主 pid={pid}，正在等待旁路连接"));
                    }
                    Ok(false) => {
                        status("UAC 被拒绝，返回/音量键仍不可用；普通键与语音不受影响。重新连接前不会再次弹窗。");
                        drop(listener);
                        std::thread::sleep(retry);
                        continue;
                    }
                    Err(e) => {
                        status(&format!(
                            "注入失败：{e}。普通键与语音不受影响，本次会话不再弹 UAC。"
                        ));
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
                let mut attempted = INJECT_ATTEMPTED_PID.lock().unwrap();
                let mut injected = INJECTED_PID.lock().unwrap();
                *attempted = None;
                *injected = None;
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    // 不设置读超时：Frida 脚本每 5 秒发心跳维持连接，
                    // 只有对端真正关闭时 read_line 才返回 Ok(0)，避免误报“关闭”。
                    let _ = stream.set_read_timeout(None);
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
        status(&format!(
            "返回/音量旁路已附着 pid={pid}，请按返回或音量键验证"
        ));
        arm_grace();
        serve_client(stream);
        *ACTIVE.lock().unwrap() = Vec::new();
        // 连接关闭也通过 status() 通知前端，避免只有日志没有状态。
        // 内部会重试，但对用户显示为“等待旁路重连”而不是“已关闭”。
        status("返回/音量旁路连接已断开，正在等待重连");
    }
}

fn serve_client(stream: std::net::TcpStream) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let Ok(msg) = serde_json::from_str::<HubMessage>(line.trim()) else {
            continue;
        };
        match msg.kind.as_str() {
            "gatt_read" => {
                if let Some(bytes) = decode_hex(&msg.raw) {
                    on_ioctl_bytes(&bytes);
                }
            }
            "gatt_read_other" => {
                core_log::log_warn(&format!(
                    "[hid-tap] 目标 IOCTL 但长度异常: length={}, raw={}",
                    msg.message, msg.raw
                ));
            }
            "error" => core_log::log_warn(&format!("[hid-tap] 旁路错误: {}", msg.message)),
            _ => {}
        }
    }
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

fn on_ioctl_bytes(data: &[u8]) {
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

    let mut next = hogp_payload_usages(payload);
    next.sort_unstable();
    next.dedup();

    // 记录所有解析出的 usage，包括未知 usage，便于校准真实信号。
    if !next.is_empty() {
        let usages: Vec<String> = next.iter().map(|u| format!("0x{u:04X}")).collect();
        core_log::log_info(&format!("[hid-tap] HOGP 载荷 usage: {}", usages.join(", ")));
    }

    let mut prev = ACTIVE.lock().unwrap();
    if *prev == next {
        return;
    }
    let pressed: Vec<u16> = next.iter().copied().filter(|u| !prev.contains(u)).collect();
    let released: Vec<u16> = prev.iter().copied().filter(|u| !next.contains(u)).collect();
    *prev = next;
    drop(prev);
    if in_grace() {
        return;
    }
    for usage in pressed {
        emit_usage(usage, true);
    }
    for usage in released {
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
    core_log::log_info(&format!(
        "[hid-tap] 特殊按键 usage=0x{usage:02X} vkey={vkey} 按下={pressed}"
    ));
    raw_input::emit(raw_input::RawInputEvent {
        vkey,
        make_code: 0,
        pressed,
    });
}

fn hex_bytes(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect()
}

fn request_inject(pid: u32) -> Result<bool, String> {
    if is_elevated() {
        inject_into(pid)?;
        return Ok(true);
    }
    elevate_and_inject(pid)
}

fn last_inject_error_path() -> PathBuf {
    gadget_dir().join("last-inject-error.txt")
}

fn write_last_inject_error(msg: &str) {
    let _ = std::fs::write(last_inject_error_path(), msg);
}

fn take_last_inject_error() -> Option<String> {
    let path = last_inject_error_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let text = text.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn gadget_dll() -> Result<PathBuf, String> {
    let path = gadget_dll_path();
    if path.is_file() {
        Ok(path)
    } else {
        Err("frida-gadget.dll not found".into())
    }
}

#[cfg(windows)]
fn find_rc003_host_pid() -> Option<u32> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_READ, REG_DWORD, REG_QWORD, REG_SZ, REG_VALUE_TYPE,
    };

    fn open_sub(parent: HKEY, path: &str) -> Option<HKEY> {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(parent, PCWSTR(wide.as_ptr()), Some(0), KEY_READ, &mut key) }
            .is_err()
        {
            return None;
        }
        Some(key)
    }

    fn enum_key(parent: HKEY, index: u32) -> Option<String> {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        if unsafe {
            RegEnumKeyExW(
                parent,
                index,
                Some(PWSTR(buf.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        }
        .is_err()
        {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }

    fn query_host_pid(key: HKEY) -> Option<u32> {
        let name: Vec<u16> = "HostPid".encode_utf16().chain(std::iter::once(0)).collect();
        let mut kind = REG_VALUE_TYPE::default();
        let mut data = [0u8; 16];
        let mut size = data.len() as u32;
        if unsafe {
            RegQueryValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr()),
                Some(&mut size),
            )
        }
        .is_err()
        {
            return None;
        }
        if kind == REG_DWORD && size >= 4 {
            return Some(u32::from_le_bytes(data[0..4].try_into().ok()?));
        }
        // Windows 10/11 HidOverGatt writes HostPid as REG_QWORD.
        if kind == REG_QWORD && size >= 8 {
            let pid = u64::from_le_bytes(data[0..8].try_into().ok()?);
            if pid > 0 && pid <= u64::from(u32::MAX) {
                return Some(pid as u32);
            }
            return None;
        }
        if kind == REG_SZ {
            let n = (size as usize / 2).saturating_sub(1);
            let wide: Vec<u16> = data[..size as usize]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take(n)
                .collect();
            return String::from_utf16_lossy(&wide).parse().ok();
        }
        core_log::log_warn(&format!(
            "[hid-tap] HostPid 类型未识别 kind={kind:?} size={size}"
        ));
        None
    }

    let root = open_sub(
        HKEY_LOCAL_MACHINE,
        r"SYSTEM\CurrentControlSet\Enum\BTHLEDevice",
    )?;
    let mut service_i = 0;
    let mut found = None;
    while let Some(service) = enum_key(root, service_i) {
        service_i += 1;
        let folded = service.to_ascii_lowercase();
        if !folded.contains("00001812-0000-1000-8000-00805f9b34fb") {
            continue;
        }
        if !folded.contains("vid&012717") || !folded.contains("pid&32b8") {
            continue;
        }
        let Some(svc_key) = open_sub(root, &service) else {
            continue;
        };
        let mut inst_i = 0;
        while let Some(instance) = enum_key(svc_key, inst_i) {
            inst_i += 1;
            let diag = format!("{instance}\\Device Parameters\\WUDFDiagnosticInfo");
            if let Some(diag_key) = open_sub(svc_key, &diag) {
                if let Some(pid) = query_host_pid(diag_key) {
                    if pid > 0 {
                        found = Some(pid);
                    }
                }
                unsafe {
                    let _ = RegCloseKey(diag_key);
                }
            }
            if found.is_some() {
                break;
            }
        }
        unsafe {
            let _ = RegCloseKey(svc_key);
        }
        if found.is_some() {
            break;
        }
    }
    unsafe {
        let _ = RegCloseKey(root);
    }
    found
}

#[cfg(not(windows))]
fn find_rc003_host_pid() -> Option<u32> {
    None
}

#[cfg(windows)]
fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
fn is_elevated() -> bool {
    false
}

#[cfg(windows)]
fn enable_debug_privilege() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, SetLastError, ERROR_NOT_ALL_ASSIGNED, HANDLE, LUID, WIN32_ERROR,
    };
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|e| e.to_string())?;
        let mut luid = LUID::default();
        let result = LookupPrivilegeValueW(None, w!("SeDebugPrivilege"), &mut luid)
            .map_err(|e| e.to_string());
        if result.is_err() {
            let _ = CloseHandle(token);
            return result;
        }
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [windows::Win32::Security::LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        SetLastError(WIN32_ERROR(0));
        let adj = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
        let last_error = GetLastError();
        let _ = CloseHandle(token);
        adj.map_err(|e| e.to_string())?;
        if last_error == ERROR_NOT_ALL_ASSIGNED {
            return Err("SeDebugPrivilege 未授予".into());
        }
        let _ = tp;
        Ok(())
    }
}

#[cfg(windows)]
fn process_is_wudfhost(pid: u32) -> Result<bool, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map_err(|e| e.to_string())?;
        let mut buf = [0u16; 512];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        ok.map_err(|e| e.to_string())?;
        let name = String::from_utf16_lossy(&buf[..size as usize]).to_ascii_lowercase();
        Ok(name.ends_with("wudfhost.exe"))
    }
}

#[cfg(windows)]
fn inject_into(pid: u32) -> Result<(), String> {
    use windows::core::{s, w};
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows::Win32::System::Threading::{
        CreateRemoteThread, OpenProcess, WaitForSingleObject, PROCESS_CREATE_THREAD,
        PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };

    enable_debug_privilege().map_err(|e| format!("无法启用 SeDebugPrivilege: {e}"))?;
    if find_rc003_host_pid() != Some(pid) {
        return Err("HOGP host pid changed before inject".into());
    }
    if !process_is_wudfhost(pid)? {
        return Err("target is not WUDFHost.exe".into());
    }
    let dll = gadget_dll()?;
    let dll_os = dll
        .canonicalize()
        .unwrap_or(dll)
        .into_os_string()
        .into_string()
        .map_err(|_| "gadget path is not UTF-8".to_string())?;
    // LoadLibraryW 需要 DOS 路径；去掉 \\?\ 前缀，否则远程 LoadLibraryW 可能返回 NULL。
    let dll_os = dll_os.strip_prefix(r"\\?\").unwrap_or(&dll_os).to_string();

    unsafe {
        let access = PROCESS_CREATE_THREAD
            | PROCESS_QUERY_INFORMATION
            | PROCESS_VM_OPERATION
            | PROCESS_VM_WRITE
            | PROCESS_VM_READ;
        let process = OpenProcess(access, false, pid).map_err(|e| e.to_string())?;
        let encoded: Vec<u16> = dll_os.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = encoded.len() * 2;
        let remote = VirtualAllocEx(
            process,
            None,
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote.is_null() {
            let _ = CloseHandle(process);
            return Err("VirtualAllocEx failed".into());
        }
        let written_ok =
            WriteProcessMemory(process, remote, encoded.as_ptr() as *const _, bytes, None);
        if written_ok.is_err() {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return Err("WriteProcessMemory failed".into());
        }
        let k32 = GetModuleHandleW(w!("kernel32.dll")).map_err(|e| e.to_string())?;
        let load = GetProcAddress(k32, s!("LoadLibraryW")).ok_or("LoadLibraryW missing")?;
        let start: windows::Win32::System::Threading::LPTHREAD_START_ROUTINE =
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
            >(load));
        let thread = CreateRemoteThread(process, None, 0, start, Some(remote), 0, None);
        let thread = match thread {
            Ok(t) => t,
            Err(e) => {
                let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
                let _ = CloseHandle(process);
                return Err(e.to_string());
            }
        };
        let wait = WaitForSingleObject(thread, 15_000);
        let mut thread_code = 0u32;
        let _ = windows::Win32::System::Threading::GetExitCodeThread(thread, &mut thread_code);
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);
        if wait != WAIT_OBJECT_0 {
            return Err("inject wait timed out".into());
        }
        if thread_code == 0 {
            return Err("LoadLibraryW 返回空（Gadget 未被加载，可能被内存完整性拦截）".into());
        }
    }
    let _ = Path::new(&dll_os);
    Ok(())
}

#[cfg(not(windows))]
fn inject_into(_pid: u32) -> Result<(), String> {
    Err("Windows only".into())
}

#[cfg(windows)]
fn elevate_and_inject(pid: u32) -> Result<bool, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_s = exe.to_string_lossy();
    let params = format!("--hid-tap-inject --pid {pid}");
    let exe_w: Vec<u16> = exe_s.encode_utf16().chain(std::iter::once(0)).collect();
    let params_w: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "runas".encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(exe_w.as_ptr()),
            lpParameters: PCWSTR(params_w.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        if ShellExecuteExW(&mut info).is_err() {
            return Ok(false);
        }
        if !info.hProcess.is_invalid() {
            let wait = WaitForSingleObject(info.hProcess, 30_000);
            let mut code = 0u32;
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
            let _ = CloseHandle(info.hProcess);
            if wait != WAIT_OBJECT_0 {
                return Err("提权注入未在 30 秒内结束（UAC 可能未确认）".into());
            }
            if code != 0 {
                let detail = take_last_inject_error()
                    .unwrap_or_else(|| format!("提权注入失败，退出码 {code}"));
                return Err(detail);
            }
        }
    }
    Ok(true)
}

#[cfg(not(windows))]
fn elevate_and_inject(_pid: u32) -> Result<bool, String> {
    Ok(false)
}
