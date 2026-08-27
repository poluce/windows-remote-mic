//! core-input — Windows keyboard injection (SendInput).

pub mod error;
pub mod hook;
pub use error::{InputError, Result};
pub use hook::{start_key_hook, RawKeyEvent};

/// Append a line to `%LOCALAPPDATA%\RemoteMic\RC003\remote-mic.log`.
///
/// This is a thin wrapper around `core_log` so existing callers can keep using
/// `core_input::log_line`.
pub fn log_line(line: &str) {
    core_log::log_line(line);
}

/// Append a DEBUG-level line. Only written when temporary debug logging is on.
pub fn log_debug(line: &str) {
    core_log::log_debug(line);
}

/// Append an ERROR-level line to the shared Remote Mic log file.
pub fn log_error(line: &str) {
    core_log::log_error(line);
}

/// Append a WARN-level line to the shared Remote Mic log file.
pub fn log_warn(line: &str) {
    core_log::log_warn(line);
}

/// Press Win + H to start Windows built-in voice typing.
#[cfg(target_os = "windows")]
pub fn press_win_h() -> Result<()> {
    crate::hotkey::press_win_h()
}

/// Non-Windows stub so the crate compiles everywhere.
#[cfg(not(target_os = "windows"))]
pub fn press_win_h() -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

/// Press Escape to close Windows voice typing.
#[cfg(target_os = "windows")]
pub fn press_escape() -> Result<()> {
    crate::hotkey::press_escape()
}

/// Non-Windows stub.
#[cfg(not(target_os = "windows"))]
pub fn press_escape() -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

#[cfg(target_os = "windows")]
mod hotkey;

/// Send a keyboard shortcut from tokens, e.g. ["win","d"] or ["ctrl","space"].
#[cfg(target_os = "windows")]
pub fn send_key_combo(tokens: &[&str]) -> Result<()> {
    crate::hotkey::send_key_combo(tokens)
}

/// Open an app / file by launching it through the shell (Windows only).
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
