//! Windows-only real-device bridge: BLE -> decode -> CABLE output -> Win+H.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;

use core_atvv::protocol::{parse_control, ControlEvent, RawControlEvent, GET_CAPABILITIES_V10};
use core_ble::capture::CaptureRecorder;
use core_ble::gatt::AtvvLink;
use core_input::{press_escape, press_win_h};

use crate::VoiceEngine;

/// Run the voice bridge using real ATVV control events.
///
/// `on_status` is invoked with `true` when the BLE connection becomes
/// connected and `false` when it becomes disconnected.
pub fn run_bridge<F>(device_id: &str, output_device: &str, on_status: F) -> Result<(), String>
where
    F: Fn(bool) + Send + 'static,
{
    core_log::log_info(&format!(
        "[bridge] starting voice bridge for device_id='{device_id}', output='{output_device}'"
    ));

    let link = AtvvLink::connect(device_id).map_err(|e| {
        core_log::log_error(&format!("[bridge] 连接 ATVV 链路失败: {e}"));
        e.to_string()
    })?;
    core_log::log_info("[bridge] ATVV 链路已连接");

    let disconnected = Arc::new(AtomicBool::new(false));
    let disconnected_cb = disconnected.clone();
    link.register_connection_status_changed(move |connected| {
        let msg = if connected {
            "connected"
        } else {
            "disconnected"
        };
        core_log::log_line(&format!("[bridge] BLE 连接状态变化: {msg}"));
        if !connected {
            disconnected_cb.store(true, Ordering::SeqCst);
        }
        on_status(connected);
    })
    .map_err(|e| e.to_string())?;
    core_log::log_info("[bridge] BLE 连接状态监听已注册");

    link.enable_audio_notifications().map_err(|e| {
        core_log::log_error(&format!("[bridge] 启用音频通知失败: {e}"));
        e.to_string()
    })?;
    core_log::log_info("[bridge] 音频通知已启用");

    link.enable_control_notifications().map_err(|e| {
        core_log::log_error(&format!("[bridge] 启用控制通知失败: {e}"));
        e.to_string()
    })?;
    core_log::log_info("[bridge] 控制通知已启用");

    link.write_tx(&GET_CAPABILITIES_V10).map_err(|e| {
        core_log::log_error(&format!(
            "[bridge] 发送能力查询（GET_CAPABILITIES_V10）失败: {e}"
        ));
        e.to_string()
    })?;
    core_log::log_info("[bridge] 已向遥控器发送能力查询（GET_CAPABILITIES_V10）");

    // ATVV notify is live. Start the optional Back/Volume tap only now so the
    // HOGP inject cannot steal the GATT session during characteristic setup.
    core_hid::tap::start_after_atvv();

    let sink = core_audio::sink::AudioSink::new(Some(output_device)).map_err(|e| {
        core_log::log_error(&format!("[bridge] 初始化音频输出（AudioSink）失败: {e}"));
        e.to_string()
    })?;
    core_log::log_info(&format!(
        "[bridge] 音频输出（AudioSink）已初始化：{output_device}"
    ));

    let capture_dir = std::env::var("LOCALAPPDATA")
        .map(|base| std::path::Path::new(&base).join("RemoteMic/RC003/captures"))
        .unwrap_or_default();
    let capture = CaptureRecorder::new(capture_dir);
    let engine = Arc::new(Mutex::new(VoiceEngine::new()));
    let is_active = Arc::new(Mutex::new(false));

    let (frame_tx, frame_rx) = mpsc::channel::<Vec<f32>>();
    let engine_cb = engine.clone();
    let capture_audio = capture.clone();
    let _audio_cookie = link
        .register_audio_handler(move |bytes| {
            capture_audio.record("audio", &bytes);
            let mut eng = match engine_cb.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let chunk = eng.feed(&bytes);
            if !chunk.output.is_empty() {
                core_log::log_debug(&format!(
                    "[bridge] 音频块解码完成：{} 采样 -> 输出 {} 采样",
                    chunk.pcm_samples, chunk.output_samples
                ));
                let _ = frame_tx.send(chunk.output);
            }
        })
        .map_err(|e| {
            core_log::log_error(&format!("[bridge] 注册音频回调失败: {e}"));
            e.to_string()
        })?;
    core_log::log_info("[bridge] 音频回调已注册");

    let engine_ctrl = engine.clone();
    let capture_ctrl = capture.clone();
    let is_active_ctrl = is_active.clone();
    let _control_cookie = link
        .register_control_handler(move |bytes| {
            capture_ctrl.record("control", &bytes);
            core_log::log_info(&format!("[bridge] 收到控制通知: {:02X?}", bytes));
            let mut eng = match engine_ctrl.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let Some(event) = parse_control(&bytes) else {
                core_log::log_warn(&format!("[bridge] 无法识别的控制包: {:02X?}", bytes));
                return;
            };
            core_log::log_info(&format!("[bridge] 已解析控制事件: {:?}", event));
            match event {
                RawControlEvent::Caps(caps) => {
                    if caps.sample_rate_hz != core_atvv::protocol::REMOTE_SAMPLE_RATE_HZ {
                        core_input::log_warn(&format!(
                            "[bridge] 不支持的 ATVV 采样率: {}",
                            caps.sample_rate_hz
                        ));
                    }
                }
                RawControlEvent::MicButtonPressed => {
                    // Toggle 模式：按一下开启语音输入，再次点击关闭语音输入
                    let mut active = match is_active_ctrl.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    if !*active {
                        *active = true;
                        core_input::log_line("[bridge] 麦克风按键 -> 开启语音输入 (Win+H)");
                        let _ = press_escape();
                        std::thread::sleep(Duration::from_millis(100));
                        if let Err(e) = press_win_h() {
                            core_input::log_error(&format!("[bridge] Win+H 开启失败: {e}"));
                        }
                        let _ = eng.on_control(ControlEvent::StreamStart);
                    } else {
                        *active = false;
                        core_input::log_line(
                            "[bridge] 麦克风按键再次点击 -> 关闭语音输入 (Escape)",
                        );
                        let _ = eng.on_control(ControlEvent::StreamStop);
                        if let Err(e) = press_escape() {
                            core_input::log_error(&format!("[bridge] 关闭语音输入失败: {e}"));
                        }
                    }
                }
                RawControlEvent::AudioStarted { .. } => {
                    let _ = eng.on_control(ControlEvent::StreamStart);
                }
                RawControlEvent::AudioStopped => {
                    // 遥控器硬件停止通知
                    if let Ok(mut active) = is_active_ctrl.lock() {
                        if *active {
                            *active = false;
                            core_input::log_line(
                                "[bridge] 遥控器 AudioStopped -> 关闭语音输入 (Escape)",
                            );
                            let _ = eng.on_control(ControlEvent::StreamStop);
                            if let Err(e) = press_escape() {
                                core_input::log_error(&format!("[bridge] 关闭语音输入失败: {e}"));
                            }
                        }
                    }
                }
                RawControlEvent::AudioSynced { .. } => {
                    // TODO: feed predictor/step_index to decoder for resync.
                }
                RawControlEvent::Unknown(_) => {}
            }
        })
        .map_err(|e| e.to_string())?;

    // Main loop: keep link alive and push decoded frames to the sink.
    loop {
        if disconnected.load(Ordering::SeqCst) {
            core_log::log_line("[bridge] BLE 已断开，停止语音桥以等待自动重连");
            break;
        }
        match frame_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(frames) => sink.push(&frames),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}
