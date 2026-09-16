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
    name: String,
) -> Result<String, String> {
    let Some(profile) = state.dispatcher.profile_named(&name) else {
        return Err(format!("找不到名为「{name}」的应用配置"));
    };

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
