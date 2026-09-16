//! 前台窗口所属进程的可执行文件名。

/// 取当前前台窗口的进程可执行文件名（例如 `Codex.exe`）。
///
/// 读不到时返回 `None`（例如没有前台窗口、进程已退出、或权限不足）。
#[cfg(target_os = "windows")]
pub fn foreground_process_name() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        process_name_of_pid(pid)
    }
}

/// 取当前前台窗口的标题。
///
/// 用于进程名认不出来的目标（Chrome PWA、跑在 WSL 里的服务）。
#[cfg(target_os = "windows")]
pub fn foreground_window_title() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let written = GetWindowTextW(hwnd, &mut buf);
        if written <= 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&buf[..written as usize]);
        (!title.trim().is_empty()).then_some(title)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn foreground_window_title() -> Option<String> {
    None
}

/// 由进程 ID 取可执行文件名（例如 `Codex.exe`）。
#[cfg(target_os = "windows")]
pub(crate) fn process_name_of_pid(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return None;
    }

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let queried = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(handle);
        queried.ok()?;

        let full_path = String::from_utf16_lossy(&buf[..len as usize]);
        file_name(&full_path)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn foreground_process_name() -> Option<String> {
    None
}

/// 从完整路径里取可执行文件名。
pub fn file_name(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_handles_windows_paths() {
        assert_eq!(
            file_name(r"C:\Program Files\Codex\Codex.exe").as_deref(),
            Some("Codex.exe")
        );
        assert_eq!(file_name("Codex.exe").as_deref(), Some("Codex.exe"));
        assert_eq!(file_name("/usr/bin/foo").as_deref(), Some("foo"));
        assert_eq!(file_name(r"C:\dir\"), None);
        assert_eq!(file_name("   "), None);
    }

    /// 真机冒烟：走一遍 GetForegroundWindow → PID → exe 名。
    /// 无前台窗口时（如无人值守环境）允许返回 None，但不能 panic。
    /// `cargo test -p core-app-profile -- --nocapture` 可以看到真实取值。
    #[test]
    fn foreground_process_name_smoke() {
        let name = foreground_process_name();
        eprintln!("foreground = {name:?}");
        if let Some(n) = &name {
            assert!(
                !n.trim().is_empty() && !n.contains('\\'),
                "应只返回文件名，实际：{n}"
            );
        }
    }
}
