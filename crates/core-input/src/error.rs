//! 输入注入层的错误类型。

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("SendInput failed (win32 hr): {0}")]
    Send(#[from] windows::core::Error),

    #[error("Windows error: {0}")]
    Windows(String),
}

pub type Result<T> = std::result::Result<T, InputError>;
