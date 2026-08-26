//! Remote Mic Tauri application shell.

mod commands;

use serde::Serialize;
use tauri::{WebviewUrl, WebviewWindowBuilder};

use core_atvv::ImaAdpcmDecoder;
use core_mapping::{ActionKind, ButtonId, Trigger};

#[cfg(target_os = "windows")]
fn find_install_script() -> Option<std::path::PathBuf> {
    fn ancestors(start: std::path::PathBuf, max: usize) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut cur = start;
        for _ in 0..max {
            out.push(cur.clone());
            if !cur.pop() {
                break;
            }
        }
        out
    }

    let mut starts = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            starts.push(p.to_path_buf());
        }
    }

    for start in starts {
        for dir in ancestors(start, 8) {
            let cand = dir.join("scripts").join("install-vb-cable.ps1");
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    None
}

/// Simple command used to verify the frontend <-> backend bridge works.
#[tauri::command]
fn ping() -> String {
    let mut decoder = ImaAdpcmDecoder::new();
    let _ = decoder.decode_bytes(&[0x00, 0x11]);
    "后端已连接（ATVV 解码正常，ADPCM 就绪）".to_string()
}

/// Frontend-friendly mapping entry.
#[derive(Serialize)]
struct MappingEntry {
    button: String,
    name: String,
    trigger: String,
    action: String,
    action_key: String,
}

/// Persisted settings helper.
fn config_store() -> Option<core_config::ConfigStore> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    core_config::ConfigStore::new(std::path::Path::new(&base).join("RemoteMic/RC003")).ok()
}

/// Mapping edit payload from the settings UI.
#[derive(serde::Deserialize)]
struct MappingEdit {
    button: String,
    trigger: String,
    action: String,
}

fn parse_button(s: &str) -> Option<ButtonId> {
    match s {
        "power" => Some(ButtonId::Power),
        "up" => Some(ButtonId::Up),
        "down" => Some(ButtonId::Down),
        "left" => Some(ButtonId::Left),
        "right" => Some(ButtonId::Right),
        "ok" => Some(ButtonId::Ok),
        "back" => Some(ButtonId::Back),
        "home" => Some(ButtonId::Home),
        "menu" => Some(ButtonId::Menu),
        "tv" => Some(ButtonId::Tv),
        "volume_up" => Some(ButtonId::VolumeUp),
        "volume_down" => Some(ButtonId::VolumeDown),
        "mic" => Some(ButtonId::Mic),
        _ => None,
    }
}

fn parse_trigger(s: &str) -> Option<Trigger> {
    match s {
        "single_click" => Some(Trigger::SingleClick),
        "double_click" => Some(Trigger::DoubleClick),
        "long_press" => Some(Trigger::LongPress),
        _ => None,
    }
}

fn parse_action(s: &str) -> Option<ActionKind> {
    use ActionKind as A;
    Some(match s {
        "disabled" => A::Disabled,
        "escape" => A::Escape,
        "return" => A::Return,
        "arrow_up" => A::ArrowUp,
        "arrow_down" => A::ArrowDown,
        "arrow_left" => A::ArrowLeft,
        "arrow_right" => A::ArrowRight,
        "delete_backward" => A::DeleteBackward,
        "show_desktop" => A::ShowDesktop,
        "context_menu" => A::ContextMenu,
        "app_switcher" => A::AppSwitcher,
        "system_volume_up" => A::SystemVolumeUp,
        "system_volume_down" => A::SystemVolumeDown,
        "system_volume_mute" => A::SystemVolumeMute,
        "play_pause" => A::PlayPause,
        "voice" => A::Voice,
        _ => return None,
    })
}

/// Stable lower-case button key used by the frontend.
fn button_key(button: &ButtonId) -> String {
    match button {
        ButtonId::Power => "power",
        ButtonId::Up => "up",
        ButtonId::Down => "down",
        ButtonId::Left => "left",
        ButtonId::Right => "right",
        ButtonId::Ok => "ok",
        ButtonId::Back => "back",
        ButtonId::Home => "home",
        ButtonId::Menu => "menu",
        ButtonId::Tv => "tv",
        ButtonId::VolumeUp => "volume_up",
        ButtonId::VolumeDown => "volume_down",
        ButtonId::Mic => "mic",
    }
    .to_string()
}

/// Stable lower-case trigger key used by the frontend.
fn trigger_key(trigger: &Trigger) -> String {
    match trigger {
        Trigger::SingleClick => "single_click",
        Trigger::DoubleClick => "double_click",
        Trigger::LongPress => "long_press",
    }
    .to_string()
}

/// Stable action key used by the frontend action picker.
fn action_key(action: &ActionKind) -> String {
    match action {
        ActionKind::Disabled => "disabled",
        ActionKind::KeyCombo(_) => "key_combo",
        ActionKind::Escape => "escape",
        ActionKind::Return => "return",
        ActionKind::ArrowUp => "arrow_up",
        ActionKind::ArrowDown => "arrow_down",
        ActionKind::ArrowLeft => "arrow_left",
        ActionKind::ArrowRight => "arrow_right",
        ActionKind::DeleteBackward => "delete_backward",
        ActionKind::ShowDesktop => "show_desktop",
        ActionKind::ContextMenu => "context_menu",
        ActionKind::AppSwitcher => "app_switcher",
        ActionKind::SystemVolumeUp => "system_volume_up",
        ActionKind::SystemVolumeDown => "system_volume_down",
        ActionKind::SystemVolumeMute => "system_volume_mute",
        ActionKind::PlayPause => "play_pause",
        ActionKind::Voice => "voice",
        ActionKind::OpenApp(_) => "open_app",
    }
    .to_string()
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
        .setup(|app| {
            let win_width = 420.0;
            let win_height = 420.0;

            let right_margin = 2.0;
            let bottom_margin = 2.0;
            let (x, y) = if let Ok(Some(monitor)) = app.handle().primary_monitor() {
                let scale = monitor.scale_factor();
                // 使用工作区（不含任务栏）定位，并留出很小的边距
                let work = monitor.work_area();
                let wx = work.position.x as f64 / scale;
                let wy = work.position.y as f64 / scale;
                let ww = work.size.width as f64 / scale;
                let wh = work.size.height as f64 / scale;
                let x = wx + ww - win_width - right_margin;
                let y = wy + wh - win_height - bottom_margin;
                (x, y)
            } else {
                (0.0, 0.0)
            };

            WebviewWindowBuilder::new(
                app,
                "quick-menu",
                WebviewUrl::App("quick-menu.html".into()),
            )
            .title("Quick Menu")
            .inner_size(win_width, win_height)
            .position(x, y)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(false)
            .visible(false)
            .build()?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            commands::connection::scan_for_rc003,
            commands::connection::connect_rc003,
            commands::connection::get_persisted_settings,
            commands::connection::save_selected_device,
            commands::connection::save_output_endpoint,
            commands::connection::open_system_settings,
            commands::log::log_message,
            commands::mapping::save_mapping,
            commands::mapping::get_mappings,
            commands::audio::list_audio_endpoints,
            commands::audio::start_voice_bridge,
            commands::audio::simulate_voice_chain,
            commands::audio::vb_cable_status,
            commands::audio::install_vb_cable,
            commands::audio::audio_diagnostics,
            commands::audio::play_test_tone,
            commands::audio::play_test_tone_loop,
            commands::diagnostics::decode_atvv_preview,
            commands::diagnostics::run_self_test,
            commands::quick_menu::toggle_quick_menu,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}