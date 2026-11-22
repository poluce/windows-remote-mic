//! 快捷菜单与应用窗口操作（前台状态、菜单图标收集、聚焦或启动目标应用）。

use tauri::{Manager, State};

use crate::{load_app_profiles, AppState};

/// 快捷菜单内环里「本软件」那一项的固定 id。
///
/// 主窗口不是应用配置（`app-profiles/*.json` 里没有它），但用户在菜单里需要一条
/// **回到全局设置**的路径：内环原本只能切到自建应用，进了某个应用就回不来了。
/// 点它 = 把主窗口切到前台；前台是本软件时没有应用配置能命中，按键自然走全局映射，
/// 于是「全局设置」和「各应用设置」在同一个菜单里就能来回切。
///
/// 用双下划线包起来是刻意的：`new_profile_id` 把进程名 slug 成 `[a-z0-9-]`，
/// 永远产不出下划线，所以真实配置 id 不可能撞上它。
pub const SELF_MENU_ID: &str = "__self__";

/// 快捷菜单内环里「本软件 / 全局设置」那一项的外观。
///
/// 抽成纯函数是为了能单测：`app_menu_apps` 需要 `AppHandle` 才能查窗口可见性，
/// 在单测里造不出来，但这一项的内容不该跟着窗口一起变成不可测。
fn self_menu_entry(open: bool) -> core_dispatch::MenuAppEntry {
    core_dispatch::MenuAppEntry {
        id: SELF_MENU_ID.to_string(),
        name: "全局设置".to_string(),
        label: "设".to_string(),
        // 与主界面强调色一致（钢蓝）；内环绘制时还会再向中性灰混 22%。
        color: "#2b6fc0".to_string(),
        open,
    }
}

/// 当前前台应用与命中的应用配置。
///
/// 诊断页用它核对「进程名是否写对」——这是新增一份应用配置的第一步。
#[tauri::command]
pub fn app_profile_status(state: State<AppState>) -> core_dispatch::ForegroundStatus {
    state.dispatcher.foreground_status()
}

/// 「新建应用配置」对话框的进程名候选：当前前台（不是本应用自己时）+ 最近前台历史。
///
/// # 为什么不能直接用 `app_profile_status`
///
/// 打开本页时前台窗口**必然是本应用自己**（用户是点这里进来的），
/// `app_profile_status.process` 于是返回 `remote-mic.exe`——拿它填进程名，
/// 建出来的配置只会匹配本应用自己，永远不会命中目标应用。
/// 用户真正要填的是「刚才还在用、现在被本应用盖住的那个应用」，
/// 所以这里给的是最近前台历史（后台 tick 线程每次刷新前台快照时记录）。
#[tauri::command]
pub fn app_profile_foreground_candidates() -> Vec<core_app_profile::ForegroundApp> {
    core_app_profile::foreground_candidates()
}

/// 简标颜色调色板（`#rrggbb` 列表）。
///
/// 前端不硬编码第二份色卡：内环自动色、颜色选择器、作用范围圆点都从这一处取，
/// 保证「用户挑的颜色」和「内环画出来的颜色」是同一组。
#[tauri::command]
pub fn app_icon_palette() -> Vec<&'static str> {
    core_app_profile::icon_palette().to_vec()
}

/// 快捷菜单内圈要显示的应用图标：**第一项固定是本软件（全局设置）**，
/// 后面是 `app-profiles/` 里的全部配置（没有 `icon` 的用自动简标兜底）。
///
/// 声明成 `async` 是纵深防御：它会遍历本机所有顶层窗口，虽然实测每次只要
/// 约 2 毫秒，但没有理由让 UI 线程替它站岗——真撞上某个窗口出问题时，至少
/// 界面还能动，日志里的「开始收集 / 完成」也能指出卡在哪。
///
/// 返回 `Result` 只是 Tauri 对「带引用的 async 命令」的硬性要求，这里不会失败；
/// 成功时前端拿到的仍是数组本身。
#[tauri::command]
pub async fn app_menu_apps(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<core_dispatch::MenuAppEntry>, String> {
    core_log::log_line("[app-profile] 开始收集菜单应用");
    let mut apps = state.dispatcher.menu_apps();
    // 主窗口可见时给自己点一个小白点（和自建应用「已打开」的含义一致）。
    let main_open = app
        .get_webview_window("main")
        .and_then(|win| win.is_visible().ok())
        .unwrap_or(false);
    apps.insert(0, self_menu_entry(main_open));
    core_log::log_line(&format!(
        "[app-profile] 菜单应用收集完成：{} 个（含全局设置）",
        apps.len()
    ));
    Ok(apps)
}

/// 「点图标」：已打开就切到前台，没打开就按配置启动。
///
/// **不需要手动切换按键配置**：映射是按前台应用实时解析的，目标应用一旦到了
/// 前台，下一按键就自动走它的专属映射。
///
/// # 为什么是 async，且把系统调用丢进阻塞线程池
///
/// Tauri 的**同步**命令跑在主线程上。`ShellExecuteW` 走 MSIX 的进程外激活时
/// 需要调用方泵消息，主线程被命令占住就会死锁——实测点 Claude 时整个应用被
/// Windows 判定为「无响应」（AppHangB1）后强杀。`SetForegroundWindow` 同样可能
/// 卡在目标线程上。所以这里：命令声明成 `async`（不再占主线程），两个可能长时间
/// 阻塞的系统调用再各丢进 `spawn_blocking`。
///
/// # 顺序
///
/// - **聚焦**必须趁快捷菜单还是前台窗口时做：`SetForegroundWindow` 只对已拥有
///   前台权限的进程放行，菜单一收起权限就没了，所以先聚焦、后收菜单。
/// - **启动**不需要前台权限（新进程本来就会拿到前台），而且可能要等很久，所以
///   先收菜单让界面立刻有反馈，再去启动。
#[tauri::command]
pub async fn open_app_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    core_log::log_line(&format!("[app-profile] 收到打开请求：{id}"));

    // 第一项是「本软件 / 全局设置」，不是应用配置——直接切回主窗口。
    if id == SELF_MENU_ID {
        return open_self_window(&app, &state);
    }

    // 定位用 id（文件名），**不用展示名**——展示名在编辑器里可以改，
    // 拿它当身份的话，用户改个名字快捷菜单就点空了。
    let Some(profile) = state.dispatcher.profile_by_id(&id) else {
        return Err(format!("找不到 id 为「{id}」的应用配置"));
    };
    // 下面所有给人看的提示都用展示名。
    let name = profile.display_name();

    // 找窗口很快（一次 EnumWindows），留在当前线程即可。
    if let Some(window) = core_app_profile::find_window(&profile) {
        let target = window.clone();
        let focused = match run_blocking(move || core_app_profile::focus(&target)).await {
            Ok(inner) => inner,
            Err(e) => {
                hide_quick_menu(&app, &state);
                return Err(format!("{name}：{e}"));
            }
        };

        // 先收菜单：无论成败都要让位——成功了要把前台交给目标应用，失败了
        // 也得让用户能继续操作。
        hide_quick_menu(&app, &state);

        return match focused {
            Ok(()) => {
                core_log::log_info(&format!(
                    "[app-profile] 已聚焦 {name}（{} | {}）",
                    window.process, window.title
                ));
                Ok(format!("已切到 {name}"))
            }
            Err(e) => {
                core_log::log_warn(&format!("[app-profile] {name} 聚焦失败：{e}"));
                Err(format!("{name}：{e}"))
            }
        };
    }

    let Some(spec) = profile.launch.clone() else {
        hide_quick_menu(&app, &state);
        core_log::log_warn(&format!("[app-profile] {name} 既没运行也没配启动方式"));
        return Err(format!("没找到 {name} 的窗口，这份配置也没写启动方式"));
    };

    hide_quick_menu(&app, &state);

    match run_blocking(move || core_app_profile::launch(&spec)).await {
        Ok(Ok(())) => {
            core_log::log_info(&format!("[app-profile] 已启动 {name}"));
            Ok(format!("正在启动 {name}"))
        }
        Ok(Err(e)) => {
            core_log::log_warn(&format!("[app-profile] {name} 启动失败：{e}"));
            Err(format!("{name}：{e}"))
        }
        Err(e) => Err(format!("{name}：{e}")),
    }
}

/// 把可能长时间阻塞的系统调用丢进阻塞线程池，别占着 Tauri 的线程。
async fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("后台任务失败：{e}"))
}

/// 收起快捷菜单；失败只记日志，不改变「点图标」本身的结果。
fn hide_quick_menu(app: &tauri::AppHandle, state: &State<'_, AppState>) {
    if let Err(e) = crate::commands::quick_menu::hide_quick_menu(app, state) {
        core_log::log_warn(&format!("[app-profile] 收起快捷菜单失败：{e}"));
    }
}

/// 「点全局设置」：把主窗口显示出来并切到前台，然后收起快捷菜单。
///
/// 前台变成主窗口后**没有任何应用配置能命中**，按键自动回到全局映射——
/// 这就是「从某个应用切回全局设置」的实现方式，不需要额外的模式开关。
///
/// 顺序同 `open_app_profile` 的聚焦分支：先聚焦再收菜单。主窗口和快捷菜单
/// 属于同一个进程，前台权限还在自己手里，`set_focus` 不会卡。
fn open_self_window(app: &tauri::AppHandle, state: &State<'_, AppState>) -> Result<String, String> {
    let Some(window) = app.get_webview_window("main") else {
        hide_quick_menu(app, state);
        return Err("找不到主窗口".to_string());
    };
    let shown = window
        .show()
        .and_then(|_| window.unminimize())
        .and_then(|_| window.set_focus())
        .map_err(|e| e.to_string());
    hide_quick_menu(app, state);
    match shown {
        Ok(()) => {
            core_log::log_info("[app-profile] 已切回本软件主窗口（全局设置）");
            Ok("已回到全局设置".to_string())
        }
        Err(e) => {
            core_log::log_warn(&format!("[app-profile] 切回主窗口失败：{e}"));
            Err(format!("切回主窗口失败：{e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 内环第一项是「本软件 / 全局设置」，不是应用配置。
    #[test]
    fn self_menu_entry_is_a_pinned_global_settings_item() {
        let entry = self_menu_entry(true);
        assert_eq!(entry.id, SELF_MENU_ID);
        assert_eq!(entry.name, "全局设置");
        assert_eq!(entry.label, "设");
        assert_eq!(
            entry.label.chars().count(),
            1,
            "简标要短，内环圆圈只放得下 1–2 个字"
        );
        assert!(
            entry.color.starts_with('#') && entry.color.len() == 7,
            "颜色必须是 #rrggbb：{}",
            entry.color
        );

        // 「已打开」只影响前端小白点，不该改变条目身份。
        assert!(entry.open);
        assert!(!self_menu_entry(false).open);
    }

    /// 固定 id 必须撞不上任何真实配置：`new_profile_id` 只能产出 `[a-z0-9-]`。
    #[test]
    fn self_menu_id_cannot_collide_with_profile_ids() {
        assert!(!SELF_MENU_ID.is_empty());
        assert!(
            !SELF_MENU_ID
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "固定 id 含有 slug 不会产出的字符，才能保证不撞车"
        );
    }
}

/// 重新加载应用专属配置（先播种，再读 `app-profiles/`），返回加载后的配置总数。
///
/// 用户直接改了 `<配置目录>/app-profiles/` 里的文件后，不必重启应用。
#[tauri::command]
pub fn reload_app_profiles(state: State<AppState>) -> usize {
    let registry = load_app_profiles();
    let count = registry.profiles().len();
    state.dispatcher.set_profiles(registry);
    core_log::log_info(&format!("[commands/app_profile] 已重载 {count} 份应用配置"));
    count
}
