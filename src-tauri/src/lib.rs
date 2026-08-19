//! Remote Mic Tauri application shell.

use core_ble::placeholder as ble_placeholder;
use core_atvv::placeholder as atvv_placeholder;
use core_audio::placeholder as audio_placeholder;

/// Simple command used to verify the frontend <-> backend bridge works.
#[tauri::command]
fn ping() -> String {
    format!(
        "pong from remote-mic backend (ble={}, atvv={}, audio={})",
        ble_placeholder(),
        atvv_placeholder(),
        audio_placeholder()
    )
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
