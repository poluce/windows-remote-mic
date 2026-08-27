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
static INJECTED_PID: Mutex<Option<u32>> = Mutex::new(None);
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
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base)
        .join("RemoteMic")
        .join("RC003")
        .join("hid-tap")
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

fn status(msg: &str) {
    core_log::log_info(&format!("[hid-tap] {msg}"));
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
    let Some(pos) = args.iter().position(|a| a == "--hid-tap-inject") else {
        return false;
    };
    let pid = args
        .iter()
        .skip(pos + 1)
        .position(|a| a == "--pid")
        .and_then(|i| args.get(pos + 1 + i + 1))
        .and_then(|s| s.parse::<u32>().ok());
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
            std::process::exit(1);
        }
    }
}

/// 启动 localhost 服务，并在需要时请求提权以执行注入。
/// 可安全地多次调用。绝不打开 HID GATT 服务。
pub fn start_after_atvv() {
    if !gadget_available() {
        status("返回/音量旁路未启用：缺少 Frida Gadget，请运行 scripts/fetch-frida-gadget.ps1");
        return;
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        core_log::log_info("[hid-tap] 旁路服务已在运行");
        return;
    }
    if let Err(e) = prepare_runtime() {
        RUNNING.store(false, Ordering::SeqCst);
        status(&format!("返回/音量旁路准备失败：{e}"));
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
    let script_path = dir.join("rc003-hid-tap.js");
    std::fs::write(&script_path, GADGET_SCRIPT).map_err(|e| e.to_string())?;
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
    std::fs::write(
        dir.join("frida-gadget.config"),
        serde_json::to_vec_pretty(&cfg).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn hub_loop() {
    let port = tap_port();
    let retry = Duration::from_secs(2);
    loop {
        let Some(pid) = find_rc003_host_pid() else {
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
            let mut injected = INJECTED_PID.lock().unwrap();
            if *injected != Some(pid) {
                match request_inject(pid) {
                    Ok(true) => {
                        *injected = Some(pid);
                        status(&format!(
                            "已请求注入 HOGP 宿主 pid={pid}（若弹出 UAC 请允许）"
                        ));
                    }
                    Ok(false) => {
                        status("UAC 被拒绝，返回/音量键仍不可用；普通键与语音不受影响");
                        drop(listener);
                        std::thread::sleep(Duration::from_secs(8));
                        continue;
                    }
                    Err(e) => {
                        status(&format!("注入失败：{e}"));
                        drop(listener);
                        std::thread::sleep(retry);
                        continue;
                    }
                }
            }
        }

        let deadline = Instant::now() + Duration::from_secs(1);
        let mut client = None;
        while Instant::now() < deadline {
            if find_rc003_host_pid() != Some(pid) {
                *INJECTED_PID.lock().unwrap() = None;
                break;
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
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
        core_log::log_info("[hid-tap] 旁路连接已关闭，稍后重试");
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
        KEY_READ, REG_DWORD, REG_SZ, REG_VALUE_TYPE,
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
        if kind == REG_SZ {
            let n = (size as usize / 2).saturating_sub(1);
            let wide: Vec<u16> = data[..size as usize]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take(n)
                .collect();
            return String::from_utf16_lossy(&wide).parse().ok();
        }
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
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
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
        let adj = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
        let _ = CloseHandle(token);
        adj.map_err(|e| e.to_string())?;
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

    if find_rc003_host_pid() != Some(pid) {
        return Err("HOGP host pid changed before inject".into());
    }
    if !process_is_wudfhost(pid)? {
        return Err("target is not WUDFHost.exe".into());
    }
    let _ = enable_debug_privilege();
    let dll = gadget_dll()?;
    let dll_os = dll
        .canonicalize()
        .unwrap_or(dll)
        .into_os_string()
        .into_string()
        .map_err(|_| "gadget path is not UTF-8".to_string())?;

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
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);
        if wait != WAIT_OBJECT_0 {
            return Err("inject wait timed out".into());
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
    use windows::Win32::System::Threading::WaitForSingleObject;
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
            let _ = WaitForSingleObject(info.hProcess, 30_000);
            if WaitForSingleObject(info.hProcess, 0) != WAIT_OBJECT_0 {
                core_log::log_warn(
                    "[hid-tap] elevated injector still running; hub will wait for gadget",
                );
            }
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(true)
}

#[cfg(not(windows))]
fn elevate_and_inject(_pid: u32) -> Result<bool, String> {
    Ok(false)
}
