//! Windows GATT discovery + ATVV transport (Windows only).

use std::sync::{Arc, Mutex};

use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
    GattDeviceService, GattValueChangedEventArgs,
};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothLEDevice};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, DataWriter, IBuffer};

use crate::{BleError, Result};

/// ATVV service/characteristic UUIDs (match core-atvv constants).
const ATVV_SERVICE_GUID: GUID = GUID::from_u128(0xAB5E0001_5A21_4F05_BC7D_AF01F617B664);
const ATVV_TX_GUID: GUID = GUID::from_u128(0xAB5E0002_5A21_4F05_BC7D_AF01F617B664);
const ATVV_AUDIO_GUID: GUID = GUID::from_u128(0xAB5E0003_5A21_4F05_BC7D_AF01F617B664);
const ATVV_CONTROL_GUID: GUID = GUID::from_u128(0xAB5E0004_5A21_4F05_BC7D_AF01F617B664);

/// ATVV endpoints discovered on the remote (for UI overview).
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

/// An open ATVV connection with TX/Audio/Control characteristics kept alive.
pub struct AtvvLink {
    _service: GattDeviceService,
    _tx: GattCharacteristic,
    audio: GattCharacteristic,
    control: GattCharacteristic,
}

/// Discover the ATVV service/characteristics and return their ids for the UI.
pub fn discover_atvv(device_id: &str) -> Result<AtvvEndpoints> {
    let (chars, _service) = open_atvv_chars_and_service(device_id)?;

    let mut endpoints = AtvvEndpoints::default();
    for (uuid, c) in &chars {
        let u = c
            .Uuid()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        if *uuid == ATVV_TX_GUID {
            endpoints.tx = Some(format!("{u:?}"));
        } else if *uuid == ATVV_AUDIO_GUID {
            endpoints.audio = Some(format!("{u:?}"));
        } else if *uuid == ATVV_CONTROL_GUID {
            endpoints.control = Some(format!("{u:?}"));
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

/// Connect and open the ATVV transport (keeps characteristics alive).
pub fn connect_atvv(device_id: &str) -> Result<AtvvLink> {
    let (tx, audio, control, service) = open_atvv_chars_with_service(device_id)?;
    Ok(AtvvLink {
        _service: service,
        _tx: tx,
        audio,
        control,
    })
}

/// Open the ATVV service and return the three characteristics.
// Helper to split found list into tx/audio/control + service.
type FoundChars = Vec<(GUID, GattCharacteristic)>;
type AtvvChars = (GattCharacteristic, GattCharacteristic, GattCharacteristic, GattDeviceService);

fn open_atvv_chars_with_service(device_id: &str) -> Result<AtvvChars> {
    let (list, service) = open_atvv_chars_and_service(device_id)?;
    let mut audio = None;
    let mut control = None;
    let mut tx = None;
    for (uuid, c) in list {
        if uuid == ATVV_TX_GUID {
            tx = Some(c);
        } else if uuid == ATVV_AUDIO_GUID {
            audio = Some(c);
        } else if uuid == ATVV_CONTROL_GUID {
            control = Some(c);
        }
    }
    match (tx, audio, control) {
        (Some(t), Some(a), Some(c)) => Ok((t, a, c, service)),
        _ => Err(BleError::Windows("ATVV characteristics incomplete".to_string())),
    }
}

fn open_atvv_chars_and_service(
    device_id: &str,
) -> Result<(FoundChars, GattDeviceService)> {
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

    for service in &services {
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

            let mut found = Vec::new();
            for characteristic in &chars {
                let cuuid = characteristic
                    .Uuid()
                    .map_err(|e| BleError::Windows(e.to_string()))?;
                if cuuid == ATVV_TX_GUID
                    || cuuid == ATVV_AUDIO_GUID
                    || cuuid == ATVV_CONTROL_GUID
                {
                    found.push((cuuid, characteristic.clone()));
                }
            }
            return Ok((found, service.clone()));
        }
    }

    Err(BleError::Windows(
        "ATVV service not found on this device".to_string(),
    ))
}

impl AtvvLink {
    /// Connect using an already-scanned device id.
    pub fn connect(device_id: &str) -> Result<Self> {
        let (tx, audio, control, service) = open_atvv_chars_with_service(device_id)?;
        Ok(Self {
            _service: service,
            _tx: tx,
            audio,
            control,
        })
    }

    /// Enable notifications on the Audio characteristic so voice data flows.
    pub fn enable_audio_notifications(&self) -> Result<()> {
        let status = pollster::block_on(async {
            self.audio
                .WriteClientCharacteristicConfigurationDescriptorAsync(
                    GattClientCharacteristicConfigurationDescriptorValue::Notify,
                )
                .map_err(|e| BleError::Windows(e.to_string()))?
                .await
                .map_err(|e| BleError::Windows(e.to_string()))
        })?;
        let _ = status;
        Ok(())
    }

    /// Write a control command (opcode bytes supplied by higher layer).
    pub fn write_control(&self, bytes: &[u8]) -> Result<()> {
        let writer = DataWriter::new().map_err(|e| BleError::Windows(e.to_string()))?;
        writer
            .WriteBytes(bytes)
            .map_err(|e| BleError::Windows(e.to_string()))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|e| BleError::Windows(e.to_string()))?;

        pollster::block_on(async {
            self.control
                .WriteValueAsync(&buffer)
                .map_err(|e| BleError::Windows(e.to_string()))?
                .await
                .map_err(|e| BleError::Windows(e.to_string()))
        })?;
        Ok(())
    }

    /// Register a callback receiving raw Audio characteristic bytes.
    /// Returns the event cookie; call `remove_audio_handler` to stop.
    pub fn register_audio_handler<F>(&self, callback: F) -> Result<i64>
    where
        F: FnMut(Vec<u8>) + Send + 'static,
    {
        let shared = Arc::new(Mutex::new(callback));
        let handler = shared.clone();

        let event_handler: TypedEventHandler<GattCharacteristic, GattValueChangedEventArgs> =
            TypedEventHandler::new(move |_sender: windows::core::Ref<GattCharacteristic>, args: windows::core::Ref<GattValueChangedEventArgs>| {
            let event_args = match args.as_ref() {
                Some(v) => v,
                None => return Ok(()),
            };
            let buffer: IBuffer = match event_args.CharacteristicValue() {
                Ok(b) => b,
                Err(_) => return Ok(()),
            };
            if let Ok(data) = buffer_to_vec(&buffer) {
                if let Ok(mut guard) = handler.lock() {
                    guard(data);
                }
            }
            Ok(())
        });

        let cookie = self
            .audio
            .ValueChanged(&event_handler)
            .map_err(|e| BleError::Windows(e.to_string()))?;

        Ok(cookie)
    }

    /// Remove a previously registered audio handler.
    pub fn remove_audio_handler(&self, cookie: i64) -> Result<()> {
        self.audio
            .RemoveValueChanged(cookie)
            .map_err(|e| BleError::Windows(e.to_string()))
    }

    /// Keep the link alive until the process exits (for a standalone bridge).
    pub fn run_forever(&self) -> Result<()> {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
}

/// Read all bytes out of an IBuffer.
fn buffer_to_vec(buffer: &IBuffer) -> Result<Vec<u8>> {
    let len = buffer
        .Length()
        .map_err(|e| BleError::Windows(e.to_string()))? as usize;
    let mut out = vec![0u8; len];
    let reader = DataReader::FromBuffer(buffer)
        .map_err(|e| BleError::Windows(e.to_string()))?;
    reader
        .ReadBytes(&mut out)
        .map_err(|e| BleError::Windows(e.to_string()))?;
    Ok(out)
}
