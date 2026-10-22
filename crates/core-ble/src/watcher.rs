//! 轻量链路监视器与快速连接检查。
//!
//! **只持有蓝牙设备句柄并监听连接状态**，不打开任何 GATT 服务，
//! 因此不会主动触发连接、也不会和别的电脑抢设备。
//!
//! 用途：语音桥不再「每 N 秒盲试连接」，而是等 Windows 把 HOGP 链路连上后
//! （遥控器回到本机、或本机是当前主机）再按需建立 ATVV 会话。

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use windows::core::IInspectable;
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothLEDevice};
use windows::Foundation::TypedEventHandler;

use super::gatt::open_ble_device;
use crate::{BleError, Result};

/// 检查给定设备当前是否处于连接状态（非阻塞）。
///
/// 用于区分「缓存里的配对/服务信息」与「当前链路是否真的连上」：
/// GATT 缓存发现可以在链路断开时成功，不能据此认为设备已连接。
pub fn is_link_connected(device_id: &str) -> bool {
    match open_ble_device(device_id) {
        Ok(device) => matches!(
            device.ConnectionStatus(),
            Ok(BluetoothConnectionStatus::Connected)
        ),
        Err(_) => false,
    }
}

/// 轻量链路监视器：**只持有蓝牙设备句柄并监听连接状态**，
/// 不打开任何 GATT 服务，因此不会主动触发连接、也不会和别的电脑抢设备。
pub struct LinkWatcher {
    device: BluetoothLEDevice,
    /// 连接状态变化事件（true=已连接）。
    changes: Receiver<bool>,
}

impl LinkWatcher {
    /// 打开设备句柄并开始监听连接状态。
    ///
    /// 只做 `FromIdAsync`，不会建立连接；设备不存在/未配对时返回错误。
    pub fn open(device_id: &str) -> Result<Self> {
        let device = open_ble_device(device_id)?;
        let (tx, changes) = mpsc::channel::<bool>();
        let handler = TypedEventHandler::<BluetoothLEDevice, IInspectable>::new(
            move |sender: windows::core::Ref<BluetoothLEDevice>,
                  _args: windows::core::Ref<IInspectable>| {
                if let Some(dev) = sender.as_ref() {
                    if let Ok(status) = dev.ConnectionStatus() {
                        let _ = tx.send(status == BluetoothConnectionStatus::Connected);
                    }
                }
                Ok(())
            },
        );
        device
            .ConnectionStatusChanged(&handler)
            .map_err(|e| BleError::Windows(format!("监听蓝牙连接状态失败: {e}")))?;
        core_log::log_info("[ble/gatt] 已开始监听遥控器链路状态（不主动连接）");
        Ok(Self { device, changes })
    }

    /// 当前链路是否已连接（直接查询系统状态，不走缓存）。
    pub fn is_connected(&self) -> bool {
        matches!(
            self.device.ConnectionStatus(),
            Ok(BluetoothConnectionStatus::Connected)
        )
    }

    /// 等待下一次连接状态变化。
    ///
    /// 返回 `Some(true)`=已连接、`Some(false)`=已断开、`None`=超时（调用方可借机
    /// 检查停止标志，避免忙等）。
    pub fn wait_change(&self, timeout: Duration) -> Option<bool> {
        self.changes.recv_timeout(timeout).ok()
    }
}
