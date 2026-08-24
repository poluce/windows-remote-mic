//! Remote Mic Tauri application shell.

use serde::Serialize;

use core_atvv::ImaAdpcmDecoder;
use core_audio::endpoint::{list_output_endpoints, placeholder_output, AudioEndpoint};
use core_mapping::{ActionKind, ButtonId, MappingConfig, Trigger};

/// Simple command used to verify the frontend <-> backend bridge works.
#[tauri::command]
fn ping() -> String {
    let mut decoder = ImaAdpcmDecoder::new();
    let _ = decoder.decode_bytes(&[0x00, 0x11]);
    "后端已连接（ATVV 解码正常，ADPCM 就绪）".to_string()
}

/// List output endpoints exposed to the settings UI.
#[tauri::command]
fn list_audio_endpoints() -> Vec<AudioEndpoint> {
    match list_output_endpoints() {
        Ok(list) if !list.is_empty() => list,
        _ => vec![placeholder_output()],
    }
}

/// Frontend-friendly mapping entry.
#[derive(Serialize)]
struct MappingEntry {
    button: String,
    name: String,
    action: String,
}

/// Scan for the RC003 remote over Bluetooth LE (Windows only).
#[tauri::command]
fn scan_for_rc003() -> Result<core_ble::BleDevice, String> {
    core_ble::scan_for_rc003().map_err(|e| e.to_string())
}

/// Connect to the RC003 and report whether the ATVV service is present.
#[derive(serde::Serialize)]
struct Rc003Connection {
    device: core_ble::BleDevice,
    endpoints: core_ble::gatt::AtvvEndpoints,
}

#[tauri::command]
fn connect_rc003() -> Result<Rc003Connection, String> {
    core_ble::scan_and_connect()
        .map(|(device, endpoints)| Rc003Connection { device, endpoints })
        .map_err(|e| e.to_string())
}

/// Run audio diagnostics: endpoints + VB-CABLE presence.
#[tauri::command]
fn audio_diagnostics() -> core_audio::diagnostics::AudioDiagnostics {
    core_audio::diagnostics::run()
}

/// Loop the test tone several times into the selected endpoint.
#[tauri::command]
fn play_test_tone_loop(device_name: Option<String>, repetitions: Option<u32>) -> String {
    let reps = repetitions.unwrap_or(3).clamp(1, 10);
    #[cfg(target_os = "windows")]
    {
        match core_audio::playback::play_test_tone_loop(device_name.as_deref(), reps, 500) {
            Ok(()) => format!("已循环播放 {reps} 次"),
            Err(e) => format!("播放失败: {e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = device_name;
        let _ = reps;
        "测试音循环仅在 Windows 可用".to_string()
    }
}

/// Play a 1 s test tone into the selected output endpoint (fuzzy name match).
#[tauri::command]
fn play_test_tone(device_name: Option<String>) -> String {
    #[cfg(target_os = "windows")]
    {
        match core_audio::playback::play_test_tone(device_name.as_deref()) {
            Ok(()) => "测试音已播放".to_string(),
            Err(e) => format!("测试音播放失败: {e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = device_name;
        "测试音播放仅在 Windows 可用".to_string()
    }
}

/// Return the default 13-key single-click mapping.
#[tauri::command]
fn default_mapping() -> Vec<MappingEntry> {
    let cfg = MappingConfig::default();
    cfg.bindings
        .iter()
        .filter(|b| b.trigger == Trigger::SingleClick)
        .map(|b| MappingEntry {
            button: serde_plain(&b.button),
            name: b.button.display_name().to_string(),
            action: action_label(&b.action),
        })
        .collect()
}

/// Serialize a button id into its stable string form.
fn serde_plain(button: &ButtonId) -> String {
    // Uses the serde serialization of the enum (lowercase variants).
    serde_json::to_value(button)
        .map(|v| v.as_str().unwrap_or_default().to_string())
        .unwrap_or_default()
}

fn action_label(action: &ActionKind) -> String {
    match action {
        ActionKind::Disabled => "禁用".into(),
        ActionKind::KeyCombo(keys) => keys.join("+"),
        ActionKind::Escape => "取消（Esc）".into(),
        ActionKind::Return => "回车（Enter）".into(),
        ActionKind::ArrowUp => "↑".into(),
        ActionKind::ArrowDown => "↓".into(),
        ActionKind::ArrowLeft => "←".into(),
        ActionKind::ArrowRight => "→".into(),
        ActionKind::DeleteBackward => "删除（退格）".into(),
        ActionKind::ShowDesktop => "显示桌面（Win+D）".into(),
        ActionKind::ContextMenu => "右键菜单（上下文菜单）".into(),
        ActionKind::AppSwitcher => "切换应用（Alt+Tab）".into(),
        ActionKind::SystemVolumeUp => "音量 +".into(),
        ActionKind::SystemVolumeDown => "音量 −".into(),
        ActionKind::SystemVolumeMute => "静音".into(),
        ActionKind::PlayPause => "播放/暂停".into(),
        ActionKind::Voice => "语音输入（Win+H）".into(),
        ActionKind::OpenApp(name) => format!("打开应用：{name}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ping,
            scan_for_rc003,
            connect_rc003,
            list_audio_endpoints,
            default_mapping,
            play_test_tone,
            play_test_tone_loop,
            audio_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
