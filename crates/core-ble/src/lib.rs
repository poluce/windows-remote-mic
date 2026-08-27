//! core-ble — RC003 的 WinRT 低功耗蓝牙（BLE）连接。

pub mod capture;
pub mod error;
pub mod profile;

use serde::{Deserialize, Serialize};

pub use error::{BleError, Result};
pub use profile::{matches_rc003, RC003_BLUETOOTH_NAMES};

/// Windows 暴露的一个蓝牙设备。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BleDevice {
    /// Windows 设备实例 ID（稍后用于连接/GATT）。
    pub id: String,
    /// Windows 报告的设备友好名称。
    pub name: String,
}

/// 执行阻塞式扫描，查找已配对的蓝牙设备；如果找到则返回 RC003。
#[cfg(target_os = "windows")]
pub fn scan_for_rc003() -> Result<BleDevice> {
    core_log::log_info("[ble] starting scan_for_rc003...");
    let devices = self::winrt::scan_paired()?;
    for d in &devices {
        if matches_rc003(&d.name) {
            core_log::log_info(&format!(
                "[ble] matched RC003 device: name='{}', id='{}'",
                d.name, d.id
            ));
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

/// 非 Windows 占位实现，使 crate 仍能编译并通过测试。
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

/// 扫描 RC003，然后发现其 ATVV 端点（仅 Windows）。
#[cfg(target_os = "windows")]
pub fn scan_and_connect() -> Result<(BleDevice, AtvvEndpoints)> {
    core_log::log_info("[ble] scan_and_connect initiated...");
    let device = scan_for_rc003()?;
    core_log::log_info(&format!(
        "[ble] device found: '{}', proceeding to discover ATVV endpoints...",
        device.name
    ));
    let endpoints = discover_atvv(&device.id)?;
    core_log::log_info(&format!(
        "[ble] ATVV endpoints discovered successfully: complete={}",
        endpoints.is_complete()
    ));
    Ok((device, endpoints))
}
