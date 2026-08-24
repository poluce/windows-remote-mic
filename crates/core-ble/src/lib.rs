//! core-ble — WinRT Bluetooth Low Energy connection for the RC003.

pub mod error;
pub mod profile;

use serde::{Deserialize, Serialize};

pub use error::{BleError, Result};
pub use profile::{matches_rc003, RC003_BLUETOOTH_NAMES};

/// A Bluetooth device surfaced by Windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BleDevice {
    /// Windows device instance id (used later to connect/GATT).
    pub id: String,
    /// Friendly name reported by Windows.
    pub name: String,
}

/// Perform a blocking scan for paired Bluetooth devices and return the RC003
/// if found.
#[cfg(target_os = "windows")]
pub fn scan_for_rc003() -> Result<BleDevice> {
    let devices = self::winrt::scan_paired()?;
    devices
        .into_iter()
        .find(|d| matches_rc003(&d.name))
        .ok_or(BleError::DeviceNotFound)
}

/// Non-Windows placeholder so the crate still compiles and tests run.
#[cfg(not(target_os = "windows"))]
pub fn scan_for_rc003() -> Result<BleDevice> {
    Err(BleError::Windows(
        "BLE scanning is only implemented on Windows".to_string(),
    ))
}

#[cfg(target_os = "windows")]
mod winrt;
