//! 应用专属按键配置（一个应用一个文件）的状态查询与重载。

use tauri::State;

use crate::{load_app_profiles, AppState};

/// 当前前台应用与命中的应用配置。
///
/// 诊断页用它核对「进程名是否写对」——这是新增一份应用配置的第一步。
#[tauri::command]
pub fn app_profile_status(state: State<AppState>) -> core_dispatch::ForegroundStatus {
    state.dispatcher.foreground_status()
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
