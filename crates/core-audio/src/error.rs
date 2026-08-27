//! core-audio 的错误类型。

/// 音频层错误。
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("audio output is not supported on this platform")]
    UnsupportedPlatform,

    #[error("no audio endpoint available")]
    NoEndpoint,

    #[error("endpoint not found: {0}")]
    EndpointNotFound(String),

    #[error("Windows audio error: {0}")]
    Windows(String),
}

/// 便捷的 Result 别名。
pub type Result<T> = std::result::Result<T, AudioError>;

#[cfg(windows)]
impl From<windows::core::Error> for AudioError {
    fn from(err: windows::core::Error) -> Self {
        AudioError::Windows(err.to_string())
    }
}
