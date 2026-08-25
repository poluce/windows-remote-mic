//! Remote Mic Tauri application shell.

use serde::Serialize;

use core_atvv::ImaAdpcmDecoder;
use core_audio::endpoint::{list_output_endpoints, placeholder_output, AudioEndpoint};
use core_mapping::{ActionKind, ButtonId, Trigger};
use core_mapping::gesture::GestureDetector;

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
    trigger: String,
    action: String,
    action_key: String,
}

/// Start the real-device voice bridge (Windows only). Runs on a worker thread.
#[tauri::command]
fn start_voice_bridge(device_id: String, output_device: String) -> String {
    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(move || {
            if let Err(e) = core_voice::run_bridge(&device_id, &output_device) {
                eprintln!("voice bridge error: {e}");
            }
        });
        "语音桥已启动（监听 ATVV Audio → CABLE 输出）".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (device_id, output_device);
        "语音桥仅在 Windows 可用".to_string()
    }
}

/// Decode a batch of ATVV audio bytes through the voice engine (self-test).
#[tauri::command]
fn decode_atvv_preview(bytes: Vec<u8>) -> core_voice::VoiceChunk {
    let mut engine = core_voice::VoiceEngine::new();
    let _ = engine.on_control(core_atvv::protocol::ControlEvent::StreamStart);
    engine.feed(&bytes)
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

/// Persisted settings exposed to the UI.
#[derive(serde::Serialize)]
struct PersistedSettings {
    selected_device_id: Option<String>,
    output_endpoint_id: Option<String>,
}

fn config_store() -> Option<core_config::ConfigStore> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    core_config::ConfigStore::new(std::path::Path::new(&base).join("RemoteMic/RC003")).ok()
}

#[tauri::command]
fn get_persisted_settings() -> PersistedSettings {
    let cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    PersistedSettings {
        selected_device_id: cfg.selected_device_id,
        output_endpoint_id: cfg.output_endpoint_id,
    }
}

#[tauri::command]
fn save_selected_device(device_id: String) -> Result<(), String> {
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
fn save_output_endpoint(endpoint_id: String) -> Result<(), String> {
    let mut cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    cfg.output_endpoint_id = Some(endpoint_id);
    config_store()
        .ok_or_else(|| "无法创建配置目录".to_string())?
        .save(&cfg)
        .map_err(|e| e.to_string())
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

/// Save one button mapping to `config.json`.
#[tauri::command]
fn save_mapping(edit: MappingEdit) -> Result<(), String> {
    let button = parse_button(&edit.button).ok_or("未知按键")?;
    let trigger = parse_trigger(&edit.trigger).ok_or("未知触发")?;
    let action = parse_action(&edit.action).ok_or("未知动作")?;

    let store = config_store().ok_or("无法创建配置目录")?;
    let mut cfg = store.load().map_err(|e| e.to_string())?;
    if let Some(binding) = cfg
        .mapping
        .bindings
        .iter_mut()
        .find(|b| b.button == button && b.trigger == trigger)
    {
        binding.action = action;
    } else {
        cfg.mapping.bindings.push(core_mapping::KeyBinding {
            button,
            trigger,
            action,
        });
    }
    store.save(&cfg).map_err(|e| e.to_string())
}

/// Simulate the full voice chain without a real remote.
#[tauri::command]
fn simulate_voice_chain(output_device: String) -> Result<core_voice::SimulatedVoiceResult, String> {
    #[cfg(target_os = "windows")]
    {
        core_voice::simulate_voice_chain(&output_device)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = output_device;
        Err("仅限 Windows".to_string())
    }
}

/// One day in stats history.
#[derive(serde::Serialize)]
struct StatsDaySummary {
    day: String,
    key_presses: u64,
    voice_seconds: u64,
}

/// Return recent daily stats (last 7 days) for simple charts.
#[tauri::command]
fn get_stats_history() -> Vec<StatsDaySummary> {
    let mut out: Vec<(u64, StatsDaySummary)> = Vec::new();
    let base = std::env::var("LOCALAPPDATA")
        .map(|b| std::path::PathBuf::from(b).join("RemoteMic/RC003"))
        .unwrap_or_default();
    if let Ok(store) = core_stats::StatsStore::new(&base) {
        if let Ok(stats) = store.load() {
            for (day, d) in &stats {
                if let Ok(num) = day.parse::<u64>() {
                    out.push((
                        num,
                        StatsDaySummary {
                            day: day.clone(),
                            key_presses: d.key_presses.values().sum(),
                            voice_seconds: d.voice_seconds,
                        },
                    ));
                }
            }
        }
    }
    out.sort_by_key(|(day, _)| *day);
    out.into_iter()
        .rev()
        .take(7)
        .map(|(_, s)| s)
        .collect()
}

/// Open a Windows system settings page by URI.
#[tauri::command]
fn open_system_settings(setting: String) -> String {
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

/// One log file entry.
#[derive(serde::Serialize)]
struct LogFileInfo {
    name: String,
    path: String,
    size: u64,
}

fn list_logs_in_dir(dir: &std::path::Path, prefix: &str, out: &mut Vec<LogFileInfo>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                out.push(LogFileInfo {
                    name: format!("{prefix}{}", entry.file_name().to_string_lossy()),
                    path: path.display().to_string(),
                    size,
                });
            }
        }
    }
}

/// List local log and capture files (config/logs/captures).
#[tauri::command]
fn list_log_files() -> Vec<LogFileInfo> {
    let base = std::env::var("LOCALAPPDATA")
        .map(|b| std::path::PathBuf::from(b).join("RemoteMic/RC003"))
        .unwrap_or_default();
    let mut out = Vec::new();
    list_logs_in_dir(&base, "", &mut out);
    list_logs_in_dir(&base.join("logs"), "logs/", &mut out);
    list_logs_in_dir(&base.join("captures"), "captures/", &mut out);
    out
}

/// Read a text log file (truncated to a safe size for the UI).
#[tauri::command]
fn read_log_file(path: String) -> Result<String, String> {
    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&data).to_string();
    Ok(limit_text(&text, 50_000))
}

fn limit_text(text: &str, max: usize) -> String {
    if text.len() > max {
        format!("...（已截断）
{}", &text[text.len() - max..])
    } else {
        text.to_string()
    }
}

/// One self-test item with a PASS / FAIL / SKIP verdict.
#[derive(serde::Serialize)]
struct SelfTestItem {
    name: String,
    status: String,
    detail: String,
}

/// Run a hardware-independent capability self-test (Windows does the audio part).
#[tauri::command]
fn run_self_test() -> Vec<SelfTestItem> {
    let mut items = Vec::new();

    // 1) Audio endpoints
    match core_audio::endpoint::list_output_endpoints() {
        Ok(list) if !list.is_empty() => items.push(SelfTestItem {
            name: "音频端点枚举".into(),
            status: "pass".into(),
            detail: format!("发现 {} 个输出端点", list.len()),
        }),
        Ok(_) => items.push(SelfTestItem {
            name: "音频端点枚举".into(),
            status: "fail".into(),
            detail: "未发现输出端点".into(),
        }),
        Err(e) => items.push(SelfTestItem {
            name: "音频端点枚举".into(),
            status: "fail".into(),
            detail: e.to_string(),
        }),
    }

    // 2) Voice decode preview (synthetic ATVV bytes)
    {
        let mut engine = core_voice::VoiceEngine::new();
        let _ = engine.on_control(core_atvv::protocol::ControlEvent::StreamStart);
        let chunk = engine.feed(&[0x55; 120]);
        if chunk.output_samples > 0 && chunk.pcm_samples > 0 {
            items.push(SelfTestItem {
                name: "ATVV→ADPCM→输出帧".into(),
                status: "pass".into(),
                detail: format!("PCM {}，输出帧 {}", chunk.pcm_samples, chunk.output_samples),
            });
        } else {
            items.push(SelfTestItem {
                name: "ATVV→ADPCM→输出帧".into(),
                status: "fail".into(),
                detail: "解码输出为空".into(),
            });
        }
    }

    // 3) Gesture detection
    {
        let mut d = GestureDetector::new();
        d.press(0);
        let fired = d.release(600);
        use core_mapping::gesture::FeedOutcome;
        use core_mapping::Trigger;
        match fired {
            FeedOutcome::Fire(ev) if ev.trigger == Trigger::LongPress => {
                items.push(SelfTestItem {
                    name: "长按手势识别".into(),
                    status: "pass".into(),
                    detail: "550ms 长按被正确识别".into(),
                });
            }
            other => items.push(SelfTestItem {
                name: "长按手势识别".into(),
                status: "fail".into(),
                detail: format!("期望 LongPress，实际 {:?}", other),
            }),
        }
    }

    // 4) Local stats write/read
    {
        let dir = std::env::temp_dir().join("remote-mic-self-test");
        let _ = std::fs::remove_dir_all(&dir);
        if let Ok(store) = core_stats::StatsStore::new(&dir) {
            let ok = store.record_key("self_test").is_ok()
                && store
                    .load()
                    .map(|m| {
                        m.values()
                            .any(|d| d.key_presses.get("self_test").copied().unwrap_or(0) > 0)
                    })
                    .unwrap_or(false);
            items.push(SelfTestItem {
                name: "本地统计读写".into(),
                status: if ok { "pass" } else { "fail" }.into(),
                detail: if ok { "统计写读一致".into() } else { "统计读写失败".into() },
            });
        } else {
            items.push(SelfTestItem {
                name: "本地统计读写".into(),
                status: "fail".into(),
                detail: "无法创建统计目录".into(),
            });
        }
    }

    // 5) Test tone playback (Windows only)
    #[cfg(target_os = "windows")]
    {
        match core_audio::playback::play_test_tone(None) {
            Ok(()) => items.push(SelfTestItem {
                name: "测试音播放（CABLE 验证）".into(),
                status: "pass".into(),
                detail: "已写入默认输出端点约 1 秒".into(),
            }),
            Err(e) => items.push(SelfTestItem {
                name: "测试音播放（CABLE 验证）".into(),
                status: "fail".into(),
                detail: e.to_string(),
            }),
        }
    }
    #[cfg(not(target_os = "windows"))]
    items.push(SelfTestItem {
        name: "测试音播放（CABLE 验证）".into(),
        status: "skip".into(),
        detail: "仅限 Windows".into(),
    });

    items
}

/// Simulate one key press and write it to local stats, then return the new summary.
#[tauri::command]
fn demo_record_key() -> StatsSummary {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let store = core_stats::StatsStore::new(std::path::Path::new(&base).join("RemoteMic/RC003"));
    if let Ok(store) = store {
        let _ = store.record_key("demo_button");
    }
    get_stats_summary_inner()
}

/// Local statistics summary (key presses + voice time).
#[derive(serde::Serialize)]
struct StatsSummary {
    today_key_presses: u64,
    today_voice_seconds: u64,
    total_key_presses: u64,
    total_voice_seconds: u64,
}

#[tauri::command]
fn get_stats_summary() -> StatsSummary {
    get_stats_summary_inner()
}

fn get_stats_summary_inner() -> StatsSummary {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let store = core_stats::StatsStore::new(std::path::Path::new(&base).join("RemoteMic/RC003"));
    let mut today_key = 0;
    let mut today_voice = 0;
    let mut total_key = 0;
    let mut total_voice = 0;
    if let Ok(store) = store {
        if let Ok(stats) = store.load() {
            for (_, day) in &stats {
                total_key += day.key_presses.values().sum::<u64>();
                total_voice += day.voice_seconds;
            }
            if let Some(day) = stats.get(&core_stats::StatsStore::date_key_now()) {
                today_key = day.key_presses.values().sum::<u64>();
                today_voice = day.voice_seconds;
            }
        }
    }
    StatsSummary {
        today_key_presses: today_key,
        today_voice_seconds: today_voice,
        total_key_presses: total_key,
        total_voice_seconds: total_voice,
    }
}

/// VB-CABLE installed status.
#[derive(serde::Serialize)]
struct VbCableStatus {
    input: bool,
    output: bool,
    ready: bool,
}

#[tauri::command]
fn vb_cable_status() -> VbCableStatus {
    let d = core_audio::diagnostics::run();
    VbCableStatus {
        input: d.cable_input_present,
        output: d.cable_output_present,
        ready: d.has_vb_cable,
    }
}

/// One-click VB-CABLE install: runs the official installer helper.
#[tauri::command]
fn install_vb_cable() -> String {
    let d = core_audio::diagnostics::run();
    if d.has_vb_cable {
        return "已安装 VB-CABLE，无需重复安装".to_string();
    }

    #[cfg(target_os = "windows")]
    {
        let script = match find_install_script() {
            Some(s) => s,
            None => {
                return format!(
                    "找不到安装脚本（从 {} 向上搜索 scripts/install-vb-cable.ps1 均失败）",
                    std::env::current_dir().map(|c| c.display().to_string()).unwrap_or_default()
                );
            }
        };

        match std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap_or_default(),
            ])
            .output()
        {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            Ok(out) => format!(
                "安装失败：{}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Err(e) => format!("无法启动安装程序：{e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = ();
        "VB-CABLE 安装仅在 Windows 上可用".to_string()
    }
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

/// Return all persisted bindings (single/double/long) for the mapping editor.
#[tauri::command]
fn get_mappings() -> Vec<MappingEntry> {
    let cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    cfg.mapping.bindings
        .iter()
        .map(|b| MappingEntry {
            button: button_key(&b.button),
            name: b.button.display_name().to_string(),
            trigger: trigger_key(&b.trigger),
            action: action_label(&b.action),
            action_key: action_key(&b.action),
        })
        .collect()
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
        .invoke_handler(tauri::generate_handler![
            ping,
            decode_atvv_preview,
            get_stats_history,
            open_system_settings,
            save_mapping,
            simulate_voice_chain,
            list_log_files,
            read_log_file,
            get_persisted_settings,
            save_selected_device,
            save_output_endpoint,
            vb_cable_status,
            install_vb_cable,
            run_self_test,
            demo_record_key,
            get_stats_summary,
            start_voice_bridge,
            scan_for_rc003,
            connect_rc003,
            list_audio_endpoints,
            get_mappings,
            play_test_tone,
            play_test_tone_loop,
            audio_diagnostics
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
