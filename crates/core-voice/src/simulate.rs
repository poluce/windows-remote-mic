//! Windows-only simulated voice chain: no real remote required.

use core_atvv::protocol::ControlEvent;
use core_input::press_win_h;
use serde::Serialize;
use std::path::Path;

use crate::VoiceEngine;

/// Built-in public-domain speech sample used when no custom WAV is supplied.
const TEST_SPEECH_WAV: &[u8] = include_bytes!("../assets/test_speech.wav");

/// Result of the simulated voice chain.
#[derive(Debug, Clone, Serialize)]
pub struct SimulatedVoiceResult {
    pub frames: usize,
    pub pcm_samples: usize,
    pub output_samples: usize,
    pub win_h_toast: bool,
    pub test_audio: String,
    pub test_audio_ms: u64,
}

/// Run: Win+H start -> StreamStart -> inject a real speech WAV after the
/// remote-audio parsing stage (via `VoiceEngine::feed_pcm`) -> CABLE output
/// -> StreamStop -> Win+H stop.
///
/// `test_audio_path` is optional. When `None`, a built-in public-domain speech
/// sample is used; when `Some(path)`, the given PCM WAV file is loaded.
pub fn simulate_voice_chain(
    output_device: &str,
    test_audio_path: Option<&str>,
) -> Result<SimulatedVoiceResult, String> {
    let (sample_rate, pcm) = match test_audio_path {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| format!("读取测试音频失败：{e}"))?;
            load_wav_pcm(&bytes)?
        }
        None => load_wav_pcm(TEST_SPEECH_WAV)?,
    };

    let samples = resample_to_16k(&pcm, sample_rate);
    let duration_ms = (samples.len() as u64 * 1000 / 16_000).max(1);

    let audio_name = test_audio_path
        .and_then(|p| Path::new(p).file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "内置公开语音样本".to_string());

    core_input::log_line(&format!(
        "[simulate] start: device={}, audio={}, sample_rate={}, pcm_samples={}, duration_ms={}",
        output_device, audio_name, sample_rate, pcm.len(), duration_ms
    ));

    let diag = core_audio::diagnostics::run();
    let input_names: Vec<String> = diag.input_endpoints.iter().map(|e| e.name.clone()).collect();
    let default_input = core_audio::endpoint::default_input_name();
    core_input::log_line(&format!(
        "[simulate] diagnostics: has_vb_cable={}, cable_input={}, cable_output={}, default_input={:?}, input_endpoints={:?}",
        diag.has_vb_cable,
        diag.cable_input_present,
        diag.cable_output_present,
        default_input,
        input_names
    ));

    let sink = core_audio::sink::AudioSink::new(Some(output_device))
        .map_err(|e| e.to_string())?;
    core_input::log_debug("[simulate] audio sink created");

    let mut engine = VoiceEngine::new();
    engine.on_control(ControlEvent::StreamStart).map_err(|e| e.to_string())?;
    let _default_guard = core_audio::default_device::DefaultInputGuard::switch_to_cable_output()
        .map_err(|e| format!("切换默认麦克风失败：{e}"))?;
    core_input::log_line("[simulate] Win+H start");
    press_win_h().map_err(|e| e.to_string())?;

    let chunk = engine.feed_pcm(&samples);
    core_input::log_line(&format!(
        "[simulate] pushed output samples={}, pcm_samples={}",
        chunk.output_samples, chunk.pcm_samples
    ));
    sink.push(&chunk.output);

    // Let the sink finish playing the speech sample.
    core_input::log_debug(&format!(
        "[simulate] sleeping {} ms for playback",
        duration_ms + 300
    ));
    std::thread::sleep(std::time::Duration::from_millis(duration_ms + 300));

    engine.on_control(ControlEvent::StreamStop).map_err(|e| e.to_string())?;
    core_input::log_line("[simulate] Win+H stop");
    press_win_h().map_err(|e| e.to_string())?;

    core_input::log_line("[simulate] done");
    Ok(SimulatedVoiceResult {
        frames: chunk.complete_frames,
        pcm_samples: chunk.pcm_samples,
        output_samples: chunk.output_samples,
        win_h_toast: true,
        test_audio: audio_name,
        test_audio_ms: duration_ms,
    })
}

/// Parse a standard PCM WAV and return `(sample_rate, mono i16 samples)`.
fn load_wav_pcm(data: &[u8]) -> Result<(u32, Vec<i16>), String> {
    if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err("不是有效的 WAV 文件".to_string());
    }

    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut bits_per_sample = 0u16;
    let mut raw_data: Option<&[u8]> = None;

    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            data[pos + 4],
            data[pos + 5],
            data[pos + 6],
            data[pos + 7],
        ]) as usize;
        let body = pos + 8;
        if body + chunk_size > data.len() {
            break;
        }

        match chunk_id {
            b"fmt " => {
                if chunk_size < 16 {
                    return Err("WAV fmt 块不完整".to_string());
                }
                let audio_format = u16::from_le_bytes([data[body], data[body + 1]]);
                if audio_format != 1 {
                    return Err("仅支持 PCM WAV".to_string());
                }
                channels = u16::from_le_bytes([data[body + 2], data[body + 3]]);
                sample_rate = u32::from_le_bytes([
                    data[body + 4],
                    data[body + 5],
                    data[body + 6],
                    data[body + 7],
                ]);
                bits_per_sample = u16::from_le_bytes([data[body + 14], data[body + 15]]);
            }
            b"data" => {
                raw_data = Some(&data[body..body + chunk_size]);
            }
            _ => {}
        }

        pos = body + chunk_size + (chunk_size % 2);
    }

    if sample_rate == 0 || channels == 0 || bits_per_sample == 0 {
        return Err("WAV 缺少必要的格式信息".to_string());
    }
    let raw_data = raw_data.ok_or_else(|| "WAV 缺少 data 块".to_string())?;

    let mut pcm = Vec::with_capacity(raw_data.len() / 2);
    match bits_per_sample {
        16 => {
            for pair in raw_data.chunks_exact(2) {
                pcm.push(i16::from_le_bytes([pair[0], pair[1]]));
            }
        }
        8 => {
            for &byte in raw_data {
                pcm.push(((i16::from(byte)) - 128) << 8);
            }
        }
        _ => return Err(format!("不支持的位深：{bits_per_sample}")),
    }

    if channels > 1 {
        let frame_count = pcm.len() / channels as usize;
        let mut mono = Vec::with_capacity(frame_count);
        for frame in pcm.chunks_exact(channels as usize) {
            let sum: i32 = frame.iter().map(|&s| i32::from(s)).sum();
            mono.push((sum / i32::from(channels)) as i16);
        }
        pcm = mono;
    }

    if pcm.is_empty() {
        return Err("WAV 中没有音频样本".to_string());
    }

    Ok((sample_rate, pcm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_speech_wav_is_valid_pcm() {
        let (sample_rate, pcm) = load_wav_pcm(TEST_SPEECH_WAV).unwrap();
        assert_eq!(sample_rate, 8000);
        assert!(!pcm.is_empty());
    }

    #[test]
    fn resample_8k_to_16k_preserves_length_ratio() {
        let input = vec![0i16; 800];
        let out = resample_to_16k(&input, 8000);
        assert_eq!(out.len(), 1600);
    }
}

/// Simple linear resampler to the 16 kHz rate used by the voice pipeline.
fn resample_to_16k(samples: &[i16], from_rate: u32) -> Vec<i16> {
    if from_rate == 16_000 {
        return samples.to_vec();
    }

    let out_len = ((samples.len() as f64 * 16_000.0 / f64::from(from_rate)).round() as usize)
        .max(1);
    let step = f64::from(from_rate) / 16_000.0;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let pos = i as f64 * step;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let a = f64::from(samples.get(idx).copied().unwrap_or(0));
        let b = samples
            .get(idx + 1)
            .copied()
            .map(f64::from)
            .unwrap_or(a);
        out.push((a + (b - a) * frac).round() as i16);
    }

    out
}