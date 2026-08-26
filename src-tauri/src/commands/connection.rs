use serde::Serialize;

use crate::config_store;

/// Connect to RC003.
#[derive(Serialize)]
pub struct Rc003Connection {
    pub device: core_ble::BleDevice,
    pub endpoints: core_ble::gatt::AtvvEndpoints,
}

/// Persisted settings exposed to the UI.
#[derive(Serialize)]
pub struct PersistedSettings {
    pub selected_device_id: Option<String>,
    pub output_endpoint_id: Option<String>,
}

#[tauri::command]
pub async fn scan_for_rc003() -> Result<core_ble::BleDevice, String> {
    core_log::log_info("[commands/connection] scan_for_rc003 invoked from UI");
    tauri::async_runtime::spawn_blocking(|| {
        match core_ble::scan_for_rc003() {
            Ok(device) => {
                core_log::log_info(&format!(
                    "[commands/connection] scan_for_rc003 succeeded: name='{}', id='{}'",
                    device.name, device.id
                ));
                Ok(device)
            }
            Err(e) => {
                core_log::log_error(&format!("[commands/connection] scan_for_rc003 failed: {e}"));
                Err(e.to_string())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn connect_rc003() -> Result<Rc003Connection, String> {
    core_log::log_info("[commands/connection] connect_rc003 invoked from UI");
    tauri::async_runtime::spawn_blocking(|| {
        match core_ble::scan_and_connect() {
            Ok((device, endpoints)) => {
                core_log::log_info(&format!(
                    "[commands/connection] connect_rc003 succeeded for '{}' ({}) -> ATVV: tx={:?}, audio={:?}, control={:?}",
                    device.name, device.id, endpoints.tx, endpoints.audio, endpoints.control
                ));
                Ok(Rc003Connection { device, endpoints })
            }
            Err(e) => {
                core_log::log_error(&format!("[commands/connection] connect_rc003 failed: {e}"));
                Err(e.to_string())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
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