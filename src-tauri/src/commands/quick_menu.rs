use tauri::Manager;

/// Show/hide the bottom-right quick menu window.
#[tauri::command]
pub fn toggle_quick_menu(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("quick-menu") {
        if win.is_visible().map_err(|e| e.to_string())? {
            win.hide().map_err(|e| e.to_string())?;
        } else {
            win.show().map_err(|e| e.to_string())?;
            win.set_focus().map_err(|e| e.to_string())?;
            // 每次显示时重新加载，确保拿到最新的 HTML 内容
            let _ = win.eval("window.location.reload()");
        }
    }
    Ok(())
}
