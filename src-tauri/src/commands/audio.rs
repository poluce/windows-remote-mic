use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::Emitter;

use crate::find_install_script;

/// 已 spawn 的语音桥重连循环数（每次调用 start_voice_bridge +1，永不减）。
static BRIDGE_LOOP_SEQ: AtomicU64 = AtomicU64::new(0);
/// 当前仍卡在 run_bridge 内的实例数（用于发现并发双桥）。
static BRIDGE_INFLIGHT: AtomicU64 = AtomicU64::new(0);

/// 启动真实设备的语音桥（仅 Windows）。在工作线程中运行。
#[tauri::command]
pub fn start_voice_bridge(
    app: tauri::AppHandle,
    device_id: String,
    output_device: String,
) -> String {
    #[cfg(target_os = "windows")]
    {
        let loop_id = BRIDGE_LOOP_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
        let inflight = BRIDGE_INFLIGHT.load(Ordering::SeqCst);
        core_log::log_info(&format!(
            "[commands/audio] 收到启动语音桥请求：loop_id={loop_id}, inflight={inflight}, 设备 ID='{device_id}'，输出='{output_device}'"
        ));
        if loop_id > 1 {
            core_log::log_warn(&format!(
                "[commands/audio] 注意：这是第 {loop_id} 条重连循环（旧循环可能仍在 sleep/run），易造成双桥争用 GATT"
            ));
        }
        let app_for_thread = app.clone();
        std::thread::spawn(move || {
            let mut attempt: u64 = 0;
            loop {
                let app_cb = app_for_thread.clone();
                let on_status = move |connected: bool| {
                    let _ = app_cb.emit("ble-connection-status", connected);
                };

                let inflight_now = BRIDGE_INFLIGHT.fetch_add(1, Ordering::SeqCst) + 1;
                core_log::log_line(&format!(
                    "[commands/audio] 语音桥启动: loop_id={loop_id}, attempt={}, inflight={inflight_now}",
                    attempt + 1
                ));
                if inflight_now > 1 {
                    core_log::log_warn(&format!(
                        "[commands/audio] 并发桥检测: 同时有 {inflight_now} 个 run_bridge 在跑（loop_id={loop_id}）"
                    ));
                }
                let result = core_voice::run_bridge(&device_id, &output_device, on_status);
                BRIDGE_INFLIGHT.fetch_sub(1, Ordering::SeqCst);
                match result {
                    Ok(()) => {
                        core_log::log_line(&format!(
                            "[commands/audio] 语音桥已停止，准备重连: loop_id={loop_id}, attempt={}",
                            attempt + 1
                        ));
                    }
                    Err(e) => {
                        core_log::log_error(&format!(
                            "[commands/audio] 语音桥错误: loop_id={loop_id}, attempt={}, err={e}",
                            attempt + 1
                        ));
                    }
                }
                // 注意：不要在这里无条件 emit ble-connection-status=false。
                // run_bridge 内部已通过 on_status 在 BLE 真正连接/断开时推送状态；
                // 这里只是“语音桥本次会话结束”，不代表遥控器连接断开。
                attempt += 1;
                let delay_secs = (attempt * 2).min(10);
                core_log::log_line(&format!(
                    "[commands/audio] 将在 {delay_secs} 秒后重连（loop_id={loop_id}, 第 {attempt} 次）"
                ));
                std::thread::sleep(Duration::from_secs(delay_secs));
                core_log::log_line(&format!(
                    "[commands/audio] 重连等待结束，开始下一次尝试（loop_id={loop_id}, 即将 attempt={}）",
                    attempt + 1
                ));
            }
        });
        "语音桥已启动（断线后自动重连）".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, device_id, output_device);
        "语音桥仅在 Windows 可用".to_string()
    }
}

/// 在没有真实遥控器的情况下模拟完整语音链路。
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

/// 一键安装 VB-CABLE：运行官方安装辅助脚本。
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
            Ok(out) => format!("安装失败：{}", String::from_utf8_lossy(&out.stderr).trim()),
            Err(e) => format!("无法启动安装程序：{e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = ();
        "VB-CABLE 安装仅在 Windows 上可用".to_string()
    }
}

/// 运行音频诊断：端点 + VB-CABLE 是否存在。
#[tauri::command]
pub fn audio_diagnostics() -> core_audio::diagnostics::AudioDiagnostics {
    core_audio::diagnostics::run()
}

/// 向所选端点循环播放数次测试音。
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

/// 触发 Windows 语音输入（Win+H），帮助用户在引导流程中配置 CABLE Output。
#[tauri::command]
pub fn trigger_voice_typing() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        core_input::press_win_h().map_err(|e| e.to_string())?;
        Ok("已唤出 Windows 语音输入条，请点击 ⚙️ 齿轮将麦克风选为 CABLE Output".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("仅限 Windows".to_string())
    }
}
