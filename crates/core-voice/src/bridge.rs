//! Windows-only real-device bridge: BLE -> decode -> CABLE output -> Win+H.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use core_atvv::protocol::{ControlEvent, RawControlEvent, GET_CAPABILITIES_V10, parse_control};
use core_ble::capture::CaptureRecorder;
use core_ble::gatt::AtvvLink;
use core_input::{press_escape, press_win_h};

use crate::VoiceEngine;

/// Run the voice bridge using real ATVV control events.
pub fn run_bridge(device_id: &str, output_device: &str) -> Result<(), String> {
    let link = AtvvLink::connect(device_id).map_err(|e| e.to_string())?;

    link.enable_audio_notifications()
        .map_err(|e| e.to_string())?;
    link.enable_control_notifications()
        .map_err(|e| e.to_string())?;
    link.write_tx(&GET_CAPABILITIES_V10)
        .map_err(|e| e.to_string())?;

    let sink = Arc::new(
        core_audio::sink::AudioSink::new(Some(output_device)).map_err(|e| e.to_string())?,
    );

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
                let _ = frame_tx.send(chunk.output);
            }
        })
        .map_err(|e| e.to_string())?;

    let engine_ctrl = engine.clone();
    let capture_ctrl = capture.clone();
    let is_active_ctrl = is_active.clone();
    let _control_cookie = link
        .register_control_handler(move |bytes| {
            capture_ctrl.record("control", &bytes);
            let mut eng = match engine_ctrl.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let Some(event) = parse_control(&bytes) else {
                return;
            };
            match event {
                RawControlEvent::Caps(caps) => {
                    if caps.sample_rate_hz != core_atvv::protocol::REMOTE_SAMPLE_RATE_HZ {
                        core_input::log_warn(&format!(
                            "[bridge] unsupported ATVV sample rate: {}",
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
                        core_input::log_line("[bridge] 麦克风按键再次点击 -> 关闭语音输入 (Escape)");
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
                            core_input::log_line("[bridge] 遥控器 AudioStopped -> 关闭语音输入 (Escape)");
                            let _ = eng.on_control(ControlEvent::StreamStop);
                            if let Err(e) = press_escape() {
                                core_input::log_error(&format!("[bridge] close voice typing failed: {e}"));
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
        match frame_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(frames) => sink.push(&frames),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}
