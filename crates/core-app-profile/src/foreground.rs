//! 前台窗口所属进程的可执行文件名。
//!
//! 调度器的 tick 线程每 25ms 要判断一次前台应用配置，而这里的两项查询
//! 都需要跨进程调用（`OpenProcess`、以及对其它进程窗口的 `GetWindowText`，
//! 后者本质是同步 `SendMessage(WM_GETTEXT)`，目标进程卡死时会被拖住）。
//! 因此统一缓存 800ms：切换应用最多延迟 1 秒内生效，但把高频跨进程调用
//! 降到了约 1 次/秒，前台应用卡死也不再能让 tick 线程反复被拖。
//!
//! 缓存刷新时顺手记录一份「最近前台应用」历史（见 [`recent_foreground_apps`]）。
//! 「新建应用配置」对话框要填的正是**用户刚在用、现在已经切走的那个应用**，
//! 而打开本应用时前台必然是本应用自己——所以不能只问「当前前台」。

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// 前台上下文缓存有效期。
#[cfg(target_os = "windows")]
const FOREGROUND_CACHE_TTL: std::time::Duration = std::time::Duration::from_millis(800);

/// 「最近前台」最多记几条。够填出常用应用，又不至于把对话框撑爆。
const RECENT_FOREGROUND_LIMIT: usize = 5;

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

/// 最近出现过的前台应用（不含本应用自己），最新的在前。
static RECENT_FOREGROUNDS: Mutex<VecDeque<ForegroundApp>> = Mutex::new(VecDeque::new());

/// 一个最近出现过的前台应用。
///
/// 供「新建应用配置」对话框一键填入：这里给的永远是**别的应用**，
/// 不会把 Remote Mic 自己（`remote-mic.exe`）当成候选。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ForegroundApp {
    /// 可执行文件名，例如 `ZCode.exe`。
    pub process: String,
    /// 当时的窗口标题；只按标题匹配的目标（Chrome PWA、WSL 里的服务）靠它。
    pub title: Option<String>,
}

/// 本进程自己的可执行文件名（例如 `remote-mic.exe`）。
///
/// 只算一次：程序运行期间不会改名。取不到时返回 `None`，此时不做排除
/// （宁可多列一个候选，也不要因为读不到路径把自身漏进来）。
fn own_executable_name() -> Option<&'static str> {
    static OWN: OnceLock<Option<String>> = OnceLock::new();
    OWN.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|path| file_name(&path.to_string_lossy()))
    })
    .as_deref()
}

/// 前台快照刷新时记录一次历史（Windows 上才有前台快照可记）。
#[cfg(target_os = "windows")]
fn record_recent(process: &Option<String>, title: &Option<String>) {
    let Some(process) = process.as_deref() else {
        return;
    };
    let Some(process) = file_name(process) else {
        return;
    };
    let mut guard = match RECENT_FOREGROUNDS.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    push_recent(
        &mut guard,
        ForegroundApp {
            process,
            title: title
                .as_deref()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(truncate_title),
        },
        own_executable_name(),
    );
}

/// 把一条记录压进历史：排除本应用、按进程名去重（同进程保留最新标题）、
/// 保留最近 [`RECENT_FOREGROUND_LIMIT`] 条。
///
/// 抽成纯函数是为了能单测——真机历史依赖「实际上切过窗口」，不适合做断言。
fn push_recent(list: &mut VecDeque<ForegroundApp>, entry: ForegroundApp, own_exe: Option<&str>) {
    if let Some(own) = own_exe {
        if entry.process.eq_ignore_ascii_case(own) {
            return;
        }
    }
    if entry.process.trim().is_empty() {
        return;
    }
    // 进程名大小写不敏感（Windows 语义），同一应用只留最近一条。
    list.retain(|old| !old.process.eq_ignore_ascii_case(&entry.process));
    list.push_front(entry);
    while list.len() > RECENT_FOREGROUND_LIMIT {
        list.pop_back();
    }
}

/// 标题只用于展示和手抄，截断到 120 字符防止异常长标题撑爆 UI。
fn truncate_title(title: &str) -> String {
    const MAX: usize = 120;
    let trimmed = title.trim();
    if trimmed.chars().count() <= MAX {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(MAX).collect();
    out.push('…');
    out
}

/// 最近出现过的前台应用（不含本应用自己），最新的在前。
pub fn recent_foreground_apps() -> Vec<ForegroundApp> {
    let guard = match RECENT_FOREGROUNDS.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    guard.iter().cloned().collect()
}

/// 「新建应用配置」对话框的候选列表。
///
/// 顺序：当前前台（不是本应用自己时）→ 最近前台历史（去重）。
/// 对话框打开时前台通常是本应用，所以**主要靠历史**——这正是用户刚切走的
/// 那个目标应用；空列表说明应用启动后还没切过别的窗口。
pub fn foreground_candidates() -> Vec<ForegroundApp> {
    let mut out: Vec<ForegroundApp> = Vec::new();

    let current_process = foreground_process_name();
    let is_self = match (current_process.as_deref(), own_executable_name()) {
        (Some(process), Some(own)) => process.eq_ignore_ascii_case(own),
        _ => false,
    };
    if let Some(process) = current_process.filter(|p| !p.trim().is_empty()) {
        if !is_self {
            out.push(ForegroundApp {
                process,
                title: foreground_window_title()
                    .map(|t| truncate_title(&t))
                    .filter(|t| !t.trim().is_empty()),
            });
        }
    }

    for app in recent_foreground_apps() {
        if out
            .iter()
            .any(|seen| seen.process.eq_ignore_ascii_case(&app.process))
        {
            continue;
        }
        out.push(app);
    }
    out
}

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
    // 历史记录放在缓存锁**外面**做：两把锁互不嵌套，避免将来加代码时死锁。
    drop(guard);
    record_recent(&out.0, &out.1);
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

    fn app(process: &str, title: Option<&str>) -> ForegroundApp {
        ForegroundApp {
            process: process.into(),
            title: title.map(str::to_string),
        }
    }

    #[test]
    fn push_recent_skips_own_process() {
        // 「新建应用」时前台必然是 Remote Mic 自己，绝不能把它填进去。
        let mut list = VecDeque::new();
        push_recent(
            &mut list,
            app("remote-mic.exe", None),
            Some("remote-mic.exe"),
        );
        assert!(list.is_empty());
        // 大小写不敏感（Windows 文件名语义）。
        push_recent(
            &mut list,
            app("Remote-Mic.EXE", None),
            Some("remote-mic.exe"),
        );
        assert!(list.is_empty());
        // 别的应用正常记录。
        push_recent(
            &mut list,
            app("ZCode.exe", Some("ZCode")),
            Some("remote-mic.exe"),
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].process, "ZCode.exe");
    }

    #[test]
    fn push_recent_dedupes_by_process_and_keeps_latest_title() {
        let mut list = VecDeque::new();
        push_recent(&mut list, app("chrome.exe", Some("旧页面")), None);
        push_recent(&mut list, app("ZCode.exe", None), None);
        push_recent(&mut list, app("chrome.exe", Some("新页面")), None);

        // 最新的在前，同一进程只留一条，标题取最新的。
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].process, "chrome.exe");
        assert_eq!(list[0].title.as_deref(), Some("新页面"));
        assert_eq!(list[1].process, "ZCode.exe");
    }

    #[test]
    fn push_recent_caps_history_length() {
        let mut list = VecDeque::new();
        for i in 0..10 {
            push_recent(&mut list, app(&format!("app{i}.exe"), None), None);
        }
        assert_eq!(list.len(), RECENT_FOREGROUND_LIMIT);
        // 留下的是最近的那几条，最早的被挤掉。
        assert_eq!(list[0].process, "app9.exe");
        assert!(!list.iter().any(|e| e.process == "app0.exe"));
    }

    #[test]
    fn truncate_title_keeps_short_titles_and_caps_long_ones() {
        assert_eq!(truncate_title("  ZCode  "), "ZCode");
        let long = "标题".repeat(100);
        let out = truncate_title(&long);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 121);
    }
}
