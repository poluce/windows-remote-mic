//! 语音会话状态机。
//!
//! 解码器被放在该状态机之外，以便跨会话重置；
//! 状态机只跟踪*生命周期*和*门控*决策，
//! 例如丢弃 STREAM_STOP 之后紧接着到达的音频尾部。

use crate::error::{AtvvError, Result};
use crate::protocol::ControlEvent;

/// STREAM_STOP 之后的尾部窗口，在此期间音频会被静默忽略。
pub const STOP_TAIL_DROP_MS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Opening,
    Streaming,
    Stopping,
}

/// 纯语音会话控制器。
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

    /// 输入一个控制事件。`now_ms` 是调用方提供的逻辑单调毫秒
    /// 时钟（例如来自 BLE 事件循环）。
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

    /// 输入一个已解码的源采样（PCM）。只有流式传输期间才会计数；
    /// 停止后尾部窗口内的数据会被丢弃。
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
                    // 尾部窗口已过：会话结束，永远丢弃。
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
        // 100ms 之后仍在 300ms 尾部窗口内。
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
