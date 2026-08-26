use serde::Serialize;

use core_audio::endpoint::{list_output_endpoints, placeholder_output, AudioEndpoint};

use crate::find_install_script;

#[tauri::command]
pub fn list_audio_endpoints() -> Vec<AudioEndpoint> {
    match list_output_endpoints() {
        Ok(list) if !list.is_empty() => list,
        _ => vec![placeholder_output()],
    }
}

/// Start the real-device voice bridge (Windows only). Runs on a worker thread.
#[tauri::command]
pub fn start_voice_bridge(device_id: String, output_device: String) -> String {
    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(move || {
            if let Err(e) = core_voice::run_bridge(&device_id, &output_device) {
                core_input::log_error(&format!("[audio] voice bridge error: {e}"));
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

/// Simulate the full voice chain without a real remote.
#[tauri::command]
pub fn simulate_voice_chain(
    output_device: String,
    test_audio_path: Option<String>,
) -> Result<core_voice::SimulatedVoiceResult, String> {
    #[cfg(target_os = "windows")]
    {
        core_voice::simulate_voice_chain(&output_device, test_audio_path.as_deref())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (output_device, test_audio_path);
        Err("仅限 Windows".to_string())
    }
}

#[derive(Serialize)]
pub struct VbCableStatus {
    pub input: bool,
    pub output: bool,
    pub ready: bool,
}

#[tauri::command]
pub fn vb_cable_status() -> VbCableStatus {
    let d = core_audio::diagnostics::run();
    VbCableStatus {
        input: d.cable_input_present,
        output: d.cable_output_present,
        ready: d.has_vb_cable,
    }
}

/// One-click VB-CABLE install: runs the official installer helper.
#[tauri::command]
pub fn install_vb_cable() -> String {
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
                    std::env::current_dir()
                        .map(|c| c.display().to_string())
                        .unwrap_or_default()
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
pub fn audio_diagnostics() -> core_audio::diagnostics::AudioDiagnostics {
    core_audio::diagnostics::run()
}

/// Loop the test tone several times into the selected endpoint.
#[tauri::command]
pub fn play_test_tone_loop(device_name: Option<String>, repetitions: Option<u32>) -> String {
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
pub fn play_test_tone(device_name: Option<String>) -> String {
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