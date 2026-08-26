//! Windows-only real-device bridge: BLE -> decode -> CABLE output -> Win+H.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use core_atvv::protocol::{ControlEvent, RawControlEvent, GET_CAPABILITIES_V10, parse_control};
use core_ble::capture::CaptureRecorder;
use core_ble::gatt::AtvvLink;
use core_input::press_win_h;

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
                    // Win+H is a toggle: one press starts system voice typing.
                    core_input::log_line("[bridge] MicButtonPressed -> Win+H start");
                    if let Err(e) = press_win_h() {
                        core_input::log_error(&format!("[bridge] Win+H start failed: {e}"));
                    }
                }
                RawControlEvent::AudioStarted { .. } => {
                    let _ = eng.on_control(ControlEvent::StreamStart);
                }
                RawControlEvent::AudioStopped => {
                    let _ = eng.on_control(ControlEvent::StreamStop);
                    // Toggle off: second Win+H press.
                    core_input::log_line("[bridge] AudioStopped -> Win+H stop");
                    if let Err(e) = press_win_h() {
                        core_input::log_error(&format!("[bridge] Win+H stop failed: {e}"));
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
