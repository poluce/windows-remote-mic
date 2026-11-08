//! Windows GATT 发现 + ATVV 传输（仅 Windows）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::core::{IInspectable, GUID, HSTRING};
use windows::Devices::Bluetooth::GenericAttributeProfile::{
    GattCharacteristic, GattClientCharacteristicConfigurationDescriptorValue,
    GattCommunicationStatus, GattDeviceService, GattOpenStatus, GattSession, GattSharingMode,
    GattValueChangedEventArgs,
};
use windows::Devices::Bluetooth::{
    BluetoothCacheMode, BluetoothConnectionStatus, BluetoothLEDevice,
};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::{DataReader, DataWriter, IBuffer};

use crate::async_wait::wait_op;
use crate::{BleError, Result};

/// 常规 GATT 操作超时（发现服务/特征、读写等）。
const OP_TIMEOUT: Duration = Duration::from_secs(15);
/// 打开设备/服务等相对更重的操作超时。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// 权限/订阅等轻量操作超时。
const LIGHT_TIMEOUT: Duration = Duration::from_secs(10);

/// ATVV 服务/特征 UUID（与 core-atvv 常量一致）。
const ATVV_SERVICE_GUID: GUID = GUID::from_u128(0xAB5E0001_5A21_4F05_BC7D_AF01F617B664);
const ATVV_TX_GUID: GUID = GUID::from_u128(0xAB5E0002_5A21_4F05_BC7D_AF01F617B664);
const ATVV_AUDIO_GUID: GUID = GUID::from_u128(0xAB5E0003_5A21_4F05_BC7D_AF01F617B664);
const ATVV_CONTROL_GUID: GUID = GUID::from_u128(0xAB5E0004_5A21_4F05_BC7D_AF01F617B664);

/// 标准 Battery Service 与 Battery Level Characteristic UUID
pub const BATTERY_SERVICE_GUID: GUID = GUID::from_u128(0x0000180F_0000_1000_8000_00805F9B34FB);
pub const BATTERY_LEVEL_GUID: GUID = GUID::from_u128(0x00002A19_0000_1000_8000_00805F9B34FB);

/// 从字节切片解析电池电量百分比（0~100）。
pub fn parse_battery_level(bytes: &[u8]) -> Option<u8> {
    bytes.first().copied().map(|b| b.min(100))
}

pub type BatteryCallback = Box<dyn Fn(u8) + Send + Sync + 'static>;

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
    /// GATT 会话：`MaintainConnection=true` 时系统不会因为空闲而回收会话。
    /// 实测未设置时链路会在约 45s 空闲后被拆掉、再重连，形成无谓的会话抖动。
    session: GattSession,
    _service: GattDeviceService,
    _tx: GattCharacteristic,
    audio: GattCharacteristic,
    control: GattCharacteristic,
    battery_level: Arc<Mutex<Option<u8>>>,
    _battery_service: Option<GattDeviceService>,
    battery_char: Option<GattCharacteristic>,
    _battery_cookie: Option<i64>,
    battery_callbacks: Arc<Mutex<Vec<BatteryCallback>>>,
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
    core_log::log_debug(&format!("[ble/gatt] 开始连接 ATVV，设备 ID='{device_id}'"));
    let (tx, audio, control, service) = open_atvv_chars_with_service(device_id)?;
    // 维持 GATT 会话：否则系统在空闲约 45s 后回收会话并拆链（实测每场会话
    // 都是精确 45.0s 后断开，随后重连，形成无意义的会话抖动）。
    let session = service
        .Session()
        .map_err(|e| BleError::Windows(format!("获取 GATT 会话失败: {e}")))?;
    match session.CanMaintainConnection() {
        Ok(true) => match session.SetMaintainConnection(true) {
            Ok(()) => {
                core_log::log_debug("[ble/gatt] 已开启 GATT 会话维持（MaintainConnection=true）")
            }
            Err(e) => core_log::log_warn(&format!("[ble/gatt] 开启 GATT 会话维持失败: {e}")),
        },
        Ok(false) => {
            core_log::log_warn("[ble/gatt] 设备不支持维持 GATT 会话（CanMaintainConnection=false）")
        }
        Err(e) => core_log::log_warn(&format!("[ble/gatt] 读取 CanMaintainConnection 失败: {e}")),
    }
    let device = open_ble_device(device_id)?;
    match device.ConnectionStatus() {
        Ok(status) => core_log::log_debug(&format!(
            "[ble/gatt] ATVV 链路连接成功，当前 ConnectionStatus={status:?}"
        )),
        Err(e) => core_log::log_warn(&format!(
            "[ble/gatt] ATVV 链路已打开，但读取 ConnectionStatus 失败: {e}"
        )),
    }

    let battery = setup_battery_service(&device).unwrap_or_else(|e| {
        core_log::log_debug(&format!(
            "[ble/gatt] 未能建立电池服务连接（不影响 ATVV）: {e}"
        ));
        BatteryServiceSetup::default()
    });

    Ok(AtvvLink {
        _device: device,
        session,
        _service: service,
        _tx: tx,
        audio,
        control,
        battery_level: battery.level,
        _battery_service: battery.service,
        battery_char: battery.characteristic,
        _battery_cookie: battery.cookie,
        battery_callbacks: battery.callbacks,
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

pub(crate) fn open_ble_device(device_id: &str) -> Result<BluetoothLEDevice> {
    core_log::log_debug(&format!(
        "[ble/gatt] 正在从设备 ID 打开蓝牙设备: '{device_id}'"
    ));
    let hstr = HSTRING::from(device_id);
    let op = BluetoothLEDevice::FromIdAsync(&hstr).map_err(|e| BleError::Windows(e.to_string()))?;
    let device = wait_op(&op, CONNECT_TIMEOUT, "打开蓝牙设备")?;
    core_log::log_debug("[ble/gatt] 已获取蓝牙设备实例");

    if let Ok(op) = device.RequestAccessAsync() {
        match wait_op(&op, LIGHT_TIMEOUT, "请求蓝牙访问权限") {
            Ok(status) => core_log::log_debug(&format!("[ble/gatt] 蓝牙访问请求状态={status:?}")),
            Err(e) => core_log::log_warn(&format!("[ble/gatt] 蓝牙访问请求失败: {e}")),
        }
    }

    Ok(device)
}

fn try_open_atvv(
    device_id: &str,
    mode: BluetoothCacheMode,
) -> Result<(FoundChars, GattDeviceService)> {
    core_log::log_debug(&format!(
        "[ble/gatt] 正在从设备 ID 打开 ATVV 服务: '{device_id}' 模式={mode:?}"
    ));
    let device = open_ble_device(device_id)?;

    let op = device
        .GetGattServicesWithCacheModeAsync(mode)
        .map_err(|e| BleError::Windows(e.to_string()))?;
    let services_result = wait_op(&op, OP_TIMEOUT, "获取 GATT 服务")?;
    let svc_status = services_result
        .Status()
        .map_err(|e| BleError::Windows(e.to_string()))?;
    core_log::log_debug(&format!(
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
    core_log::log_debug(&format!(
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
        core_log::log_debug("[ble/gatt] 找到 ATVV 服务，正在以共享模式打开…");
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
        let op = match service.OpenAsync(mode) {
            Ok(op) => op,
            Err(e) => {
                core_log::log_warn(&format!("[ble/gatt] ATVV 打开（{mode:?}）调用失败: {e}"));
                continue;
            }
        };
        match wait_op(&op, LIGHT_TIMEOUT, "打开 ATVV 服务") {
            Ok(s) if s == GattOpenStatus::Success || s == GattOpenStatus::AlreadyOpened => {
                core_log::log_debug(&format!("[ble/gatt] ATVV 打开（{mode:?}）-> {s:?}"));
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
    // 三种共享模式都打不开：必须显式失败，避免后续读特征“看起来成功、实际无数据”。
    Err(BleError::Windows(
        "ATVV 服务打开失败：共享读写/只读/未指定三种模式均不可用".into(),
    ))
}

fn read_atvv_characteristics(
    service: &GattDeviceService,
    mode: BluetoothCacheMode,
) -> Result<FoundChars> {
    let op = service
        .GetCharacteristicsWithCacheModeAsync(mode)
        .map_err(|e| BleError::Windows(e.to_string()))?;
    let char_result = wait_op(&op, OP_TIMEOUT, "获取 ATVV 特征")?;
    let char_status = char_result
        .Status()
        .map_err(|e| BleError::Windows(e.to_string()))?;
    core_log::log_debug(&format!(
        "[ble/gatt] 获取特征（{mode:?}）状态={char_status:?}"
    ));
    if char_status == GattCommunicationStatus::Success {
        let chars = char_result
            .Characteristics()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        core_log::log_debug(&format!(
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
        core_log::log_debug("[ble/gatt] 找到 ATVV TX 特征");
        found.push((cuuid, characteristic));
    } else if cuuid == ATVV_AUDIO_GUID {
        core_log::log_debug("[ble/gatt] 找到 ATVV AUDIO 特征");
        found.push((cuuid, characteristic));
    } else if cuuid == ATVV_CONTROL_GUID {
        core_log::log_debug("[ble/gatt] 找到 ATVV CONTROL 特征");
        found.push((cuuid, characteristic));
    }
}

fn read_atvv_characteristics_by_uuid(
    service: &GattDeviceService,
    mode: BluetoothCacheMode,
) -> Result<FoundChars> {
    let mut found = Vec::new();
    for uuid in [ATVV_TX_GUID, ATVV_AUDIO_GUID, ATVV_CONTROL_GUID] {
        let result = match service.GetCharacteristicsForUuidWithCacheModeAsync(uuid, mode) {
            Ok(op) => wait_op(&op, LIGHT_TIMEOUT, "按 UUID 获取特征"),
            Err(e) => Err(BleError::Windows(e.to_string())),
        };
        let Ok(char_result) = result else {
            core_log::log_warn(&format!("[ble/gatt] 按 UUID 获取特征失败: {uuid:?}"));
            continue;
        };
        let status = char_result
            .Status()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        core_log::log_debug(&format!(
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

    /// 当前链路是否仍处于已连接状态（直接读系统状态）。
    ///
    /// 用于兜底：偶尔 WinRT 的 `ConnectionStatusChanged` 事件不会及时送达
    /// （例如蓝牙无线电被关闭/驱动卡住），会话循环需要能主动发现断链。
    pub fn is_connected(&self) -> bool {
        matches!(
            self._device.ConnectionStatus(),
            Ok(BluetoothConnectionStatus::Connected)
        )
    }

    /// 启用 Audio 特征的通知，使语音数据可以流动。
    pub fn enable_audio_notifications(&self) -> Result<()> {
        core_log::log_debug("[ble/gatt] 正在启用音频通知（CCCD Notify）…");
        self.enable_notifications(&self.audio)?;
        core_log::log_debug("[ble/gatt] 音频通知启用成功");
        Ok(())
    }

    /// 启用 Control 特征的通知（设备事件）。
    pub fn enable_control_notifications(&self) -> Result<()> {
        core_log::log_debug("[ble/gatt] 正在启用控制通知（CCCD Notify）…");
        self.enable_notifications(&self.control)?;
        core_log::log_debug("[ble/gatt] 控制通知启用成功");
        Ok(())
    }

    fn enable_notifications(&self, characteristic: &GattCharacteristic) -> Result<()> {
        let op = characteristic
            .WriteClientCharacteristicConfigurationDescriptorAsync(
                GattClientCharacteristicConfigurationDescriptorValue::Notify,
            )
            .map_err(|e| BleError::Windows(e.to_string()))?;
        let status = wait_op(&op, LIGHT_TIMEOUT, "写 CCCD 启用通知")?;
        if status != GattCommunicationStatus::Success {
            // 之前这里只打 DEBUG 日志就当作成功，导致“链路看着正常、永远收不到通知”
            // 的半死状态无法被发现（实测曾空转数小时）。必须显式失败。
            core_log::log_error(&format!(
                "[ble/gatt] 启用通知失败: CCCD 写入状态={status:?}"
            ));
            return Err(BleError::Windows(format!(
                "写 CCCD 启用通知失败: {status:?}"
            )));
        }
        core_log::log_debug(&format!(
            "[ble/gatt] 写入客户端特征配置描述符状态: {:?}",
            status
        ));
        Ok(())
    }

    /// 主机 -> 设备的命令字节写入 TX 特征。
    pub fn write_tx(&self, bytes: &[u8]) -> Result<()> {
        core_log::log_debug(&format!(
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

        let op = self
            ._tx
            .WriteValueAsync(&buffer)
            .map_err(|e| BleError::Windows(e.to_string()))?;
        let status = wait_op(&op, LIGHT_TIMEOUT, "写 TX 特征")?;
        if status != GattCommunicationStatus::Success {
            core_log::log_error(&format!("[ble/gatt] TX 写入失败，状态: {status:?}"));
            return Err(BleError::Windows(format!("TX 写入失败: {status:?}")));
        }
        core_log::log_debug(&format!("[ble/gatt] TX 写入完成，状态: {status:?}"));
        Ok(())
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

        let typed = TypedEventHandler::<BluetoothLEDevice, IInspectable>::new(
            move |sender, _args| {
                if let Some(device) = sender.as_ref() {
                    let device_id = device
                        .DeviceId()
                        .ok()
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "(unknown)".into());
                    let status = match device.ConnectionStatus() {
                        Ok(s) => s,
                        Err(e) => {
                            core_log::log_warn(&format!(
                                "[ble/gatt] ConnectionStatusChanged 但读取状态失败: device={device_id}, err={e}"
                            ));
                            return Ok(());
                        }
                    };
                    let connected = status == BluetoothConnectionStatus::Connected;
                    // 断链定位：记录 WinRT 原始枚举，区分 Connected/Disconnected。
                    core_log::log_info(&format!(
                        "[ble/gatt] ConnectionStatusChanged: status={status:?}, connected={connected}, device={device_id}"
                    ));
                    if let Ok(mut guard) = event_handler.lock() {
                        guard(connected);
                    }
                } else {
                    core_log::log_warn(
                        "[ble/gatt] ConnectionStatusChanged: sender 为空，无法读取设备状态",
                    );
                }
                Ok(())
            },
        );

        self._device
            .ConnectionStatusChanged(&typed)
            .map_err(|e| BleError::Windows(e.to_string()))
    }

    /// 获取当前记录的遥控器电池电量百分比（0~100）。
    pub fn battery_level(&self) -> Option<u8> {
        self.battery_level.lock().ok().and_then(|g| *g)
    }

    /// 主动刷新并读取一次电池电量。
    pub fn refresh_battery_level(&self) -> Option<u8> {
        let Some(ref ch) = self.battery_char else {
            return self.battery_level();
        };
        if let Some(level) = read_battery_level_from_char(ch) {
            if let Ok(mut g) = self.battery_level.lock() {
                *g = Some(level);
            }
            if let Ok(listeners) = self.battery_callbacks.lock() {
                for cb in listeners.iter() {
                    cb(level);
                }
            }
            return Some(level);
        }
        self.battery_level()
    }

    /// 注册电池电量变更（通过通知或主动刷新时）的回调。
    pub fn register_battery_handler<F: Fn(u8) + Send + Sync + 'static>(&self, callback: F) {
        if let Ok(mut g) = self.battery_callbacks.lock() {
            g.push(Box::new(callback));
        }
    }
}

impl Drop for AtvvLink {
    fn drop(&mut self) {
        // 若先 Drop 链路再收到 WinRT 断连回调，可据此判断是“我们主动释放”还是“对端/系统先断”。
        let status = self
            ._device
            .ConnectionStatus()
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|e| format!("read_err={e}"));
        core_log::log_info(&format!(
            "[ble/gatt] AtvvLink Drop：释放 GATT 句柄，此刻 ConnectionStatus={status}"
        ));
        if let Some(cookie) = self._battery_cookie.take() {
            if let Some(ref ch) = self.battery_char {
                let _ = ch.RemoveValueChanged(cookie);
            }
        }
        if let Some(ref service) = self._battery_service {
            let _ = service.Close();
        }
        // 显式解除会话维持并关闭句柄：WinRT 的 GattDeviceService / GattSession
        // 都实现了 IClosable，不 Close 会留下共享句柄，影响下一轮会话的通知订阅。
        let _ = self.session.SetMaintainConnection(false);
        let _ = self.session.Close();
        let _ = self._service.Close();
        core_log::log_debug("[ble/gatt] GATT 会话与服务句柄已显式关闭");
    }
}

pub use super::watcher::{is_link_connected, LinkWatcher};

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

fn find_battery_service(
    device: &BluetoothLEDevice,
    mode: BluetoothCacheMode,
) -> Result<GattDeviceService> {
    if let Ok(op) = device.GetGattServicesForUuidWithCacheModeAsync(BATTERY_SERVICE_GUID, mode) {
        if let Ok(res) = wait_op(&op, LIGHT_TIMEOUT, "按 UUID 获取电池服务") {
            if let Ok(status) = res.Status() {
                if status == GattCommunicationStatus::Success {
                    if let Ok(services) = res.Services() {
                        if services.Size().unwrap_or(0) > 0 {
                            if let Ok(svc) = services.GetAt(0) {
                                return Ok(svc);
                            }
                        }
                    }
                }
            }
        }
    }

    let op = device
        .GetGattServicesWithCacheModeAsync(mode)
        .map_err(|e| BleError::Windows(e.to_string()))?;
    let services_result = wait_op(&op, OP_TIMEOUT, "获取 GATT 服务列表查找电池")?;
    let status = services_result
        .Status()
        .map_err(|e| BleError::Windows(e.to_string()))?;
    if status == GattCommunicationStatus::Success {
        let services = services_result
            .Services()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        for service in &services {
            if let Ok(uuid) = service.Uuid() {
                if uuid == BATTERY_SERVICE_GUID {
                    return Ok(service.clone());
                }
            }
        }
    }

    Err(BleError::Windows("设备未提供标准 Battery Service".into()))
}

fn open_battery_service(service: &GattDeviceService) -> Result<()> {
    for mode in [
        GattSharingMode::SharedReadOnly,
        GattSharingMode::SharedReadAndWrite,
        GattSharingMode::Unspecified,
    ] {
        let op = match service.OpenAsync(mode) {
            Ok(op) => op,
            Err(_) => continue,
        };
        match wait_op(&op, LIGHT_TIMEOUT, "打开电池服务") {
            Ok(s) if s == GattOpenStatus::Success || s == GattOpenStatus::AlreadyOpened => {
                core_log::log_debug(&format!("[ble/gatt] 电池服务打开（{mode:?}）-> {s:?}"));
                return Ok(());
            }
            _ => continue,
        }
    }
    Err(BleError::Windows("打开电池服务失败".into()))
}

fn find_battery_characteristic(
    service: &GattDeviceService,
    mode: BluetoothCacheMode,
) -> Result<GattCharacteristic> {
    if let Ok(op) = service.GetCharacteristicsForUuidWithCacheModeAsync(BATTERY_LEVEL_GUID, mode) {
        if let Ok(res) = wait_op(&op, LIGHT_TIMEOUT, "按 UUID 获取电池电量特征") {
            if let Ok(status) = res.Status() {
                if status == GattCommunicationStatus::Success {
                    if let Ok(chars) = res.Characteristics() {
                        if chars.Size().unwrap_or(0) > 0 {
                            if let Ok(ch) = chars.GetAt(0) {
                                return Ok(ch);
                            }
                        }
                    }
                }
            }
        }
    }

    let op = service
        .GetCharacteristicsWithCacheModeAsync(mode)
        .map_err(|e| BleError::Windows(e.to_string()))?;
    let char_result = wait_op(&op, LIGHT_TIMEOUT, "获取特征列表查找电池特征")?;
    let status = char_result
        .Status()
        .map_err(|e| BleError::Windows(e.to_string()))?;
    if status == GattCommunicationStatus::Success {
        let chars = char_result
            .Characteristics()
            .map_err(|e| BleError::Windows(e.to_string()))?;
        for ch in &chars {
            if let Ok(uuid) = ch.Uuid() {
                if uuid == BATTERY_LEVEL_GUID {
                    return Ok(ch.clone());
                }
            }
        }
    }

    Err(BleError::Windows("未找到 Battery Level 特征".into()))
}

fn read_characteristic_byte(
    characteristic: &GattCharacteristic,
    mode: BluetoothCacheMode,
) -> Result<u8> {
    let op = characteristic
        .ReadValueWithCacheModeAsync(mode)
        .map_err(|e| BleError::Windows(e.to_string()))?;
    let res = wait_op(&op, LIGHT_TIMEOUT, "读取电池特征值")?;
    let status = res.Status().map_err(|e| BleError::Windows(e.to_string()))?;
    if status != GattCommunicationStatus::Success {
        return Err(BleError::Windows(format!("ReadValue status: {status:?}")));
    }
    let buf = res.Value().map_err(|e| BleError::Windows(e.to_string()))?;
    let bytes = buffer_to_vec(&buf)?;
    parse_battery_level(&bytes).ok_or_else(|| BleError::Windows("电池特征返回空数据".into()))
}

fn read_battery_level_from_char(characteristic: &GattCharacteristic) -> Option<u8> {
    for mode in [BluetoothCacheMode::Uncached, BluetoothCacheMode::Cached] {
        if let Ok(val) = read_characteristic_byte(characteristic, mode) {
            return Some(val);
        }
    }
    None
}

fn try_open_battery(device: &BluetoothLEDevice) -> Result<(GattDeviceService, GattCharacteristic)> {
    for mode in [BluetoothCacheMode::Cached, BluetoothCacheMode::Uncached] {
        if let Ok(service) = find_battery_service(device, mode) {
            if open_battery_service(&service).is_ok() {
                if let Ok(ch) = find_battery_characteristic(&service, mode) {
                    return Ok((service, ch));
                }
            }
        }
    }
    Err(BleError::Windows("无法打开电池服务或特征".into()))
}

fn try_enable_battery_notifications(characteristic: &GattCharacteristic) -> bool {
    let op = match characteristic.WriteClientCharacteristicConfigurationDescriptorAsync(
        GattClientCharacteristicConfigurationDescriptorValue::Notify,
    ) {
        Ok(op) => op,
        Err(e) => {
            core_log::log_debug(&format!("[ble/gatt] 电池特征写 CCCD Async 失败: {e}"));
            return false;
        }
    };
    match wait_op(&op, LIGHT_TIMEOUT, "写电池特征 CCCD 启用通知") {
        Ok(status) if status == GattCommunicationStatus::Success => {
            core_log::log_debug("[ble/gatt] 电池电量特征已启用通知（CCCD Notify）");
            true
        }
        Ok(status) => {
            core_log::log_debug(&format!("[ble/gatt] 电池电量写 CCCD 返回状态: {status:?}"));
            false
        }
        Err(e) => {
            core_log::log_debug(&format!("[ble/gatt] 电池电量写 CCCD 超时或失败: {e}"));
            false
        }
    }
}

#[allow(clippy::type_complexity)]
fn setup_battery_service(device: &BluetoothLEDevice) -> Result<BatteryServiceSetup> {
    let (service, ch) = try_open_battery(device)?;
    let initial_level = read_battery_level_from_char(&ch);
    if let Some(lvl) = initial_level {
        core_log::log_info(&format!("[ble/gatt] 电池服务已连接，当前电量: {lvl}%"));
    } else {
        core_log::log_debug("[ble/gatt] 电池服务已连接，但首次读取电量失败");
    }

    let level_holder = Arc::new(Mutex::new(initial_level));
    let callbacks_holder: Arc<Mutex<Vec<BatteryCallback>>> = Arc::new(Mutex::new(Vec::new()));

    let mut cookie = None;
    if try_enable_battery_notifications(&ch) {
        let level_cb = level_holder.clone();
        let listeners_cb = callbacks_holder.clone();
        let event_handler: TypedEventHandler<GattCharacteristic, GattValueChangedEventArgs> =
            TypedEventHandler::new(
                move |_sender: windows::core::Ref<GattCharacteristic>,
                      args: windows::core::Ref<GattValueChangedEventArgs>| {
                    let Some(event_args) = args.as_ref() else {
                        return Ok(());
                    };
                    let Ok(buffer) = event_args.CharacteristicValue() else {
                        return Ok(());
                    };
                    if let Ok(data) = buffer_to_vec(&buffer) {
                        if let Some(level) = parse_battery_level(&data) {
                            core_log::log_info(&format!(
                                "[ble/gatt] 收到遥控器电池电量变更通知: {level}%"
                            ));
                            if let Ok(mut g) = level_cb.lock() {
                                *g = Some(level);
                            }
                            if let Ok(listeners) = listeners_cb.lock() {
                                for cb in listeners.iter() {
                                    cb(level);
                                }
                            }
                        }
                    }
                    Ok(())
                },
            );
        match ch.ValueChanged(&event_handler) {
            Ok(c) => {
                core_log::log_debug(&format!(
                    "[ble/gatt] 已注册电池电量 ValueChanged 回调，cookie={c}"
                ));
                cookie = Some(c);
            }
            Err(e) => {
                core_log::log_warn(&format!(
                    "[ble/gatt] 注册电池电量 ValueChanged 回调失败: {e}"
                ));
            }
        }
    }

    Ok(BatteryServiceSetup {
        level: level_holder,
        service: Some(service),
        characteristic: Some(ch),
        cookie,
        callbacks: callbacks_holder,
    })
}

#[derive(Default)]
struct BatteryServiceSetup {
    level: Arc<Mutex<Option<u8>>>,
    service: Option<GattDeviceService>,
    characteristic: Option<GattCharacteristic>,
    cookie: Option<i64>,
    callbacks: Arc<Mutex<Vec<BatteryCallback>>>,
}

/// 独立尝试读取已配对设备的电池电量（0~100）。若设备不可达或未暴露电量则返回 None。
pub fn read_battery_level(device_id: &str) -> Option<u8> {
    let device = match open_ble_device(device_id) {
        Ok(d) => d,
        Err(e) => {
            core_log::log_debug(&format!("[ble/gatt] 独立读取电量时打开设备失败: {e}"));
            return None;
        }
    };
    let (service, ch) = match try_open_battery(&device) {
        Ok(res) => res,
        Err(e) => {
            core_log::log_debug(&format!("[ble/gatt] 独立读取电量时打开电池服务失败: {e}"));
            return None;
        }
    };
    let level = read_battery_level_from_char(&ch);
    let _ = service.Close();
    if let Some(lvl) = level {
        core_log::log_info(&format!("[ble/gatt] 独立读取到设备电量: {lvl}%"));
    }
    level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_battery_level_works() {
        assert_eq!(parse_battery_level(&[]), None);
        assert_eq!(parse_battery_level(&[0]), Some(0));
        assert_eq!(parse_battery_level(&[85]), Some(85));
        assert_eq!(parse_battery_level(&[100]), Some(100));
        assert_eq!(parse_battery_level(&[255]), Some(100));
    }

    #[test]
    fn battery_uuids_match_specification() {
        assert_eq!(
            BATTERY_SERVICE_GUID,
            GUID::from_u128(0x0000180F_0000_1000_8000_00805F9B34FB)
        );
        assert_eq!(
            BATTERY_LEVEL_GUID,
            GUID::from_u128(0x00002A19_0000_1000_8000_00805F9B34FB)
        );
    }
}
