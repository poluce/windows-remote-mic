//! 向 WUDFHost 注入 Frida gadget：提权（UAC）/ 远程线程 LoadLibrary / 目标校验。
//!
//! 这些函数只在 Windows 上真正工作；非 Windows 平台提供空实现，便于编译与测试。

use std::path::PathBuf;

use super::{gadget_dir, gadget_dll_path};

pub(super) fn request_inject(pid: u32) -> Result<bool, String> {
    if is_elevated() {
        inject_into(pid)?;
        return Ok(true);
    }
    elevate_and_inject(pid)
}

fn last_inject_error_path() -> PathBuf {
    gadget_dir().join("last-inject-error.txt")
}

pub(super) fn write_last_inject_error(msg: &str) {
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
pub(super) fn find_rc003_host_pid() -> Option<u32> {
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
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
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
pub(super) fn find_rc003_host_pid() -> Option<u32> {
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
pub(super) fn inject_into(pid: u32) -> Result<(), String> {
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
    Ok(())
}

#[cfg(not(windows))]
pub(super) fn inject_into(_pid: u32) -> Result<(), String> {
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
