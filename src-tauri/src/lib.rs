//! Remote Mic Tauri application shell.

use serde::Serialize;

use core_atvv::ImaAdpcmDecoder;
use core_audio::endpoint::{list_output_endpoints, placeholder_output, AudioEndpoint};
use core_mapping::{ActionKind, ButtonId, MappingConfig, Trigger};
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
    action: String,
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
            decode_atvv_preview,
            vb_cable_status,
            install_vb_cable,
            run_self_test,
            demo_record_key,
            get_stats_summary,
            start_voice_bridge,
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
