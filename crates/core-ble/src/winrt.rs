//! Windows WinRT BLE 扫描（仅 Windows）。

use std::time::Duration;

use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Enumeration::DeviceInformation;

use crate::async_wait::wait_op;
use crate::{BleDevice, BleError, Result};

/// 枚举蓝牙设备的超时。设备多/蓝牙栈异常时不至于永久阻塞。
const SCAN_TIMEOUT: Duration = Duration::from_secs(15);

/// 枚举 Windows 当前已知的蓝牙低功耗设备。
pub fn scan_paired() -> Result<Vec<BleDevice>> {
    core_log::log_info("[ble/winrt] 正在通过 WinRT DeviceInformation 扫描蓝牙设备…");
    let selector = BluetoothLEDevice::GetDeviceSelector().map_err(|e| {
        core_log::log_error(&format!(
            "[ble/winrt] 获取设备选择器（GetDeviceSelector）失败: {e}"
        ));
        BleError::Windows(e.to_string())
    })?;

    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector).map_err(|e| {
        core_log::log_error(&format!(
            "[ble/winrt] 查找蓝牙设备（FindAllAsyncAqsFilter）失败: {e}"
        ));
        BleError::Windows(e.to_string())
    })?;

    let collection = wait_op(&operation, SCAN_TIMEOUT, "枚举蓝牙设备").map_err(|e| {
        core_log::log_error(&format!("[ble/winrt] 等待蓝牙设备查询失败: {e}"));
        e
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
            core_log::log_debug(&format!("[ble/winrt] 发现蓝牙设备: '{name}' ({id})"));
            out.push(BleDevice { id, name });
        }
    }

    core_log::log_info(&format!(
        "[ble/winrt] 扫描完成，发现 {} 个有名称的蓝牙设备",
        out.len()
    ));
    Ok(out)
}
