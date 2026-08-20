//! Error types for the ATVV voice layer.

/// ATVV layer error.
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

/// Convenient result alias.
pub type Result<T> = std::result::Result<T, AtvvError>;
