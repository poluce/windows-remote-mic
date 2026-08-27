//! Windows GATT 发现 + ATVV 传输（仅 Windows）。

use std::sync::{Arc, Mutex};

use windows::core::{IInspectable, GUID, HSTRING};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
    GattCommunicationStatus, GattDeviceService, GattOpenStatus, GattSharingMode,
    GattValueChangedEventArgs,
};
use windows::Devices::Bluetooth::{
    BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice,
};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, DataWriter, IBuffer};

use crate::{BleError, Result};

/// ATVV 服务/特征 UUID（与 core-atvv 常量一致）。
const ATVV_SERVICE_GUID: GUID = GUID::from_u128(0xAB5E0001_5A21_4F05_BC7D_AF01F617B664);
const ATVV_TX_GUID: GUID = GUID::from_u128(0xAB5E0002_5A21_4F05_BC7D_AF01F617B664);
const ATVV_AUDIO_GUID: GUID = GUID::from_u128(0xAB5E0003_5A21_4F05_BC7D_AF01F617B664);
const ATVV_CONTROL_GUID: GUID = GUID::from_u128(0xAB5E0004_5A21_4F05_BC7D_AF01F617B664);

/// 在遥控器上发现的 ATVV 端点（用于 UI 概览）。
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

/// 一个保持 TX/Audio/Control 特征存活的 ATVV 连接。
pub struct AtvvLink {
    _device: BluetoothLEDevice,
    _service: GattDeviceService,
    _tx: GattCharacteristic,
    audio: GattCharacteristic,
    control: GattCharacteristic,
}

/// 发现 ATVV 服务/特征，并返回其 ID 供 UI 使用。
pub fn discover_atvv(device_id: &str) -> Result<AtvvEndpoints> {
    core_log::log_info(&format!(
        "[ble/gatt] 开始发现 ATVV 端点，设备 ID='{device_id}'"
    ));
    let (chars, _service) = open_atvv_chars_and_service(device_id)?;

    let mut endpoints = AtvvEndpoints::default();
    for (uuid, c) in &chars {
        let u = c.Uuid().map_err(|e| BleError::Windows(e.to_string()))?;
        if *uuid == ATVV_TX_GUID {
            endpoints.tx = Some(format!("{u:?}"));
        } else if *uuid == ATVV_AUDIO_GUID {
            endpoints.audio = Some(format!("{u:?}"));
        } else if *uuid == ATVV_CONTROL_GUID {
            endpoints.control = Some(format!("{u:?}"));
        }
    }

    if endpoints.is_complete() {
        core_log::log_info(&format!("[ble/gatt] ATVV 端点齐全: {:?}", endpoints));
        Ok(endpoints)
    } else {
        core_log::log_warn(&format!("[ble/gatt] ATVV 端点不完整: {:?}", endpoints));
        Err(BleError::Windows(
            "ATVV service/characteristics not found on this device".to_string(),
        ))
    }
}

/// 连接并打开 ATVV 传输（保持特征存活）。
pub fn connect_atvv(device_id: &str) -> Result<AtvvLink> {
    core_log::log_info(&format!("[ble/gatt] 开始连接 ATVV，设备 ID='{device_id}'"));
    let (tx, audio, control, service) = open_atvv_chars_with_service(device_id)?;
    let device = open_ble_device(device_id)?;
    core_log::log_info("[ble/gatt] ATVV 链路连接成功");
    Ok(AtvvLink {
        _device: device,
        _service: service,
        _tx: tx,
        audio,
        control,
    })
}

/// 打开 ATVV 服务并返回三个特征。
// 辅助：将找到的特征列表拆分为 tx/audio/control + 服务。
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
            core_log::log_error("[ble/gatt] ATVV 特征不完整（缺少 TX、Audio 或 Control）");
            Err(BleError::Windows(
                "ATVV characteristics incomplete".to_string(),
            ))
        }
    }
}

fn open_atvv_chars_and_service(device_id: &str) -> Result<(FoundChars, GattDeviceService)> {
    match try_open_atvv(device_id, BluetoothCacheMode::Cached) {
        Ok(v) => Ok(v),
        Err(e) => {
            core_log::log_warn(&format!(
                "[ble/gatt] 使用缓存打开 ATVV 失败: {e}；将在新设备上以非缓存模式重试"
            ));
            try_open_atvv(device_id, BluetoothCacheMode::Uncached)
        }
    }
}

fn open_ble_device(device_id: &str) -> Result<BluetoothLEDevice> {
    core_log::log_info(&format!(
        "[ble/gatt] 正在从设备 ID 打开蓝牙设备: '{device_id}'"
    ));
    let hstr = HSTRING::from(device_id);
    let device = pollster::block_on(async {
        let op =
            BluetoothLEDevice::FromIdAsync(&hstr).map_err(|e| BleError::Windows(e.to_string()))?;
        op.await.map_err(|e| BleError::Windows(e.to_string()))
    })?;
    core_log::log_info("[ble/gatt] 已获取蓝牙设备实例");

    if let Ok(op) = device.RequestAccessAsync() {
        match pollster::block_on(async { op.await }) {
            Ok(status) => core_log::log_info(&format!("[ble/gatt] 蓝牙访问请求状态={status:?}")),
            Err(e) => core_log::log_warn(&format!("[ble/gatt] 蓝牙访问请求失败: {e}")),
        }
    }

    Ok(device)
}

fn try_open_atvv(
    device_id: &str,
    mode: BluetoothCacheMode,
) -> Result<(FoundChars, GattDeviceService)> {
    core_log::log_info(&format!(
        "[ble/gatt] 正在从设备 ID 打开 ATVV 服务: '{device_id}' 模式={mode:?}"
    ));
    let device = open_ble_device(device_id)?;

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
        "[ble/gatt] 获取 GATT 服务（{mode:?}）状态={svc_status:?}"
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
        "[ble/gatt] 在设备上发现 {} 个 GATT 服务",
        services.Size().unwrap_or(0)
    ));

    for service in &services {
        let uuid = service
            .Uuid()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        if uuid != ATVV_SERVICE_GUID {
            continue;
        }
        core_log::log_info("[ble/gatt] 找到 ATVV 服务，正在以共享模式打开…");
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
                core_log::log_info(&format!("[ble/gatt] ATVV 打开（{mode:?}）-> {s:?}"));
                return Ok(());
            }
            Ok(s) => {
                core_log::log_warn(&format!("[ble/gatt] ATVV 打开（{mode:?}）-> {s:?}"));
            }
            Err(e) => {
                core_log::log_warn(&format!("[ble/gatt] ATVV 打开（{mode:?}）失败: {e}"));
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
        "[ble/gatt] 获取特征（{mode:?}）状态={char_status:?}"
    ));
    if char_status == GattCommunicationStatus::Success {
        let chars = char_result
            .Characteristics()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        core_log::log_info(&format!(
            "[ble/gatt] 在 ATVV 服务中发现 {} 个特征",
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

    core_log::log_warn("[ble/gatt] 获取全部特征失败或不完整；将按 UUID 查询 TX/Audio/Control");
    read_atvv_characteristics_by_uuid(service, mode)
}

fn push_atvv_char(found: &mut FoundChars, cuuid: GUID, characteristic: GattCharacteristic) {
    if cuuid == ATVV_TX_GUID {
        core_log::log_info("[ble/gatt] 找到 ATVV TX 特征");
        found.push((cuuid, characteristic));
    } else if cuuid == ATVV_AUDIO_GUID {
        core_log::log_info("[ble/gatt] 找到 ATVV AUDIO 特征");
        found.push((cuuid, characteristic));
    } else if cuuid == ATVV_CONTROL_GUID {
        core_log::log_info("[ble/gatt] 找到 ATVV CONTROL 特征");
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
            core_log::log_warn(&format!("[ble/gatt] 按 UUID 获取特征失败: {uuid:?}"));
            continue;
        };
        let status = char_result
            .Status()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        core_log::log_info(&format!(
            "[ble/gatt] 按 UUID 获取特征（{uuid:?}）状态={status:?}"
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
    /// 使用已扫描到的设备 ID 进行连接。
    pub fn connect(device_id: &str) -> Result<Self> {
        connect_atvv(device_id)
    }

    /// 启用 Audio 特征的通知，使语音数据可以流动。
    pub fn enable_audio_notifications(&self) -> Result<()> {
        core_log::log_info("[ble/gatt] 正在启用音频通知（CCCD Notify）…");
        self.enable_notifications(&self.audio)?;
        core_log::log_info("[ble/gatt] 音频通知启用成功");
        Ok(())
    }

    /// 启用 Control 特征的通知（设备事件）。
    pub fn enable_control_notifications(&self) -> Result<()> {
        core_log::log_info("[ble/gatt] 正在启用控制通知（CCCD Notify）…");
        self.enable_notifications(&self.control)?;
        core_log::log_info("[ble/gatt] 控制通知启用成功");
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
        core_log::log_debug(&format!(
            "[ble/gatt] 写入客户端特征配置描述符状态: {:?}",
            status
        ));
        Ok(())
    }

    /// 主机 -> 设备的命令字节写入 TX 特征。
    pub fn write_tx(&self, bytes: &[u8]) -> Result<()> {
        core_log::log_info(&format!(
            "[ble/gatt] 正在向 TX 写入 {} 字节: {:02X?}",
            bytes.len(),
            bytes
        ));
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
        core_log::log_info(&format!("[ble/gatt] TX 写入完成，状态: {:?}", status));
        Ok(())
    }

    /// 旧调用方使用的向后兼容别名。
    pub fn write_control(&self, bytes: &[u8]) -> Result<()> {
        self.write_tx(bytes)
    }

    /// 注册一个接收原始 Audio 特征字节的回调。
    /// 返回事件 cookie；调用 `remove_audio_handler` 停止。
    pub fn register_audio_handler<F>(&self, callback: F) -> Result<i64>
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

        let cookie = self
            .audio
            .ValueChanged(&event_handler)
            .map_err(|e| BleError::Windows(e.to_string()))?;

        Ok(cookie)
    }

    /// 注册一个接收 Control 特征通知字节的回调。
    pub fn register_control_handler<F>(&self, callback: F) -> Result<i64>
    where
        F: FnMut(Vec<u8>) + Send + 'static,
    {
        self.register_value_changed_handler(&self.control, callback)
    }

    /// 移除之前注册的音频处理器。
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

    /// 注册一个在 BLE 连接状态变化时触发的回调。
    pub fn register_connection_status_changed<F>(&self, callback: F) -> Result<i64>
    where
        F: FnMut(bool) + Send + 'static,
    {
        let handler = Arc::new(Mutex::new(callback));
        let event_handler = handler.clone();

        let typed =
            TypedEventHandler::<BluetoothLEDevice, IInspectable>::new(move |sender, _args| {
                if let Some(device) = sender.as_ref() {
                    if let Ok(status) = device.ConnectionStatus() {
                        if let Ok(mut guard) = event_handler.lock() {
                            guard(status == BluetoothConnectionStatus::Connected);
                        }
                    }
                }
                Ok(())
            });

        self._device
            .ConnectionStatusChanged(&typed)
            .map_err(|e| BleError::Windows(e.to_string()))
    }

    /// 保持链路存活直到进程退出（用于独立桥接程序）。
    pub fn run_forever(&self) -> Result<()> {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }
}

/// 从 IBuffer 中读取全部字节。
fn buffer_to_vec(buffer: &IBuffer) -> Result<Vec<u8>> {
    let len = buffer
        .Length()
        .map_err(|e| BleError::Windows(e.to_string()))? as usize;
    let mut out = vec![0u8; len];
    let reader = DataReader::FromBuffer(buffer).map_err(|e| BleError::Windows(e.to_string()))?;
    reader
        .ReadBytes(&mut out)
        .map_err(|e| BleError::Windows(e.to_string()))?;
    Ok(out)
}
