//! 应用专属按键配置（一个应用一个文件）的状态查询、重载与「点图标」动作。

use tauri::State;

use crate::{load_app_profiles, AppState};

/// 当前前台应用与命中的应用配置。
///
/// 诊断页用它核对「进程名是否写对」——这是新增一份应用配置的第一步。
#[tauri::command]
pub fn app_profile_status(state: State<AppState>) -> core_dispatch::ForegroundStatus {
    state.dispatcher.foreground_status()
}

/// 快捷菜单内圈要显示的应用图标（只含配了 `icon` 的配置）。
#[tauri::command]
pub fn app_menu_apps(state: State<AppState>) -> Vec<core_dispatch::MenuAppEntry> {
    state.dispatcher.menu_apps()
}

/// 「点图标」：已打开就切到前台，没打开就按配置启动。
///
/// **不需要手动切换按键配置**：映射是按前台应用实时解析的，目标应用一旦到了
/// 前台，下一按键就自动走它的专属映射。
///
/// 顺序上先聚焦/启动、再收起快捷菜单——快捷菜单窗口是本应用的前台窗口，
/// 收起它之前本进程才持有前台权限，`SetForegroundWindow` 才会被放行。
#[tauri::command]
pub fn open_app_profile(
    app: tauri::AppHandle,
    state: State<AppState>,
    name: String,
) -> Result<String, String> {
    let Some(profile) = state.dispatcher.profile_named(&name) else {
        return Err(format!("找不到名为「{name}」的应用配置"));
    };

    let result = match core_app_profile::open_profile(&profile) {
        Ok(core_app_profile::OpenOutcome::Focused { process, title }) => {
            core_log::log_info(&format!(
                "[app-profile] 已聚焦 {name}（{process} | {title}）"
            ));
            Ok(format!("已切到 {name}"))
        }
        Ok(core_app_profile::OpenOutcome::Launched) => {
            core_log::log_info(&format!("[app-profile] 已启动 {name}"));
            Ok(format!("正在启动 {name}"))
        }
        Ok(core_app_profile::OpenOutcome::NotFound) => {
            core_log::log_warn(&format!("[app-profile] {name} 既没运行也没配启动方式"));
            Err(format!("没找到 {name} 的窗口，这份配置也没写启动方式"))
        }
        Err(e) => {
            core_log::log_warn(&format!("[app-profile] {name} 聚焦/启动失败：{e}"));
            Err(format!("{name}：{e}"))
        }
    };

    // 无论成败都收起菜单：成功了要让位给目标应用，失败了也得让用户能操作。
    if let Err(e) = crate::commands::quick_menu::hide_quick_menu(&app, &state) {
        core_log::log_warn(&format!("[app-profile] 收起快捷菜单失败：{e}"));
    }

    result
}

/// 重新加载应用专属配置（内置 + 用户目录），返回加载后的配置总数。
///
/// 用户往 `<配置目录>/app-profiles/` 放了新文件后，不必重启应用。
#[tauri::command]
pub fn reload_app_profiles(state: State<AppState>) -> usize {
    let registry = load_app_profiles();
    let count = registry.profiles().len();
    state.dispatcher.set_profiles(registry);
    core_log::log_info(&format!("[commands/app_profile] 已重载 {count} 份应用配置"));
    count
}
