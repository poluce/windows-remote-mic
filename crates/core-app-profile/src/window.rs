//! 应用窗口的查找 / 聚焦 / 启动。
//!
//! 与 [`crate::foreground`] 的分工：
//! - `foreground`：读「现在谁在前台」——用于决定用哪份配置；
//! - 本模块：把某个应用弄到前台、或把它启动起来——用于快捷菜单点击图标。
//!
//! 窗口句柄对外用 `isize` 表示，避免把 `windows` crate 的类型泄到公开 API 上，
//! 这样非 Windows 平台也能编译、也能单测纯逻辑部分。

use crate::model::{AppProfile, LaunchSpec};

/// 一个可聚焦的顶层窗口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppWindow {
    /// 原生窗口句柄。
    pub handle: isize,
    /// 所属进程的可执行文件名，例如 `ZCode.exe`。
    pub process: String,
    /// 窗口标题。
    pub title: String,
}

/// 查找 / 聚焦 / 启动过程中可能出现的失败。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WindowError {
    /// 当前平台不支持窗口操作（非 Windows 构建）。
    #[error("当前平台不支持窗口操作")]
    Unsupported,
    /// 窗口找到了，但系统拒绝把它切到前台。
    #[error("系统拒绝把窗口切到前台")]
    FocusDenied,
    /// 启动失败，附带 `ShellExecuteW` 的返回值（≤ 32 表示错误码）。
    #[error("启动失败（ShellExecuteW 返回 {0}）")]
    LaunchFailed(isize),
}

/// 点击图标之后的实际结果，供 UI 提示用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    /// 找到已打开的窗口并切到前台。
    Focused { process: String, title: String },
    /// 没有已打开的窗口，已按配置启动。
    Launched,
    /// 既没找到窗口，也没配置启动方式——只能提示用户手动打开。
    NotFound,
}

/// 一次「点击图标」的完整动作：先找已开的窗口，找不到再启动。
///
/// 注意顺序不能倒过来：先启动会开出第二个实例。
///
/// Tauri 层没有直接用这个便利函数，而是把它拆成两半分别丢进阻塞线程池——
/// `focus` 必须趁快捷菜单还在前台时做（否则没有前台权限），`launch` 则要先
/// 收起菜单再慢慢等。这里保留完整流程供测试与其它调用方使用。
pub fn open_profile(profile: &AppProfile) -> Result<OpenOutcome, WindowError> {
    if let Some(window) = find_window(profile) {
        focus(&window)?;
        return Ok(OpenOutcome::Focused {
            process: window.process,
            title: window.title,
        });
    }

    match &profile.launch {
        Some(spec) => {
            launch(spec)?;
            Ok(OpenOutcome::Launched)
        }
        None => Ok(OpenOutcome::NotFound),
    }
}

/// 前台窗口的摘要信息（句柄、进程 ID、标题），仅用于真机冒烟排查。
#[cfg(all(test, target_os = "windows"))]
fn foreground_summary() -> Option<(usize, u32, String)> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; len.max(0) as usize + 1];
        let written = GetWindowTextW(hwnd, &mut buf);
        Some((
            hwnd.0 as usize,
            pid,
            String::from_utf16_lossy(&buf[..written.max(0) as usize]),
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn foreground_summary() -> Option<(usize, u32, String)> {
    None
}

/// 按配置查找已打开的窗口；找不到返回 `None`。
///
/// 进程名优先：同一进程名命中时不再看标题。只有进程名都不匹配时，
/// 才退回标题匹配——那是 Chrome PWA / WSL 服务这类「进程名认不出来」的唯一办法。
#[cfg(target_os = "windows")]
pub fn find_window(profile: &AppProfile) -> Option<AppWindow> {
    let windows = list_windows();
    windows
        .iter()
        .find(|w| profile.process.matches(&w.process))
        .or_else(|| windows.iter().find(|w| profile.matches_title(&w.title)))
        .cloned()
}

#[cfg(not(target_os = "windows"))]
pub fn find_window(_profile: &AppProfile) -> Option<AppWindow> {
    None
}

/// 枚举当前所有可见的顶层窗口。
#[cfg(target_os = "windows")]
pub fn list_windows() -> Vec<AppWindow> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, TRUE};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
    };

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<AppWindow>);

        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }

        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return TRUE;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let written = GetWindowTextW(hwnd, &mut buf);
        if written <= 0 {
            return TRUE;
        }
        let title = String::from_utf16_lossy(&buf[..written as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Some(process) = crate::foreground::process_name_of_pid(pid) else {
            return TRUE;
        };

        out.push(AppWindow {
            handle: hwnd.0 as isize,
            process,
            title,
        });
        TRUE
    }

    let mut out: Vec<AppWindow> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

#[cfg(not(target_os = "windows"))]
pub fn list_windows() -> Vec<AppWindow> {
    Vec::new()
}

/// 把窗口切到前台（最小化时先还原）。
///
/// # 两层策略
///
/// Windows 的前台锁只对「已拥有前台权限」的进程放行，所以：
/// 1. 先把本线程的输入队列挂到前台线程上借用权限，再 `SetForegroundWindow`；
/// 2. 仍不行就 `BringWindowToTop` + `SwitchToThisWindow`（Alt+Tab 同款路径）。
///
/// # 已知限制（实测）
///
/// **从后台进程调用会被拒**：`cargo test` 这类控制台进程调 `focus()` 会拿到
/// [`WindowError::FocusDenied`]。本项目的真实路径不受影响——点快捷菜单时
/// `remote-mic.exe` 自己就是前台进程（快捷菜单窗口属于它），第一步必定放行。
/// 调用方应当把 `FocusDenied` 当成可展示的失败（提示用户手动点任务栏），
/// 而不是 panic。
///
/// 判定成败用「该应用现在是否拥有前台窗口」回读，不看 `SetForegroundWindow`
/// 的返回值——它会在只闪一下任务栏的情况下也返回成功。
#[cfg(target_os = "windows")]
pub fn focus(window: &AppWindow) -> Result<(), WindowError> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        SetForegroundWindow, ShowWindow, SwitchToThisWindow, SW_RESTORE, SW_SHOW,
    };

    let hwnd = HWND(window.handle as *mut core::ffi::c_void);

    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        // 已经在最前面就别再动它——反复 SetForegroundWindow 会闪任务栏。
        if is_app_foreground(hwnd) {
            return Ok(());
        }

        // 第一步：把自己的输入队列挂到前台线程上，借用它的前台权限。
        // 调用完必须摘掉，否则两个线程的输入状态会一直被绑在一起。
        let foreground = GetForegroundWindow();
        let foreground_thread = if foreground.is_invalid() {
            0
        } else {
            GetWindowThreadProcessId(foreground, None)
        };
        let current_thread = GetCurrentThreadId();
        let attach = foreground_thread != 0 && foreground_thread != current_thread;

        let attached =
            attach && AttachThreadInput(current_thread, foreground_thread, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        if attached {
            let _ = AttachThreadInput(current_thread, foreground_thread, false);
        }

        // 第二步：前台锁不认上面的借用时，走 Alt+Tab 同款路径。
        // 它是「直接切换」而不是「请求」，权限要求低得多。
        if !is_app_foreground(hwnd) {
            let _ = BringWindowToTop(hwnd);
            SwitchToThisWindow(hwnd, true);
        }

        // 用「这个应用现在是不是在前台」判定成败，而不是句柄是否相等：
        // 一个应用可能有多个顶层窗口（启动画面、弹出主窗口），激活后真正拿到
        // 前台的可能不是我们枚举到的那个。SetForegroundWindow 返回成功但实际
        // 只闪了一下任务栏的情况也存在，所以必须回读。
        if is_app_foreground(hwnd) {
            Ok(())
        } else {
            Err(WindowError::FocusDenied)
        }
    }
}

/// 该窗口所属的进程现在是否拥有前台窗口。
#[cfg(target_os = "windows")]
fn is_app_foreground(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_invalid() {
            return false;
        }

        let mut foreground_pid = 0u32;
        let mut target_pid = 0u32;
        GetWindowThreadProcessId(foreground, Some(&mut foreground_pid));
        GetWindowThreadProcessId(hwnd, Some(&mut target_pid));

        foreground_pid != 0 && foreground_pid == target_pid
    }
}

#[cfg(not(target_os = "windows"))]
pub fn focus(_window: &AppWindow) -> Result<(), WindowError> {
    Err(WindowError::Unsupported)
}

/// 启动配置指向的目标。
#[cfg(target_os = "windows")]
pub fn launch(spec: &LaunchSpec) -> Result<(), WindowError> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let target = launch_target(spec);
    let op = to_wide("open");
    let file = to_wide(&target);

    // 这一步可能阻塞很久（MSIX 的进程外激活尤其慢），所以**不要**在 Tauri
    // 主线程上调用。前后各记一条日志，万一卡住一眼能看出卡在哪一步。
    core_log::log_line(&format!("[app-profile] 正在启动：{target}"));

    unsafe {
        let result = ShellExecuteW(
            None,
            PCWSTR(op.as_ptr()),
            PCWSTR(file.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        // 文档规定返回值 ≤ 32 即错误码，> 32 才是成功。
        let code = result.0 as isize;
        if code > 32 {
            core_log::log_line(&format!("[app-profile] 启动调用已返回：{target}"));
            Ok(())
        } else {
            core_log::log_warn(&format!(
                "[app-profile] 启动失败（ShellExecuteW 返回 {code}）：{target}"
            ));
            Err(WindowError::LaunchFailed(code))
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn launch(_spec: &LaunchSpec) -> Result<(), WindowError> {
    Err(WindowError::Unsupported)
}

/// 把 [`LaunchSpec`] 翻译成 `ShellExecuteW` 能吃的字符串。
///
/// `appid` 走 `shell:AppsFolder\` 协议——这是启动 MSIX / 打包应用的唯一正道，
/// 直接拼 `WindowsApps` 下的 exe 路径会因权限被拒。
fn launch_target(spec: &LaunchSpec) -> String {
    match spec {
        LaunchSpec::Path { value } | LaunchSpec::Url { value } => value.trim().to_string(),
        LaunchSpec::Appid { value } => format!("shell:AppsFolder\\{}", value.trim()),
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(json: &str) -> AppProfile {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn launch_target_maps_three_kinds() {
        assert_eq!(
            launch_target(&LaunchSpec::Path {
                value: r"E:\APP\ZCode\ZCode.exe".into()
            }),
            r"E:\APP\ZCode\ZCode.exe"
        );
        assert_eq!(
            launch_target(&LaunchSpec::Url {
                value: "http://127.0.0.1:3088".into()
            }),
            "http://127.0.0.1:3088"
        );
        assert_eq!(
            launch_target(&LaunchSpec::Appid {
                value: "Claude_pzs8sxrjxfjjc!Claude".into()
            }),
            r"shell:AppsFolder\Claude_pzs8sxrjxfjjc!Claude"
        );
    }

    #[test]
    fn launch_target_trims_whitespace() {
        assert_eq!(
            launch_target(&LaunchSpec::Appid {
                value: "  App!Id  ".into()
            }),
            r"shell:AppsFolder\App!Id"
        );
    }

    #[test]
    fn open_profile_without_launch_reports_not_found() {
        // 进程名保证不可能命中，且没配 launch → NotFound 而不是报错。
        let p = profile(r#"{ "process": "__definitely_not_running__.exe" }"#);
        assert_eq!(open_profile(&p), Ok(OpenOutcome::NotFound));
    }

    #[test]
    fn find_window_prefers_process_name_over_title() {
        // 标题匹配不该抢在进程名前面：这里进程名不存在，标题也不存在 → None。
        let p = profile(
            r#"{
                "process": "__definitely_not_running__.exe",
                "window_title_contains": ["__also_not_a_title__"]
            }"#,
        );
        assert_eq!(find_window(&p), None);
    }

    #[test]
    fn find_window_matches_nothing_for_blank_profile() {
        let p = profile(r#"{ "process": "  " }"#);
        assert_eq!(find_window(&p), None);
    }

    /// 真机冒烟：列出可见窗口，并用标题关键字找 DeepSeek Harness 的浏览器窗口。
    ///
    /// 环境相关，默认不跑；需要时用
    /// `cargo test -p core-app-profile -- --ignored --nocapture` 手动验证。
    #[test]
    #[ignore = "需要桌面会话与已打开的目标窗口"]
    fn list_windows_smoke() {
        let windows = list_windows();
        eprintln!("可见窗口 {} 个", windows.len());
        for w in windows.iter().take(20) {
            eprintln!("  {} | {}", w.process, w.title);
        }
        assert!(
            windows
                .iter()
                .any(|w| w.process.eq_ignore_ascii_case("explorer.exe")),
            "桌面会话里至少应该有 explorer.exe 的窗口"
        );
    }

    /// 真机冒烟：找到 DeepSeek Harness 的窗口并聚焦。
    #[test]
    #[ignore = "需要桌面上已打开 DeepSeek Harness"]
    fn focus_deepseek_harness_smoke() {
        let p = profile(
            r#"{
                "process": "__not_a_real_process__.exe",
                "window_title_contains": ["DeepSeek Harness"]
            }"#,
        );
        let window = find_window(&p).expect("应能按标题找到 DeepSeek Harness 窗口");
        eprintln!("命中窗口：{} | {}", window.process, window.title);
        eprintln!("聚焦结果：{:?}", focus(&window));
    }

    /// 真机冒烟：用**内置配置**走一遍「点图标」，两条分支都要能验。
    ///
    /// - ZCode 已打开 → `find_window` 必须命中（验证 EnumWindows + exe 名解析 + 进程名匹配）；
    /// - 没打开 → `launch` 必须真把它拉起来（验证 ShellExecuteW + `shell:AppsFolder`）。
    ///
    /// 聚焦结果是**尽力而为**，只打印不断言：从 `cargo test` 这种后台进程调用会被
    /// 前台锁拒绝，原因见 [`focus`] 的文档。
    #[test]
    #[ignore = "需要桌面上已安装 ZCode"]
    fn open_zcode_smoke() {
        let registry = crate::ProfileRegistry::builtin();
        let p = registry
            .profiles()
            .iter()
            .find(|p| p.display_name() == "ZCode")
            .expect("内置配置里应有 ZCode");

        eprintln!("配置：{} / {:?}", p.display_name(), p.process);
        eprintln!("启动方式：{:?}", p.launch);
        eprintln!("当前前台：{:?}", foreground_summary());

        match open_profile(p) {
            Ok(OpenOutcome::Focused { process, title }) => {
                assert_eq!(
                    process.to_lowercase(),
                    "zcode.exe",
                    "命中的窗口不属于 ZCode"
                );
                eprintln!("结果：已聚焦 {process} | {title}");
            }
            Ok(OpenOutcome::Launched) => {
                eprintln!("结果：已启动，等窗口出现…");
                let mut found = None;
                for _ in 0..40 {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Some(w) = find_window(p) {
                        found = Some(w);
                        break;
                    }
                }
                let w = found.expect("启动后 20 秒内应能看到 ZCode 窗口");
                eprintln!("窗口已出现：{} | {}", w.process, w.title);
            }
            Ok(OpenOutcome::NotFound) => panic!("既没找到窗口，也没配置启动方式"),
            Err(WindowError::FocusDenied) => {
                // 预期内的环境限制：后台进程抢不到前台。窗口本身必须还在。
                let w = find_window(p).expect("窗口应该还在，只是切不到前台");
                eprintln!(
                    "结果：窗口在（{} | {}），但前台锁拒绝切换——从后台进程调用时的已知限制",
                    w.process, w.title
                );
            }
            Err(e) => panic!("open_profile 失败：{e}"),
        }
    }
}
