//! Error types for the BLE layer.

/// BLE layer error.
#[derive(Debug, thiserror::Error)]
pub enum BleError {
    #[error("failed to scan Bluetooth devices: {0}")]
    Scan(String),

    #[error("no matching remote device found")]
    DeviceNotFound,

    #[error("Windows error: {0}")]
    Windows(String),
}

/// Convenient result alias.
pub type Result<T> = std::result::Result<T, BleError>;
