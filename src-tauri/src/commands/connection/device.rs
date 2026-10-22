//! RC003 扫描 / 连接，以及「上次成功连接的设备」记忆。

use serde::Serialize;

use crate::config_store;

/// 连接 RC003。
#[derive(Serialize)]
pub struct Rc003Connection {
    pub device: core_ble::BleDevice,
    pub endpoints: core_ble::gatt::AtvvEndpoints,
    /// 当前 BLE 链路是否真的已连接。
    ///
    /// `scan_and_connect` 可能命中 WinRT 的 GATT 缓存，即使链路断开也能发现端点；
    /// 前端不能用「发现成功」推断「已连接」，必须以该字段为准。
    pub link_connected: bool,
}

#[tauri::command]
pub async fn scan_for_rc003() -> Result<core_ble::BleDevice, String> {
    core_log::log_info("[commands/connection] 前端请求扫描 RC003");
    tauri::async_runtime::spawn_blocking(|| match core_ble::scan_for_rc003() {
        Ok(device) => {
            core_log::log_info(&format!(
                "[commands/connection] 扫描 RC003 成功：名称='{}'，ID='{}'",
                device.name, device.id
            ));
            Ok(device)
        }
        Err(e) => {
            core_log::log_error(&format!("[commands/connection] 扫描 RC003 失败: {e}"));
            Err(e.to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn connect_rc003() -> Result<Rc003Connection, String> {
    core_log::log_info("[commands/connection] 前端请求连接 RC003");
    tauri::async_runtime::spawn_blocking(|| {
        match core_ble::scan_and_connect() {
            Ok((device, endpoints)) => {
                core_log::log_info(&format!(
                    "[commands/connection] 连接 RC003 成功：'{}' ({}) -> ATVV: tx={:?}, audio={:?}, control={:?}",
                    device.name, device.id, endpoints.tx, endpoints.audio, endpoints.control
                ));
                // 发现即就绪：把「ATVV 端点是否发现」这一事实同步给语音桥状态机，
                // 避免要等到会话建立后才更新（此前 UI 会短暂/持续显示「端点未就绪」）。
                core_voice::set_atvv_endpoints_ready(endpoints.is_complete());
                let link_connected = core_ble::is_link_connected(&device.id);
                core_log::log_info(&format!(
                    "[commands/connection] 连接结果：link_connected={link_connected}"
                ));
                Ok(Rc003Connection {
                    device,
                    endpoints,
                    link_connected,
                })
            }
            Err(e) => {
                core_log::log_error(&format!("[commands/connection] 连接 RC003 失败: {e}"));
                Err(e.to_string())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn save_selected_device(device_id: String) -> Result<(), String> {
    let mut cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    cfg.selected_device_id = Some(device_id);
    config_store()
        .ok_or_else(|| "无法创建配置目录".to_string())?
        .save(&cfg)
        .map_err(|e| e.to_string())
}

/// 读取上次成功连接过的设备 ID（供连接页启动时自动重连一次）。
#[tauri::command]
pub fn get_saved_device_id() -> Option<String> {
    config_store()
        .and_then(|s| s.load().ok())
        .and_then(|cfg| cfg.selected_device_id)
}
