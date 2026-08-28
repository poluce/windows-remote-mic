use serde::Serialize;

use crate::config_store;

/// 连接页需要恢复的运行时状态快照。
#[derive(Serialize)]
pub struct RuntimeStatus {
    pub connected: bool,
    pub bridge_running: bool,
    pub tap_status: Option<String>,
    pub endpoints_ready: bool,
}

#[tauri::command]
pub fn get_runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        connected: core_voice::connection_active(),
        bridge_running: core_voice::bridge_running(),
        tap_status: core_hid::tap::last_status(),
        endpoints_ready: core_voice::atvv_endpoints_ready(),
    }
}

/// 连接 RC003。
#[derive(Serialize)]
pub struct Rc003Connection {
    pub device: core_ble::BleDevice,
    pub endpoints: core_ble::gatt::AtvvEndpoints,
}

/// 暴露给界面的持久化设置。
#[derive(Serialize)]
pub struct PersistedSettings {
    pub selected_device_id: Option<String>,
    pub output_endpoint_id: Option<String>,
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
                Ok(Rc003Connection { device, endpoints })
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

/// 返回当前 ATVV/BLE 链路是否已连接。
#[tauri::command]
pub fn get_connection_status() -> bool {
    core_voice::connection_active()
}

#[tauri::command]
pub fn get_persisted_settings() -> PersistedSettings {
    let cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    PersistedSettings {
        selected_device_id: cfg.selected_device_id,
        output_endpoint_id: cfg.output_endpoint_id,
    }
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

#[tauri::command]
pub fn save_output_endpoint(endpoint_id: String) -> Result<(), String> {
    let mut cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    cfg.output_endpoint_id = Some(endpoint_id);
    config_store()
        .ok_or_else(|| "无法创建配置目录".to_string())?
        .save(&cfg)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_system_settings(setting: String) -> String {
    let uri = match setting.as_str() {
        "bluetooth" => "ms-settings:bluetooth",
        "microphone" => "ms-settings:privacy-microphone",
        "sound" => "ms-settings:sound",
        _ => "ms-settings:",
    };
    #[cfg(target_os = "windows")]
    {
        match std::process::Command::new("cmd")
            .args(["/C", "start", "", uri])
            .spawn()
        {
            Ok(_) => "已打开系统设置".to_string(),
            Err(e) => format!("打开失败：{e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = uri;
        "仅限 Windows".to_string()
    }
}
