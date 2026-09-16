use tauri::{Emitter, Manager, State};

use core_dispatch::AppEvent;
use core_mapping::ButtonId;

use crate::AppState;

/// 快捷菜单按键事件载荷（菜单独占模式下由调度器转发）。
#[derive(Clone, serde::Serialize)]
pub struct QuickMenuKeyEvent {
    pub key: &'static str,
    pub pressed: bool,
}

/// 显示/隐藏右下角的快捷菜单窗口。
///
/// 打开时进入「菜单独占模式」：遥控器按键全部直接路由给快捷菜单
/// （无需窗口焦点，点击其它窗口也不影响）；关闭时恢复普通按键映射。
#[tauri::command]
pub fn toggle_quick_menu(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("quick-menu") {
        if win.is_visible().map_err(|e| e.to_string())? {
            hide_quick_menu(&app, &state)?;
        } else {
            win.show().map_err(|e| e.to_string())?;
            win.set_focus().map_err(|e| e.to_string())?;
            // 每次显示时重新加载，确保拿到最新的 HTML 内容
            let _ = win.eval("window.location.reload()");
            state
                .dispatcher
                .set_input_mode(core_dispatch::InputMode::QuickMenu);
            core_log::log_info("[quick-menu] 已打开，进入菜单独占模式");
        }
    }
    Ok(())
}

/// 收起快捷菜单并恢复普通按键映射；窗口本来就不可见时是空操作。
///
/// 独立成一个函数是因为「打开某个应用」也要用它——那条路径不能走
/// `toggle_quick_menu`，否则菜单没开时反而会把它打开。
pub fn hide_quick_menu(app: &tauri::AppHandle, state: &State<AppState>) -> Result<(), String> {
    let Some(win) = app.get_webview_window("quick-menu") else {
        return Ok(());
    };
    if !win.is_visible().map_err(|e| e.to_string())? {
        return Ok(());
    }
    win.hide().map_err(|e| e.to_string())?;
    state
        .dispatcher
        .set_input_mode(core_dispatch::InputMode::Normal);
    core_log::log_info("[quick-menu] 已关闭，恢复普通按键映射");
    Ok(())
}

/// 只收起、不切换——供菜单页面在「点到空白处」时自己关闭。
///
/// 页面不能用 `toggle_quick_menu`：那是切换语义，一旦页面与后端对当前可见性
/// 的判断不一致，就会变成「关闭→立刻又打开」。
///
/// # 为什么必须是 async
///
/// 调用方**就是被隐藏的那个窗口**。同步命令跑在主线程上，主线程一边执行
/// `hide()`、一边又要把 IPC 回执交给同一个 webview，重入后双方互等——实测
/// 主线程就此卡死，应用被 Windows 判定 AppHang 关掉（16:59:41 那次）。
/// 声明成 `async` 后 `hide()` 在异步线程上执行，主线程空出来投递回执。
#[tauri::command]
pub async fn close_quick_menu(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    core_log::log_line("[quick-menu] 收到页面关闭请求");
    hide_quick_menu(&app, &state)
}

/// 调度器应用事件出口：处理所有需要 Tauri 层执行的事件。
///
/// - [`AppEvent::ToggleQuickMenu`]：映射动作，开关快捷菜单；
/// - [`AppEvent::MenuKey`]：菜单独占模式下遥控器按键直转，映射为
///   `quick-menu-key` 事件发给菜单窗口；菜单/返回键直接在此关闭
///   （关闭只由本处执行，页面收到 close 仅做清理，避免双重开关）。
pub fn handle_app_event(app: tauri::AppHandle, event: AppEvent) {
    match event {
        AppEvent::ToggleQuickMenu => {
            if let Err(e) = toggle_quick_menu(app.clone(), app.state::<AppState>()) {
                core_log::log_warn(&format!("[dispatch] 快捷菜单切换失败: {e}"));
            }
        }
        AppEvent::MenuKey(button, pressed) => {
            let key = match button {
                ButtonId::Up => Some("up"),
                ButtonId::Down => Some("down"),
                ButtonId::Left => Some("left"),
                ButtonId::Right => Some("right"),
                ButtonId::Ok => Some("ok"),
                ButtonId::Menu | ButtonId::Back => Some("close"),
                _ => None,
            };
            let Some(key) = key else { return };
            let _ = app.emit("quick-menu-key", QuickMenuKeyEvent { key, pressed });
            if key == "close" && pressed {
                if let Err(e) = toggle_quick_menu(app.clone(), app.state::<AppState>()) {
                    core_log::log_warn(&format!("[quick-menu] 关闭失败: {e}"));
                }
            }
        }
    }
}
