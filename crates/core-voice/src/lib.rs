//! core-voice — orchestration: ATVV bytes -> ADPCM decode -> 48k stereo frames.

use core_atvv::adpcm::ImaAdpcmDecoder;
use core_atvv::frame::AudioFrameAssembler;
use core_atvv::protocol::ControlEvent;
use core_atvv::session::VoiceSession;
use serde::Serialize;

/// Default ATVV audio frame length (bytes) — may need real-device tuning.
pub const ATVV_FRAME_BYTES: usize = 120;

/// Result of feeding one batch of raw BLE bytes.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VoiceChunk {
    pub bytes_fed: usize,
    pub complete_frames: usize,
    pub pcm_samples: usize,
    pub output_samples: usize,
    pub dropped_bytes: usize,
}

/// Orchestrates the remote->system voice path for one session.
pub struct VoiceEngine {
    decoder: ImaAdpcmDecoder,
    assembler: AudioFrameAssembler,
    session: VoiceSession,
    now_ms: u64,
}

impl Default for VoiceEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceEngine {
    pub fn new() -> Self {
        Self {
            decoder: ImaAdpcmDecoder::new(),
            assembler: AudioFrameAssembler::new(ATVV_FRAME_BYTES),
            session: VoiceSession::new(),
            now_ms: 0,
        }
    }

    /// Advance the logical clock (milliseconds).
    pub fn advance(&mut self, ms: u64) {
        self.now_ms = self.now_ms.saturating_add(ms);
    }

    /// Feed a control event (MicOpen / StreamStart / StreamStop).
    pub fn on_control(&mut self, event: ControlEvent) -> Result<(), core_atvv::AtvvError> {
        self.session.on_control(event, self.now_ms)
    }

    /// Feed raw Audio-characteristic bytes; decodes complete frames.
    pub fn feed(&mut self, bytes: &[u8]) -> VoiceChunk {
        let mut chunk = VoiceChunk {
            bytes_fed: bytes.len(),
            ..Default::default()
        };

        for frame in self.assembler.push(bytes) {
            chunk.complete_frames += 1;

            let accepted = match self
                .session
                .on_audio_frame(self.now_ms, frame.len() * 2)
            {
                Ok(ok) => ok,
                Err(_) => false,
            };

            if !accepted {
                chunk.dropped_bytes += frame.len();
                continue;
            }

            let pcm = self.decoder.decode_bytes(&frame);
            chunk.pcm_samples += pcm.len();

            let mut mono_f32: Vec<f32> = pcm.iter().map(|&s| f32::from(s) / 32768.0).collect();
            // Keep any adaptive gain modest for now (unit-level; value verified later).
            let frame_out = core_audio::build_output_frame(&mono_f32, core_audio::DEFAULT_GAIN_DB);
            chunk.output_samples += frame_out.len();
            let _ = &mut mono_f32;
        }

        chunk
    }

    /// Whether the session is currently streaming.
    pub fn is_streaming(&self) -> bool {
        self.session.is_streaming()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_before_stream_start_are_dropped() {
        let mut engine = VoiceEngine::new();
        let chunk = engine.feed(&[0u8; 240]);
        assert_eq!(chunk.complete_frames, 2);
        assert_eq!(chunk.pcm_samples, 0);
        assert_eq!(chunk.dropped_bytes, 240);
    }

    #[test]
    fn streaming_frame_produces_48k_stereo_output() {
        let mut engine = VoiceEngine::new();
        engine.on_control(ControlEvent::MicOpen).unwrap();
        engine.on_control(ControlEvent::StreamStart).unwrap();

        let chunk = engine.feed(&[0x55; 120]);
        assert_eq!(chunk.complete_frames, 1);
        assert_eq!(chunk.pcm_samples, 240); // 120 bytes * 2 samples
        assert_eq!(chunk.output_samples, 240 * 3 * 2); // 48k stereo
    }

    #[test]
    fn stop_then_feed_within_tail_is_dropped() {
        let mut engine = VoiceEngine::new();
        engine.on_control(ControlEvent::StreamStart).unwrap();
        engine.feed(&[0x11; 120]);
        engine.advance(1000);
        engine.on_control(ControlEvent::StreamStop).unwrap();
        let chunk = engine.feed(&[0x22; 120]);
        assert_eq!(chunk.pcm_samples, 0);
    }
}
