//! Windows-only simulated voice chain: no real remote required.

use core_atvv::protocol::ControlEvent;
use core_input::press_win_h;
use serde::Serialize;

use crate::VoiceEngine;

/// Result of the simulated voice chain.
#[derive(Debug, Clone, Serialize)]
pub struct SimulatedVoiceResult {
    pub frames: usize,
    pub pcm_samples: usize,
    pub output_samples: usize,
    pub win_h_toast: bool,
}

/// Run: Win+H start -> StreamStart -> feed synthetic ATVV bytes -> CABLE output
/// -> StreamStop -> Win+H stop.
pub fn simulate_voice_chain(output_device: &str) -> Result<SimulatedVoiceResult, String> {
    let sink = core_audio::sink::AudioSink::new(Some(output_device))
        .map_err(|e| e.to_string())?;

    let mut engine = VoiceEngine::new();
    engine.on_control(ControlEvent::StreamStart).map_err(|e| e.to_string())?;
    press_win_h().map_err(|e| e.to_string())?;

    // Synthetic ATVV frames (0x55 is just some non-zero ADPCM payload).
    let synthetic = vec![0x55u8; 120 * 5];
    let chunk = engine.feed(&synthetic);
    sink.push(&chunk.output);

    // Let the sink play a short burst.
    std::thread::sleep(std::time::Duration::from_millis(400));

    engine.on_control(ControlEvent::StreamStop).map_err(|e| e.to_string())?;
    press_win_h().map_err(|e| e.to_string())?;

    Ok(SimulatedVoiceResult {
        frames: chunk.complete_frames,
        pcm_samples: chunk.pcm_samples,
        output_samples: chunk.output_samples,
        win_h_toast: true,
    })
}
