//! [`crate::ports::VoiceTransport`] 的真实实现：WinRT ATVV GATT 链路。
//!
//! 把 `core-ble::gatt::AtvvLink` 包成端口 trait；桥接层不再直接依赖
//! `core-ble` 的具体类型。

use core_ble::gatt::AtvvLink;

use crate::ports::VoiceTransport;

/// 真实设备传输：连接 RC003 的 ATVV 服务。
pub struct AtvvTransport {
    link: AtvvLink,
}

impl AtvvTransport {
    /// 连接设备的 ATVV 服务（失败把 `BleError` 转成字符串）。
    pub fn connect(device_id: &str) -> Result<Self, String> {
        let link = AtvvLink::connect(device_id).map_err(|e| e.to_string())?;
        Ok(Self { link })
    }
}

impl VoiceTransport for AtvvTransport {
    fn register_connection_handler(
        &self,
        callback: Box<dyn FnMut(bool) + Send + 'static>,
    ) -> Result<i64, String> {
        self.link
            .register_connection_status_changed(callback)
            .map_err(|e| e.to_string())
    }

    fn enable_audio_notifications(&self) -> Result<(), String> {
        self.link
            .enable_audio_notifications()
            .map_err(|e| e.to_string())
    }

    fn enable_control_notifications(&self) -> Result<(), String> {
        self.link
            .enable_control_notifications()
            .map_err(|e| e.to_string())
    }

    fn write_tx(&self, bytes: &[u8]) -> Result<(), String> {
        self.link.write_tx(bytes).map_err(|e| e.to_string())
    }

    fn register_audio_handler(
        &self,
        callback: Box<dyn FnMut(Vec<u8>) + Send + 'static>,
    ) -> Result<i64, String> {
        self.link
            .register_audio_handler(callback)
            .map_err(|e| e.to_string())
    }

    fn register_control_handler(
        &self,
        callback: Box<dyn FnMut(Vec<u8>) + Send + 'static>,
    ) -> Result<i64, String> {
        self.link
            .register_control_handler(callback)
            .map_err(|e| e.to_string())
    }

    fn is_connected(&self) -> bool {
        self.link.is_connected()
    }
}
