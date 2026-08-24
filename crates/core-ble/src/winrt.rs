//! Windows WinRT BLE scanning (Windows only).

use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Enumeration::DeviceInformation;

use crate::{BleDevice, BleError, Result};

/// Enumerate Bluetooth LE devices currently known to Windows.
pub fn scan_paired() -> Result<Vec<BleDevice>> {
    let selector = BluetoothLEDevice::GetDeviceSelector()
        .map_err(|e| BleError::Windows(e.to_string()))?;

    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector)
        .map_err(|e| BleError::Windows(e.to_string()))?;

    let collection = pollster::block_on(async { operation.await })
        .map_err(|e| BleError::Windows(e.to_string()))?;

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
            out.push(BleDevice { id, name });
        }
    }

    Ok(out)
}
