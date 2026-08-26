//! core-ble — WinRT Bluetooth Low Energy connection for the RC003.

pub mod capture;
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
    core_log::log_info("[ble] starting scan_for_rc003...");
    let devices = self::winrt::scan_paired()?;
    for d in &devices {
        if matches_rc003(&d.name) {
            core_log::log_info(&format!("[ble] matched RC003 device: name='{}', id='{}'", d.name, d.id));
            return Ok(d.clone());
        }
    }
    core_log::log_warn(&format!(
        "[ble] RC003 remote not found among {} paired devices. Checked names: {:?}",
        devices.len(),
        RC003_BLUETOOTH_NAMES
    ));
    Err(BleError::DeviceNotFound)
}

/// Non-Windows placeholder so the crate still compiles and tests run.
#[cfg(not(target_os = "windows"))]
pub fn scan_for_rc003() -> Result<BleDevice> {
    Err(BleError::Windows(
        "BLE scanning is only implemented on Windows".to_string(),
    ))
}

#[cfg(target_os = "windows")]
pub mod gatt;

#[cfg(target_os = "windows")]
pub use gatt::{discover_atvv, AtvvEndpoints};
#[cfg(target_os = "windows")]
mod winrt;

/// Scan for the RC003 then discover its ATVV endpoints (Windows only).
#[cfg(target_os = "windows")]
pub fn scan_and_connect() -> Result<(BleDevice, AtvvEndpoints)> {
    core_log::log_info("[ble] scan_and_connect initiated...");
    let device = scan_for_rc003()?;
    core_log::log_info(&format!("[ble] device found: '{}', proceeding to discover ATVV endpoints...", device.name));
    let endpoints = discover_atvv(&device.id)?;
    core_log::log_info(&format!("[ble] ATVV endpoints discovered successfully: complete={}", endpoints.is_complete()));
    Ok((device, endpoints))
}
