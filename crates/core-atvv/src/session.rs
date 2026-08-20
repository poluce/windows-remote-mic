//! Voice session state machine.
//!
//! The decoder is kept outside this state machine so it can be reset across
//! sessions; the state machine only tracks *lifecycle* and *gate* decisions,
//! e.g. dropping the audio tail that arrives right after STREAM_STOP.

use crate::error::{AtvvError, Result};
use crate::protocol::ControlEvent;

/// Tail window after STREAM_STOP during which audio is silently ignored.
pub const STOP_TAIL_DROP_MS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Opening,
    Streaming,
    Stopping,
}

/// Pure voice-session controller.
#[derive(Debug, Clone)]
pub struct VoiceSession {
    state: VoiceState,
    stop_at_ms: Option<u64>,
    frames_so_far: u64,
    samples_so_far: u64,
}

impl Default for VoiceSession {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceSession {
    pub fn new() -> Self {
        Self {
            state: VoiceState::Idle,
            stop_at_ms: None,
            frames_so_far: 0,
            samples_so_far: 0,
        }
    }

    pub fn state(&self) -> VoiceState {
        self.state
    }

    pub fn frames_so_far(&self) -> u64 {
        self.frames_so_far
    }

    pub fn samples_so_far(&self) -> u64 {
        self.samples_so_far
    }

    pub fn is_streaming(&self) -> bool {
        self.state == VoiceState::Streaming
    }

    /// Feed a control event. `now_ms` is a logical monotonic millisecond
    /// clock supplied by the caller (e.g. from a BLE event loop).
    pub fn on_control(&mut self, event: ControlEvent, now_ms: u64) -> Result<()> {
        match (self.state, event) {
            (VoiceState::Idle, ControlEvent::MicOpen) => {
                self.state = VoiceState::Opening;
                Ok(())
            }
            (VoiceState::Opening, ControlEvent::StreamStart)
            | (VoiceState::Stopping, ControlEvent::StreamStart)
            | (VoiceState::Idle, ControlEvent::StreamStart) => {
                self.state = VoiceState::Streaming;
                self.frames_so_far = 0;
                self.samples_so_far = 0;
                self.stop_at_ms = None;
                Ok(())
            }
            (VoiceState::Streaming, ControlEvent::StreamStop) => {
                self.state = VoiceState::Stopping;
                self.stop_at_ms = Some(now_ms);
                Ok(())
            }
            (VoiceState::Streaming, ControlEvent::MicExtend) => Ok(()),
            (VoiceState::Stopping, ControlEvent::MicOpen) => {
                self.state = VoiceState::Opening;
                Ok(())
            }
            _ => Err(AtvvError::InvalidControl {
                state: format!("{:?}", self.state),
                detail: format!("unexpected event {:?}", event),
            }),
        }
    }

    /// Feed one decoded source sample (PCM). Audio is only counted while
    /// streaming; anything inside the post-stop tail window is dropped.
    pub fn on_audio_frame(&mut self, now_ms: u64, sample_count: usize) -> Result<bool> {
        match self.state {
            VoiceState::Streaming => {
                self.frames_so_far += 1;
                self.samples_so_far += sample_count as u64;
                Ok(true)
            }
            VoiceState::Stopping => {
                let within_tail = self
                    .stop_at_ms
                    .map(|t| now_ms.saturating_sub(t) <= STOP_TAIL_DROP_MS)
                    .unwrap_or(false);
                if within_tail {
                    Ok(false)
                } else {
                    // Tail window elapsed: session is finished, drop forever.
                    self.state = VoiceState::Idle;
                    self.stop_at_ms = None;
                    Ok(false)
                }
            }
            VoiceState::Opening | VoiceState::Idle => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_then_stream_then_stop() {
        let mut s = VoiceSession::new();
        s.on_control(ControlEvent::MicOpen, 0).unwrap();
        s.on_control(ControlEvent::StreamStart, 0).unwrap();
        assert!(s.is_streaming());
        assert!(s.on_audio_frame(0, 240).unwrap());
        s.on_control(ControlEvent::StreamStop, 1000).unwrap();
        assert!(!s.is_streaming());
    }

    #[test]
    fn audio_within_tail_after_stop_is_dropped() {
        let mut s = VoiceSession::new();
        s.on_control(ControlEvent::StreamStart, 0).unwrap();
        s.on_control(ControlEvent::StreamStop, 1000).unwrap();
        // 100ms later is inside the 300ms tail window.
        assert!(!s.on_audio_frame(1100, 240).unwrap());
    }

    #[test]
    fn audio_after_tail_window_is_dropped() {
        let mut s = VoiceSession::new();
        s.on_control(ControlEvent::StreamStart, 0).unwrap();
        s.on_control(ControlEvent::StreamStop, 1000).unwrap();
        assert!(!s.on_audio_frame(2000, 240).unwrap());
    }

    #[test]
    fn restart_resets_counters() {
        let mut s = VoiceSession::new();
        s.on_control(ControlEvent::StreamStart, 0).unwrap();
        s.on_audio_frame(1, 240).unwrap();
        assert_eq!(s.frames_so_far(), 1);

        s.on_control(ControlEvent::StreamStop, 100).unwrap();
        s.on_control(ControlEvent::StreamStart, 150).unwrap();
        assert!(s.is_streaming());
        assert_eq!(s.frames_so_far(), 0);
        assert!(s.on_audio_frame(200, 240).unwrap());
    }

    #[test]
    fn unexpected_stop_in_idle_is_rejected() {
        let mut s = VoiceSession::new();
        assert!(s.on_control(ControlEvent::StreamStop, 0).is_err());
    }
}
