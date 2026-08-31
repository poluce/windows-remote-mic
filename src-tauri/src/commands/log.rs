use serde::Serialize;

/// 面向前端的日志辅助函数：向共享的 Remote Mic 日志追加一行。
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

/// 返回当前活动日志路径、大小、DEBUG 状态以及轮转文件。
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

/// 读取活动日志的尾部。`max_bytes` 会被限制在 [1024, 1 MiB] 范围内。
#[tauri::command]
pub fn read_log_tail(max_bytes: Option<usize>) -> String {
    let max = max_bytes.unwrap_or(64 * 1024).clamp(1024, 1024 * 1024);
    core_log::read_log_tail(max)
}

/// 清空活动日志文件。
#[tauri::command]
pub fn clear_log() -> Result<(), String> {
    core_log::clear_log().map_err(|e| e.to_string())
}

/// 在 Windows 资源管理器 / 系统文件管理器中打开日志目录。
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

/// 在运行时启用/禁用详细 DEBUG 日志。
#[tauri::command]
pub fn set_debug_logging(enabled: bool) -> bool {
    core_log::set_debug_enabled(enabled);
    let state = core_log::debug_enabled();
    if state {
        core_log::log_info("[core-log] 运行时已开启 DEBUG 详细日志");
    } else {
        core_log::log_info("[core-log] 运行时已关闭 DEBUG 详细日志");
    }
    state
}
