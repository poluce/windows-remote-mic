//! core-input — Windows 键盘注入（SendInput）。

pub mod error;
pub mod hook;
pub use error::{InputError, Result};
pub use hook::{start_key_hook, RawKeyEvent};

/// 向 `%LOCALAPPDATA%\RemoteMic\RC003\remote-mic.log` 追加一行日志。
///
/// 这是对 `core_log` 的薄封装，让现有调用方可以继续使用
/// `core_input::log_line`。
pub fn log_line(line: &str) {
    core_log::log_line(line);
}

/// 追加一行 DEBUG 级别日志。仅在临时调试日志开启时写入。
pub fn log_debug(line: &str) {
    core_log::log_debug(line);
}

/// 向共享的 Remote Mic 日志文件追加一行 ERROR 级别日志。
pub fn log_error(line: &str) {
    core_log::log_error(line);
}

/// 向共享的 Remote Mic 日志文件追加一行 WARN 级别日志。
pub fn log_warn(line: &str) {
    core_log::log_warn(line);
}

/// 按下 Win + H 启动 Windows 自带语音输入。
#[cfg(target_os = "windows")]
pub fn press_win_h() -> Result<()> {
    crate::hotkey::press_win_h()
}

/// 非 Windows 平台桩实现，使 crate 在所有平台都能编译。
#[cfg(not(target_os = "windows"))]
pub fn press_win_h() -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

/// 打开 Windows 语音输入（Win+H）。每次都直接按 Win+H：
/// 弹窗关闭时打开；弹窗已打开时只会重置当前输入会话（实测不会关闭弹窗），
/// 可接受。关闭弹窗用 Esc / ✕。
#[cfg(target_os = "windows")]
pub fn open_voice_typing() -> Result<()> {
    crate::log_line("[input] 语音输入 -> 开启 (Win+H)");
    crate::hotkey::press_win_h()
}

/// 非 Windows 平台桩实现。
#[cfg(not(target_os = "windows"))]
pub fn open_voice_typing() -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

/// 按下 Escape 关闭 Windows 语音输入。
#[cfg(target_os = "windows")]
pub fn press_escape() -> Result<()> {
    crate::hotkey::press_escape()
}

/// 非 Windows 平台桩实现。
#[cfg(not(target_os = "windows"))]
pub fn press_escape() -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

#[cfg(target_os = "windows")]
mod hotkey;

/// 根据按键标记发送快捷键，例如 ["win","d"] 或 ["ctrl","space"]。
#[cfg(target_os = "windows")]
pub fn send_key_combo(tokens: &[&str]) -> Result<()> {
    crate::hotkey::send_key_combo(tokens)
}

/// 通过 shell 打开应用/文件（仅 Windows）。
#[cfg(target_os = "windows")]
pub fn open_app(name: &str) -> Result<()> {
    crate::hotkey::open_app(name)
}

#[cfg(not(target_os = "windows"))]
pub fn send_key_combo(_tokens: &[&str]) -> Result<()> {
    Err(InputError::Windows(
        "key injection only on Windows".to_string(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn open_app(_name: &str) -> Result<()> {
    Err(InputError::Windows("open_app only on Windows".to_string()))
}
