//! Windows-only simulated voice chain: no real remote required.

use core_atvv::protocol::ControlEvent;
use core_input::press_win_h;
use serde::Serialize;
use std::f32::consts::TAU;

use crate::VoiceEngine;

/// Default simulated test-signal metadata, so the UI can show what audio was played.
pub const TEST_TONE_HZ: u32 = 1000;
pub const TEST_TONE_MS: u64 = 500;

/// Result of the simulated voice chain.
#[derive(Debug, Clone, Serialize)]
pub struct SimulatedVoiceResult {
    pub frames: usize,
    pub pcm_samples: usize,
    pub output_samples: usize,
    pub win_h_toast: bool,
    pub test_tone_hz: u32,
    pub test_tone_ms: u64,
}

/// Run: Win+H start -> StreamStart -> inject specified test PCM after the
/// remote-audio parsing stage (via `VoiceEngine::feed_pcm`) -> CABLE output
/// -> StreamStop -> Win+H stop.
pub fn simulate_voice_chain(
    output_device: &str,
    test_tone_hz: Option<u32>,
    test_tone_ms: Option<u64>,
) -> Result<SimulatedVoiceResult, String> {
    let hz = test_tone_hz.unwrap_or(TEST_TONE_HZ);
    let ms = test_tone_ms.unwrap_or(TEST_TONE_MS);

    let sink = core_audio::sink::AudioSink::new(Some(output_device))
        .map_err(|e| e.to_string())?;

    let mut engine = VoiceEngine::new();
    engine.on_control(ControlEvent::StreamStart).map_err(|e| e.to_string())?;
    press_win_h().map_err(|e| e.to_string())?;

    // Generate a sine test tone at the remote's 16 kHz voice rate and inject
    // it after parsing/decoding, so downstream output can be tested with a
    // caller-specified audio signal.
    let sample_rate = 16_000u32;
    let total_samples = (sample_rate as u64 * ms / 1000) as usize;
    let amplitude = 6_000f32;
    let samples: Vec<i16> = (0..total_samples)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            ((t * hz as f32 * TAU).sin() * amplitude) as i16
        })
        .collect();

    let chunk = engine.feed_pcm(&samples);
    sink.push(&chunk.output);

    // Let the sink play the test tone.
    std::thread::sleep(std::time::Duration::from_millis(ms + 200));

    engine.on_control(ControlEvent::StreamStop).map_err(|e| e.to_string())?;
    press_win_h().map_err(|e| e.to_string())?;

    Ok(SimulatedVoiceResult {
        frames: chunk.complete_frames,
        pcm_samples: chunk.pcm_samples,
        output_samples: chunk.output_samples,
        win_h_toast: true,
        test_tone_hz: hz,
        test_tone_ms: ms,
    })
}