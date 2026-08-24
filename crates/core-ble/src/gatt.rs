//! Windows GATT discovery for ATVV (Windows only).

use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice};

use crate::{BleError, Result};

/// ATVV service/characteristic UUIDs (match core-atvv constants).
const ATVV_SERVICE_GUID: GUID = GUID::from_u128(0xAB5E0001_5A21_4F05_BC7D_AF01F617B664);
const ATVV_TX_GUID: GUID = GUID::from_u128(0xAB5E0002_5A21_4F05_BC7D_AF01F617B664);
const ATVV_AUDIO_GUID: GUID = GUID::from_u128(0xAB5E0003_5A21_4F05_BC7D_AF01F617B664);
const ATVV_CONTROL_GUID: GUID = GUID::from_u128(0xAB5E0004_5A21_4F05_BC7D_AF01F617B664);

/// ATVV endpoints discovered on the remote.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AtvvEndpoints {
    pub tx: Option<String>,
    pub audio: Option<String>,
    pub control: Option<String>,
}

impl AtvvEndpoints {
    pub fn is_complete(&self) -> bool {
        self.tx.is_some() && self.audio.is_some() && self.control.is_some()
    }
}

/// Connect to a device by its Windows id and enumerate the ATVV service with
/// UNCACHED characteristics.
pub fn discover_atvv(device_id: &str) -> Result<AtvvEndpoints> {
    let hstr = HSTRING::from(device_id);

    let device = pollster::block_on(async {
        BluetoothLEDevice::FromIdAsync(&hstr)
            .map_err(|e| BleError::Windows(e.to_string()))?
            .await
            .map_err(|e| BleError::Windows(e.to_string()))
    })?;

    let services = device
        .GattServices()
        .map_err(|e| BleError::Windows(e.to_string()))?;

    let mut endpoints = AtvvEndpoints::default();
    for service in services {
        let uuid = service
            .Uuid()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        if uuid == ATVV_SERVICE_GUID {
            let result = pollster::block_on(async {
                service
                    .GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)
                    .map_err(|e| BleError::Windows(e.to_string()))?
                    .await
                    .map_err(|e| BleError::Windows(e.to_string()))
            })?;

            let chars = result
                .Characteristics()
                .map_err(|e| BleError::Windows(e.to_string()))?;

            for characteristic in chars {
                let cuuid = characteristic
                    .Uuid()
                    .map_err(|e| BleError::Windows(e.to_string()))?;
                if cuuid == ATVV_TX_GUID {
                    endpoints.tx = Some(format!("{cuuid:?}"));
                } else if cuuid == ATVV_AUDIO_GUID {
                    endpoints.audio = Some(format!("{cuuid:?}"));
                } else if cuuid == ATVV_CONTROL_GUID {
                    endpoints.control = Some(format!("{cuuid:?}"));
                }
            }
            break;
        }
    }

    if endpoints.is_complete() {
        Ok(endpoints)
    } else {
        Err(BleError::Windows(
            "ATVV service/characteristics not found on this device".to_string(),
        ))
    }
}
