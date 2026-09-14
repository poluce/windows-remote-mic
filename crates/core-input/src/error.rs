//! 输入注入层的错误类型。

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("Windows error: {0}")]
    Windows(String),
}

impl InputError {
    /// 不含错误类型前缀的原始消息，便于拼接面向用户的文案。
    pub fn message(&self) -> &str {
        match self {
            InputError::Windows(m) => m,
        }
    }
}

pub type Result<T> = std::result::Result<T, InputError>;
