//! 前台窗口所属进程的可执行文件名。
//!
//! 调度器的 tick 线程每 25ms 要判断一次前台应用配置，而这里的两项查询
//! 都需要跨进程调用（`OpenProcess`、以及对其它进程窗口的 `GetWindowText`，
//! 后者本质是同步 `SendMessage(WM_GETTEXT)`，目标进程卡死时会被拖住）。
//! 因此统一缓存 800ms：切换应用最多延迟 1 秒内生效，但把高频跨进程调用
//! 降到了约 1 次/秒，前台应用卡死也不再能让 tick 线程反复被拖。

/// 前台上下文缓存有效期。
#[cfg(target_os = "windows")]
const FOREGROUND_CACHE_TTL: std::time::Duration = std::time::Duration::from_millis(800);

/// 一次前台窗口快照（进程名 + 标题），带缓存时间戳。
#[cfg(target_os = "windows")]
#[derive(Clone)]
struct ForegroundSnapshot {
    at: std::time::Instant,
    process: Option<String>,
    title: Option<String>,
}

#[cfg(target_os = "windows")]
static FOREGROUND_CACHE: std::sync::Mutex<Option<ForegroundSnapshot>> = std::sync::Mutex::new(None);

/// 取当前前台窗口的（进程名, 标题）快照，带 800ms 缓存。
#[cfg(target_os = "windows")]
fn foreground_snapshot() -> (Option<String>, Option<String>) {
    use std::sync::MutexGuard;

    let mut guard: MutexGuard<Option<ForegroundSnapshot>> = match FOREGROUND_CACHE.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    if let Some(snap) = guard.as_ref() {
        if snap.at.elapsed() < FOREGROUND_CACHE_TTL {
            return (snap.process.clone(), snap.title.clone());
        }
    }
    let snap = ForegroundSnapshot {
        at: std::time::Instant::now(),
        process: query_foreground_process_name(),
        title: query_foreground_window_title(),
    };
    let out = (snap.process.clone(), snap.title.clone());
    *guard = Some(snap);
    out
}

/// 直接查询前台窗口的进程名（不走缓存）。
#[cfg(target_os = "windows")]
fn query_foreground_process_name() -> Option<String> {
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

/// 取当前前台窗口的进程可执行文件名（例如 `Codex.exe`）。
///
/// 读不到时返回 `None`（例如没有前台窗口、进程已退出、或权限不足）。
/// 结果缓存 800ms，见模块文档。
#[cfg(target_os = "windows")]
pub fn foreground_process_name() -> Option<String> {
    foreground_snapshot().0
}

/// 取当前前台窗口的标题。
///
/// 用于进程名认不出来的目标（Chrome PWA、跑在 WSL 里的服务）。
///
/// # 为什么自家窗口直接返回 None
///
/// `GetWindowText` 对**属于本进程**的窗口是走 `SendMessage(WM_GETTEXT)` 的，
/// 也就是一次跨线程同步——对方线程不泵消息就永远等下去。快捷菜单打开时前台
/// 窗口正是我们自己的窗口，调用方又常在持锁状态下取标题，于是：
/// tick 线程持锁等主线程处理消息、主线程正等着取同一把锁——死锁。
/// 自家窗口本来也没有匹配应用配置的必要，索性直接不看。
#[cfg(target_os = "windows")]
pub fn foreground_window_title() -> Option<String> {
    foreground_snapshot().1
}

/// 直接查询前台窗口标题（不走缓存）。
#[cfg(target_os = "windows")]
fn query_foreground_window_title() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || belongs_to_current_process(hwnd) {
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

/// 该窗口是否属于当前进程。
///
/// 判断「能不能对它用会发消息的窗口 API」时的依据。
#[cfg(target_os = "windows")]
pub(crate) fn belongs_to_current_process(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != 0 && pid == GetCurrentProcessId()
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
