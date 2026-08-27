use serde::Serialize;

/// Frontend-facing log helper: append a line to the shared Remote Mic log.
#[tauri::command]
pub fn log_message(message: String) {
    core_log::log_line(&format!("[前端] {message}"));
}

#[derive(Serialize)]
pub struct LogInfo {
    pub path: String,
    pub file_size: u64,
    pub debug_enabled: bool,
    pub files: Vec<core_log::LogFileInfo>,
}

/// Return the active log path, current size, DEBUG state and rotated files.
#[tauri::command]
pub fn get_log_info() -> LogInfo {
    let path = core_log::log_path();
    let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    LogInfo {
        path: path.display().to_string(),
        file_size,
        debug_enabled: core_log::debug_enabled(),
        files: core_log::log_files(),
    }
}

/// Read the tail of the active log. `max_bytes` is clamped to [1024, 1 MiB].
#[tauri::command]
pub fn read_log_tail(max_bytes: Option<usize>) -> String {
    let max = max_bytes.unwrap_or(64 * 1024).clamp(1024, 1024 * 1024);
    core_log::read_log_tail(max)
}

/// Clear the active log file.
#[tauri::command]
pub fn clear_log() -> Result<(), String> {
    core_log::clear_log().map_err(|e| e.to_string())
}

/// Open the log directory in Windows Explorer / system file manager.
#[tauri::command]
pub fn open_log_dir() -> Result<(), String> {
    let dir = core_log::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(dir.as_os_str())
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = dir;
        Err("打开日志目录仅支持 Windows".to_string())
    }
}

/// Enable/disable verbose DEBUG logging at runtime.
#[tauri::command]
pub fn set_debug_logging(enabled: bool) -> bool {
    core_log::set_debug_enabled(enabled);
    core_log::debug_enabled()
}
