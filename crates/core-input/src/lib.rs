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
