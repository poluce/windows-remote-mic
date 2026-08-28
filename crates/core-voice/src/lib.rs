//! core-voice — 编排：ATVV 字节 -> ADPCM 解码 -> 48k 立体声帧。

use core_atvv::adpcm::ImaAdpcmDecoder;
use core_atvv::frame::AudioFrameAssembler;
use core_atvv::protocol::ControlEvent;
use core_atvv::session::VoiceSession;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// ATVV 音频帧的默认长度（字节）——可能需要根据真实设备调整。
pub const ATVV_FRAME_BYTES: usize = 120;

/// 当前 ATVV/BLE 链路是否处于已连接状态。
/// 由 [`run_bridge`] 在连接成功/断开时更新，供 UI 查询初始状态。
static CONNECTION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// 当前语音桥是否处于运行状态。
static BRIDGE_RUNNING: AtomicBool = AtomicBool::new(false);

/// 最近一次成功发现/缓存的 ATVV 端点是否齐全。
static ATVV_ENDPOINTS_READY: Mutex<bool> = Mutex::new(false);

/// 查询当前 ATVV/BLE 链路是否已连接。
pub fn connection_active() -> bool {
    CONNECTION_ACTIVE.load(Ordering::Relaxed)
}

/// 设置当前 ATVV/BLE 链路连接状态（内部使用）。
pub fn set_connection_active(active: bool) {
    CONNECTION_ACTIVE.store(active, Ordering::Relaxed);
}

/// 查询当前语音桥是否运行中。
pub fn bridge_running() -> bool {
    BRIDGE_RUNNING.load(Ordering::Relaxed)
}

/// 设置当前语音桥运行状态（内部使用）。
pub fn set_bridge_running(running: bool) {
    BRIDGE_RUNNING.store(running, Ordering::Relaxed);
}

/// 查询 ATVV 端点是否已就绪（audio + control 均存在）。
pub fn atvv_endpoints_ready() -> bool {
    *ATVV_ENDPOINTS_READY.lock().unwrap_or_else(|e| e.into_inner())
}

/// 设置 ATVV 端点就绪状态（内部使用）。
pub fn set_atvv_endpoints_ready(ready: bool) {
    *ATVV_ENDPOINTS_READY.lock().unwrap_or_else(|e| e.into_inner()) = ready;
}

/// 向引擎喂入一批原始 BLE 字节后的结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct VoiceChunk {
    pub bytes_fed: usize,
    pub complete_frames: usize,
    pub pcm_samples: usize,
    pub output_samples: usize,
    pub dropped_bytes: usize,
    /// 准备写入输出设备的 48 kHz 立体声浮点帧。
    pub output: Vec<f32>,
}

/// 为单个会话编排从遥控器到系统的语音通路。
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

    /// 推进逻辑时钟（毫秒）。
    pub fn advance(&mut self, ms: u64) {
        self.now_ms = self.now_ms.saturating_add(ms);
    }

    /// 送入一个控制事件（MicOpen / StreamStart / StreamStop）。
    pub fn on_control(&mut self, event: ControlEvent) -> Result<(), core_atvv::AtvvError> {
        if event == ControlEvent::StreamStart {
            self.decoder.reset();
        }
        self.session.on_control(event, self.now_ms)
    }

    /// 送入 Audio 特征的原始字节；解码出完整帧。
    pub fn feed(&mut self, bytes: &[u8]) -> VoiceChunk {
        let mut chunk = VoiceChunk {
            bytes_fed: bytes.len(),
            ..Default::default()
        };

        for frame in self.assembler.push(bytes) {
            chunk.complete_frames += 1;

            let accepted = self
                .session
                .on_audio_frame(self.now_ms, frame.len() * 2)
                .unwrap_or_default();

            if !accepted {
                chunk.dropped_bytes += frame.len();
                continue;
            }

            let pcm = self.decoder.decode_bytes(&frame);
            chunk.pcm_samples += pcm.len();

            let mut mono_f32: Vec<f32> = pcm.iter().map(|&s| f32::from(s) / 32768.0).collect();
            // 暂时让自适应增益保持适中（单位级；数值后续再验证）。
            let frame_out = core_audio::build_output_frame(&mono_f32, core_audio::DEFAULT_GAIN_DB);
            chunk.output_samples += frame_out.len();
            chunk.output.extend_from_slice(&frame_out);
            let _ = &mut mono_f32;
        }

        chunk
    }

    /// 会话当前是否正在流式传输（streaming）。
    pub fn is_streaming(&self) -> bool {
        self.session.is_streaming()
    }

    /// 将已解码的 PCM 注入输出阶段。
    ///
    /// 这是在远端音频解析/解码之后添加指定测试音频的扩展点：
    /// 调用方提供已解码的 16 位单声道 PCM，
    /// 它会被转换为与真实桥接路径相同的 48 kHz 立体声输出。
    pub fn feed_pcm(&mut self, samples: &[i16]) -> VoiceChunk {
        let mut chunk = VoiceChunk {
            bytes_fed: 0,
            complete_frames: 0,
            pcm_samples: samples.len(),
            ..Default::default()
        };
        let mono_f32: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32768.0).collect();
        let frame_out = core_audio::build_output_frame(&mono_f32, core_audio::DEFAULT_GAIN_DB);
        chunk.output_samples = frame_out.len();
        chunk.output = frame_out;
        chunk
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
        assert_eq!(chunk.pcm_samples, 240); // 120 字节 * 2 采样
        assert_eq!(chunk.output_samples, 240 * 3 * 2); // 48k 立体声
    }

    #[test]
    fn feed_pcm_produces_48k_stereo_output() {
        let mut engine = VoiceEngine::new();
        let chunk = engine.feed_pcm(&[0i16; 240]);
        assert_eq!(chunk.pcm_samples, 240);
        assert_eq!(chunk.output_samples, 240 * 3 * 2); // 48k 立体声
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

#[cfg(target_os = "windows")]
pub mod bridge;

#[cfg(target_os = "windows")]
pub use bridge::run_bridge;

#[cfg(target_os = "windows")]
pub mod simulate;

#[cfg(target_os = "windows")]
pub use simulate::{simulate_voice_chain, SimulatedVoiceResult};
