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
            win.hide().map_err(|e| e.to_string())?;
            state
                .dispatcher
                .set_input_mode(core_dispatch::InputMode::Normal);
            core_log::log_info("[quick-menu] 已关闭，恢复普通按键映射");
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
