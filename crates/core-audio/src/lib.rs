//! core-audio — Windows Remote Mic 应用的虚拟音频输出。
//!
//! 职责：接收来自 `core-atvv` 的解码后 16 kHz 单声道 PCM，
//! 处理为目标格式（48 kHz / 立体声 / 增益 / DC 阻挡），并写入
//! 用户选择的输出端点（例如 VB-CABLE 的 CABLE Input）。

#[cfg(target_os = "windows")]
mod wasapi;

#[cfg(target_os = "windows")]
pub mod default_device;
#[cfg(target_os = "windows")]
pub mod playback;
#[cfg(target_os = "windows")]
pub mod sink;

pub mod diagnostics;
pub mod dsp;
pub mod endpoint;
pub mod error;
pub mod test_tone;

pub use error::{AudioError, Result};

/// 后处理前解码后的遥控器音频格式。
pub const REMOTE_AUDIO_CHANNELS: u16 = 1;
/// 写入输出端点的声道数（VB-CABLE 端点为立体声）。
pub const OUTPUT_CHANNELS: u16 = 2;
/// 发送到端点前应用的默认输出增益（dB）。
pub const DEFAULT_GAIN_DB: f32 = 10.0;

/// 生成默认 1 秒 / 1 kHz / 立体声测试音。
pub fn default_test_tone() -> Vec<f32> {
    test_tone::default_test_tone()
}

/// 从原始单声道 16 kHz 采样构建完整输出帧。
///
/// 流水线：16k->48k 重采样 -> 增益 -> DC 阻挡 -> 立体声复制 -> 限幅。
pub fn build_output_frame(mono_16k: &[f32], gain_db: f32) -> Vec<f32> {
    let mut resampled = dsp::resample_16k_to_48k(mono_16k);
    dsp::apply_gain_db(&mut resampled, gain_db);
    let mut block = dsp::DcBlock::new();
    block.process_batch(&mut resampled);
    let mut stereo = dsp::to_channels(&resampled, OUTPUT_CHANNELS);
    dsp::hard_limiter(&mut stereo, 1.0);
    stereo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_frame_is_48k_stereo() {
        let input = vec![0.25; 160]; // 16 kHz 下 10 毫秒
        let out = build_output_frame(&input, DEFAULT_GAIN_DB);
        assert_eq!(out.len(), 160 * 3 * 2);
    }

    #[test]
    fn output_frame_never_exceeds_unit() {
        let input = vec![0.9; 480];
        let out = build_output_frame(&input, 30.0);
        assert!(out.iter().all(|s| s.abs() <= 1.0));
    }
}
