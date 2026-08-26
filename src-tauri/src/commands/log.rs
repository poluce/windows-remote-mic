/// Frontend-facing log helper: append a line to the shared Remote Mic log.
#[tauri::command]
pub fn log_message(message: String) {
    core_input::log_line(&format!("[frontend] {message}"));
}