//! Windows GATT discovery + ATVV transport (Windows only).

use std::sync::{Arc, Mutex};

use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
    GattCommunicationStatus, GattDeviceService, GattOpenStatus, GattSharingMode,
    GattValueChangedEventArgs,
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
    core_log::log_info(&format!("[ble/gatt] discover_atvv called for device_id='{device_id}'"));
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
        core_log::log_info(&format!("[ble/gatt] ATVV endpoints complete: {:?}", endpoints));
        Ok(endpoints)
    } else {
        core_log::log_warn(&format!("[ble/gatt] ATVV endpoints incomplete: {:?}", endpoints));
        Err(BleError::Windows(
            "ATVV service/characteristics not found on this device".to_string(),
        ))
    }
}

/// Connect and open the ATVV transport (keeps characteristics alive).
pub fn connect_atvv(device_id: &str) -> Result<AtvvLink> {
    core_log::log_info(&format!("[ble/gatt] connect_atvv called for device_id='{device_id}'"));
    let (tx, audio, control, service) = open_atvv_chars_with_service(device_id)?;
    core_log::log_info("[ble/gatt] connect_atvv established AtvvLink successfully");
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
type AtvvChars = (
    GattCharacteristic,
    GattCharacteristic,
    GattCharacteristic,
    GattDeviceService,
);

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
        _ => {
            core_log::log_error("[ble/gatt] ATVV characteristics incomplete (missing TX, Audio, or Control)");
            Err(BleError::Windows("ATVV characteristics incomplete".to_string()))
        }
    }
}

fn open_atvv_chars_and_service(
    device_id: &str,
) -> Result<(FoundChars, GattDeviceService)> {
    match try_open_atvv(device_id, BluetoothCacheMode::Cached) {
        Ok(v) => Ok(v),
        Err(e) => {
            core_log::log_warn(&format!(
                "[ble/gatt] Cached ATVV open failed: {e}; retrying Uncached on a fresh device"
            ));
            try_open_atvv(device_id, BluetoothCacheMode::Uncached)
        }
    }
}

fn try_open_atvv(
    device_id: &str,
    mode: BluetoothCacheMode,
) -> Result<(FoundChars, GattDeviceService)> {
    core_log::log_info(&format!(
        "[ble/gatt] opening BluetoothLEDevice from ID: '{device_id}' mode={mode:?}"
    ));
    let hstr = HSTRING::from(device_id);
    let device = pollster::block_on(async {
        let op = BluetoothLEDevice::FromIdAsync(&hstr).map_err(|e| BleError::Windows(e.to_string()))?;
        op.await.map_err(|e| BleError::Windows(e.to_string()))
    })?;
    core_log::log_info("[ble/gatt] BluetoothLEDevice instance obtained");

    if let Ok(op) = device.RequestAccessAsync() {
        match pollster::block_on(async { op.await }) {
            Ok(status) => core_log::log_info(&format!("[ble/gatt] RequestAccessAsync status={status:?}")),
            Err(e) => core_log::log_warn(&format!("[ble/gatt] RequestAccessAsync failed: {e}")),
        }
    }

    let services_result = pollster::block_on(async {
        let op = device
            .GetGattServicesWithCacheModeAsync(mode)
            .map_err(|e| BleError::Windows(e.to_string()))?;
        op.await.map_err(|e| BleError::Windows(e.to_string()))
    })?;
    let svc_status = services_result
        .Status()
        .map_err(|e| BleError::Windows(e.to_string()))?;
    core_log::log_info(&format!(
        "[ble/gatt] GetGattServices({mode:?}) status={svc_status:?}"
    ));
    if svc_status != GattCommunicationStatus::Success {
        return Err(BleError::Windows(format!(
            "GetGattServices status={svc_status:?}"
        )));
    }
    let services = services_result
        .Services()
        .map_err(|e| BleError::Windows(e.to_string()))?;
    core_log::log_info(&format!(
        "[ble/gatt] found {} GATT services on device",
        services.Size().unwrap_or(0)
    ));

    for service in &services {
        let uuid = service
            .Uuid()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        if uuid != ATVV_SERVICE_GUID {
            continue;
        }
        core_log::log_info("[ble/gatt] matched ATVV service, opening for shared access...");
        open_atvv_service(&service)?;
        let found = read_atvv_characteristics(&service, mode)?;
        return Ok((found, service.clone()));
    }

    Err(BleError::Windows(
        "ATVV service not found on this device".into(),
    ))
}

fn open_atvv_service(service: &GattDeviceService) -> Result<()> {
    for mode in [
        GattSharingMode::SharedReadAndWrite,
        GattSharingMode::SharedReadOnly,
        GattSharingMode::Unspecified,
    ] {
        let status = pollster::block_on(async {
            let op = service
                .OpenAsync(mode)
                .map_err(|e| BleError::Windows(e.to_string()))?;
            op.await.map_err(|e| BleError::Windows(e.to_string()))
        });
        match status {
            Ok(s) if s == GattOpenStatus::Success || s == GattOpenStatus::AlreadyOpened => {
                core_log::log_info(&format!("[ble/gatt] ATVV OpenAsync({mode:?}) -> {s:?}"));
                return Ok(());
            }
            Ok(s) => {
                core_log::log_warn(&format!("[ble/gatt] ATVV OpenAsync({mode:?}) -> {s:?}"));
            }
            Err(e) => {
                core_log::log_warn(&format!("[ble/gatt] ATVV OpenAsync({mode:?}) failed: {e}"));
            }
        }
    }
    Ok(())
}

fn read_atvv_characteristics(
    service: &GattDeviceService,
    mode: BluetoothCacheMode,
) -> Result<FoundChars> {
    let char_result = pollster::block_on(async {
        let op = service
            .GetCharacteristicsWithCacheModeAsync(mode)
            .map_err(|e| BleError::Windows(e.to_string()))?;
        op.await.map_err(|e| BleError::Windows(e.to_string()))
    })?;
    let char_status = char_result
        .Status()
        .map_err(|e| BleError::Windows(e.to_string()))?;
    core_log::log_info(&format!(
        "[ble/gatt] GetCharacteristics({mode:?}) status={char_status:?}"
    ));
    if char_status == GattCommunicationStatus::Success {
        let chars = char_result
            .Characteristics()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        core_log::log_info(&format!(
            "[ble/gatt] found {} characteristics in ATVV service",
            chars.Size().unwrap_or(0)
        ));
        let mut found = Vec::new();
        for characteristic in &chars {
            let cuuid = characteristic
                .Uuid()
                .map_err(|e| BleError::Windows(e.to_string()))?;
            push_atvv_char(&mut found, cuuid, characteristic.clone());
        }
        if found.len() >= 3 {
            return Ok(found);
        }
    }

    core_log::log_warn(
        "[ble/gatt] GetCharacteristics-all failed or incomplete; querying TX/Audio/Control by UUID",
    );
    read_atvv_characteristics_by_uuid(service, mode)
}

fn push_atvv_char(found: &mut FoundChars, cuuid: GUID, characteristic: GattCharacteristic) {
    if cuuid == ATVV_TX_GUID {
        core_log::log_info("[ble/gatt] found ATVV TX characteristic");
        found.push((cuuid, characteristic));
    } else if cuuid == ATVV_AUDIO_GUID {
        core_log::log_info("[ble/gatt] found ATVV AUDIO characteristic");
        found.push((cuuid, characteristic));
    } else if cuuid == ATVV_CONTROL_GUID {
        core_log::log_info("[ble/gatt] found ATVV CONTROL characteristic");
        found.push((cuuid, characteristic));
    }
}

fn read_atvv_characteristics_by_uuid(
    service: &GattDeviceService,
    mode: BluetoothCacheMode,
) -> Result<FoundChars> {
    let mut found = Vec::new();
    for uuid in [ATVV_TX_GUID, ATVV_AUDIO_GUID, ATVV_CONTROL_GUID] {
        let result = pollster::block_on(async {
            let op = service
                .GetCharacteristicsForUuidWithCacheModeAsync(uuid, mode)
                .map_err(|e| BleError::Windows(e.to_string()))?;
            op.await.map_err(|e| BleError::Windows(e.to_string()))
        });
        let Ok(char_result) = result else {
            core_log::log_warn(&format!("[ble/gatt] GetCharacteristicsForUuid {uuid:?} failed"));
            continue;
        };
        let status = char_result
            .Status()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        core_log::log_info(&format!(
            "[ble/gatt] GetCharacteristicsForUuid {uuid:?} status={status:?}"
        ));
        if status != GattCommunicationStatus::Success {
            continue;
        }
        let Ok(chars) = char_result.Characteristics() else {
            continue;
        };
        if let Ok(ch) = chars.GetAt(0) {
            if let Ok(cuuid) = ch.Uuid() {
                push_atvv_char(&mut found, cuuid, ch);
            }
        }
    }
    if found.len() < 3 {
        return Err(BleError::Windows(format!(
            "ATVV GetCharacteristics status={:?} (by-uuid got {})",
            GattCommunicationStatus::AccessDenied,
            found.len()
        )));
    }
    Ok(found)
}

impl AtvvLink {
    /// Connect using an already-scanned device id.
    pub fn connect(device_id: &str) -> Result<Self> {
        connect_atvv(device_id)
    }

    /// Enable notifications on the Audio characteristic so voice data flows.
    pub fn enable_audio_notifications(&self) -> Result<()> {
        core_log::log_info("[ble/gatt] enabling audio notifications (CCCD Notify)...");
        self.enable_notifications(&self.audio)?;
        core_log::log_info("[ble/gatt] audio notifications enabled successfully");
        Ok(())
    }

    /// Enable notifications on the Control characteristic (device events).
    pub fn enable_control_notifications(&self) -> Result<()> {
        core_log::log_info("[ble/gatt] enabling control notifications (CCCD Notify)...");
        self.enable_notifications(&self.control)?;
        core_log::log_info("[ble/gatt] control notifications enabled successfully");
        Ok(())
    }

    fn enable_notifications(&self, characteristic: &GattCharacteristic) -> Result<()> {
        let status = pollster::block_on(async {
            characteristic
                .WriteClientCharacteristicConfigurationDescriptorAsync(
                    GattClientCharacteristicConfigurationDescriptorValue::Notify,
                )
                .map_err(|e| BleError::Windows(e.to_string()))?
                .await
                .map_err(|e| BleError::Windows(e.to_string()))
        })?;
        core_log::log_debug(&format!("[ble/gatt] WriteClientCharacteristicConfigurationDescriptor status: {:?}", status));
        Ok(())
    }

    /// Host -> device command bytes are written to the TX characteristic.
    pub fn write_tx(&self, bytes: &[u8]) -> Result<()> {
        core_log::log_info(&format!("[ble/gatt] writing {} bytes to TX: {:02X?}", bytes.len(), bytes));
        let writer = DataWriter::new().map_err(|e| BleError::Windows(e.to_string()))?;
        writer
            .WriteBytes(bytes)
            .map_err(|e| BleError::Windows(e.to_string()))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|e| BleError::Windows(e.to_string()))?;

        let status = pollster::block_on(async {
            self._tx
                .WriteValueAsync(&buffer)
                .map_err(|e| BleError::Windows(e.to_string()))?
                .await
                .map_err(|e| BleError::Windows(e.to_string()))
        })?;
        core_log::log_info(&format!("[ble/gatt] write_tx completed with status: {:?}", status));
        Ok(())
    }

    /// Backwards-compatible alias used by older callers.
    pub fn write_control(&self, bytes: &[u8]) -> Result<()> {
        self.write_tx(bytes)
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

    /// Register a callback receiving Control characteristic notification bytes.
    pub fn register_control_handler<F>(&self, callback: F) -> Result<i64>
    where
        F: FnMut(Vec<u8>) + Send + 'static,
    {
        self.register_value_changed_handler(&self.control, callback)
    }

    /// Remove a previously registered audio handler.
    pub fn remove_audio_handler(&self, cookie: i64) -> Result<()> {
        self.audio
            .RemoveValueChanged(cookie)
            .map_err(|e| BleError::Windows(e.to_string()))
    }

    fn register_value_changed_handler<F>(
        &self,
        characteristic: &GattCharacteristic,
        callback: F,
    ) -> Result<i64>
    where
        F: FnMut(Vec<u8>) + Send + 'static,
    {
        let shared = Arc::new(Mutex::new(callback));
        let handler = shared.clone();

        let event_handler: TypedEventHandler<GattCharacteristic, GattValueChangedEventArgs> =
            TypedEventHandler::new(
                move |_sender: windows::core::Ref<GattCharacteristic>,
                      args: windows::core::Ref<GattValueChangedEventArgs>| {
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
                },
            );

        let cookie = characteristic
            .ValueChanged(&event_handler)
            .map_err(|e| BleError::Windows(e.to_string()))?;
        Ok(cookie)
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
