//! Windows WinRT BLE scanning (Windows only).

use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Enumeration::DeviceInformation;

use crate::{BleDevice, BleError, Result};

/// Enumerate Bluetooth LE devices currently known to Windows.
pub fn scan_paired() -> Result<Vec<BleDevice>> {
    core_log::log_info("[ble/winrt] starting BLE device scan via WinRT DeviceInformation...");
    let selector = BluetoothLEDevice::GetDeviceSelector()
        .map_err(|e| {
            core_log::log_error(&format!("[ble/winrt] GetDeviceSelector failed: {e}"));
            BleError::Windows(e.to_string())
        })?;

    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|e| {
            core_log::log_error(&format!("[ble/winrt] FindAllAsyncAqsFilter failed: {e}"));
            BleError::Windows(e.to_string())
        })?;

    let collection = pollster::block_on(async { operation.await })
        .map_err(|e| {
            core_log::log_error(&format!("[ble/winrt] FindAllAsync await failed: {e}"));
            BleError::Windows(e.to_string())
        })?;

    let mut out = Vec::new();
    for device in &collection {
        let id = device
            .Id()
            .map_err(|e| BleError::Windows(e.to_string()))?
            .to_string();
        let name = device
            .Name()
            .map_err(|e| BleError::Windows(e.to_string()))?
            .to_string();

        if !name.trim().is_empty() {
            core_log::log_debug(&format!("[ble/winrt] discovered BLE device: '{name}' ({id})"));
            out.push(BleDevice { id, name });
        }
    }

    core_log::log_info(&format!("[ble/winrt] scan finished, found {} named BLE devices", out.len()));
    Ok(out)
}
