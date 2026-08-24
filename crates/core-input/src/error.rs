//! Error types for the input injection layer.

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("SendInput failed (win32 hr): {0}")]
    Send(#[from] windows::core::Error),

    #[error("Windows error: {0}")]
    Windows(String),
}

pub type Result<T> = std::result::Result<T, InputError>;
