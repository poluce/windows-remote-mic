//! core-audio — virtual audio output for the Windows Remote Mic app.
//!
//! Responsibility: take decoded 16 kHz mono PCM from `core-atvv`, process it
//! into the destination format (48 kHz / stereo / gain / DC-block), and write
//! it to the user-selected output endpoint (e.g. VB-CABLE's CABLE Input).

#[cfg(target_os = "windows")]
mod wasapi;

pub mod dsp;
pub mod endpoint;
pub mod error;

pub use error::{AudioError, Result};

/// Format of decoded remote audio before post-processing.
pub const REMOTE_AUDIO_CHANNELS: u16 = 1;
/// Channel count written to the output endpoint (VB-CABLE endpoints are stereo).
pub const OUTPUT_CHANNELS: u16 = 2;
/// Default output gain in dB applied before sending to the endpoint.
pub const DEFAULT_GAIN_DB: f32 = 10.0;

/// Builds a full output frame from raw mono 16 kHz samples.
///
/// Pipeline: resample 16k->48k -> gain -> DC block -> stereo copy -> limiter.
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
        let input = vec![0.25; 160]; // 10 ms at 16 kHz
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
