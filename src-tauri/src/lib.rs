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
    format!(
        "pong from remote-mic backend (atvv samples decoded ok, adpcm ready)"
    )
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
        ActionKind::Escape => "Esc".into(),
        ActionKind::Return => "Enter".into(),
        ActionKind::ArrowUp => "↑".into(),
        ActionKind::ArrowDown => "↓".into(),
        ActionKind::ArrowLeft => "←".into(),
        ActionKind::ArrowRight => "→".into(),
        ActionKind::DeleteBackward => "Backspace".into(),
        ActionKind::ShowDesktop => "显示桌面 Win+D".into(),
        ActionKind::ContextMenu => "上下文菜单".into(),
        ActionKind::AppSwitcher => "应用切换 Alt+Tab".into(),
        ActionKind::SystemVolumeUp => "音量 +".into(),
        ActionKind::SystemVolumeDown => "音量 −".into(),
        ActionKind::SystemVolumeMute => "静音".into(),
        ActionKind::PlayPause => "播放/暂停".into(),
        ActionKind::Voice => "语音输入 Win+H".into(),
        ActionKind::OpenApp(name) => format!("打开 {name}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            ping,
            list_audio_endpoints,
            default_mapping
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
