//! Windows-only real-device bridge: BLE -> decode -> CABLE output -> Win+H.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use core_atvv::protocol::ControlEvent;
use core_ble::gatt::AtvvLink;
use core_input::press_win_h;

use crate::VoiceEngine;

/// Run the voice bridge until the process stops.
///
/// NOTE: until real command opcodes are captured, the first audio notification
/// is treated as an implicit stream start and triggers Win+H once. Replace with
/// proper MicOpen/StreamStart/StreamStop once confirmed on hardware.
pub fn run_bridge(device_id: &str, output_device: &str) -> Result<(), String> {
    let link = AtvvLink::connect(device_id).map_err(|e| e.to_string())?;
    link.enable_audio_notifications()
        .map_err(|e| e.to_string())?;

    let sink = core_audio::sink::AudioSink::new(Some(output_device))
        .map_err(|e| e.to_string())?;

    let engine = Arc::new(Mutex::new(VoiceEngine::new()));
    let stream_started = Arc::new(AtomicBool::new(false));

    // Event thread -> main thread bridge (mpsc::Sender is Send).
    let (frame_tx, frame_rx) = mpsc::channel::<Vec<f32>>();
    let engine_cb = engine.clone();
    let started_cb = stream_started.clone();

    let _cookie = link
        .register_audio_handler(move |bytes| {
            let mut eng = match engine_cb.lock() {
                Ok(g) => g,
                Err(_) => return,
            };

            // Provisional: first audio bytes = voice started.
            if !started_cb.load(Ordering::Relaxed) {
                let _ = eng.on_control(ControlEvent::StreamStart);
                if let Err(e) = press_win_h() {
                    eprintln!("press Win+H failed: {e}");
                }
                started_cb.store(true, Ordering::Relaxed);
            }

            let chunk = eng.feed(&bytes);
            if !chunk.output.is_empty() {
                let _ = frame_tx.send(chunk.output);
            }
        })
        .map_err(|e| e.to_string())?;

    // Main loop: receive decoded frames and push them to the audio sink.
    // Keeps the link alive while waiting.
    loop {
        match frame_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(frames) => sink.push(&frames),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}
