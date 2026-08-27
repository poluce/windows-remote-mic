//! BLE 层的错误类型。

/// BLE 层错误。
#[derive(Debug, thiserror::Error)]
pub enum BleError {
    #[error("failed to scan Bluetooth devices: {0}")]
    Scan(String),

    #[error("no matching remote device found")]
    DeviceNotFound,

    #[error("Windows error: {0}")]
    Windows(String),
}

/// 便捷的 Result 别名。
pub type Result<T> = std::result::Result<T, BleError>;
