//! core-input — Windows keyboard injection (SendInput).

pub mod error;
pub use error::{InputError, Result};

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
    Err(InputError::Windows("key injection only on Windows".to_string()))
}

#[cfg(not(target_os = "windows"))]
pub fn open_app(_name: &str) -> Result<()> {
    Err(InputError::Windows("open_app only on Windows".to_string()))
}
