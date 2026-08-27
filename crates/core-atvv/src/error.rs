//! ATVV 语音层的错误类型。

/// ATVV 层错误。
#[derive(Debug, thiserror::Error)]
pub enum AtvvError {
    #[error("invalid frame length: got {got}, expected {expected}")]
    InvalidFrameLength { got: usize, expected: usize },

    #[error("invalid control command in state {state}: {detail}")]
    InvalidControl { state: String, detail: String },

    #[error("voice session is not active")]
    NotActive,

    #[error("codec unsupported: {0}")]
    UnsupportedCodec(String),

    #[error("ADPCM index out of range: {0}")]
    IndexOutOfRange(i32),
}

/// 便捷的 Result 别名。
pub type Result<T> = std::result::Result<T, AtvvError>;
