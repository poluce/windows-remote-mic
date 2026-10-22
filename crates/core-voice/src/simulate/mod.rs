//! 仅 Windows 的模拟语音链路：无需真实遥控器。
//!
//! 子模块职责：
//! - `tts`：Windows SAPI 中文语音合成（PowerShell）；
//! - `wav`：PCM WAV 解析（16/8 bit、多声道下混）；
//! - `resample`：线性重采样到语音链路使用的 16 kHz。

mod resample;
mod tts;
mod wav;

use core_atvv::protocol::ControlEvent;
use serde::Serialize;
use std::path::Path;

use crate::keys::system_key_sink;
use crate::sink::open_sink;
use crate::VoiceEngine;

use resample::resample_to_16k;
use tts::synthesize_chinese_speech;
use wav::load_wav_pcm;

/// 模拟语音链路的结果。
#[derive(Debug, Clone, Serialize)]
pub struct SimulatedVoiceResult {
    pub frames: usize,
    pub pcm_samples: usize,
    pub output_samples: usize,
    pub win_h_toast: bool,
    pub test_audio: String,
    pub test_audio_ms: u64,
}

/// 运行流程：Win+H 启动 -> StreamStart -> 在远端音频解析阶段之后
/// 注入真实语音 WAV（通过 `VoiceEngine::feed_pcm`）-> CABLE 输出
/// -> StreamStop -> Win+H 停止。
///
/// `test_audio_path` 是可选的。为 `None` 时使用内置的公共领域语音样本；
/// 为 `Some(path)` 时加载指定的 PCM WAV 文件。
pub fn simulate_voice_chain(
    output_device: &str,
    test_audio_path: Option<&str>,
) -> Result<SimulatedVoiceResult, String> {
    let (sample_rate, pcm) = match test_audio_path {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| format!("读取测试音频失败：{e}"))?;
            load_wav_pcm(&bytes)?
        }
        None => {
            let wav_path =
                std::env::temp_dir().join(format!("remote_mic_tts_{}.wav", std::process::id()));
            synthesize_chinese_speech(&wav_path)?;
            let bytes = std::fs::read(&wav_path).map_err(|e| format!("读取 TTS 音频失败：{e}"))?;
            let _ = std::fs::remove_file(&wav_path);
            load_wav_pcm(&bytes)?
        }
    };

    let samples = resample_to_16k(&pcm, sample_rate);
    let duration_ms = (samples.len() as u64 * 1000 / 16_000).max(1);

    let audio_name = test_audio_path
        .and_then(|p| {
            Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "TTS中文语音".to_string());

    core_log::log_line(&format!(
        "[simulate] start: device={}, audio={}, sample_rate={}, pcm_samples={}, duration_ms={}",
        output_device,
        audio_name,
        sample_rate,
        pcm.len(),
        duration_ms
    ));

    let diag = core_audio::diagnostics::run();
    let input_names: Vec<String> = diag
        .input_endpoints
        .iter()
        .map(|e| e.name.clone())
        .collect();
    let default_input = core_audio::endpoint::default_input_name();
    core_log::log_line(&format!(
        "[simulate] diagnostics: has_vb_cable={}, cable_input={}, cable_output={}, default_input={:?}, input_endpoints={:?}",
        diag.has_vb_cable,
        diag.cable_input_present,
        diag.cable_output_present,
        default_input,
        input_names
    ));

    let sink = open_sink(output_device)?;
    core_log::log_debug(&format!(
        "[simulate] audio sink created: {}",
        sink.format_label()
    ));
    let key_sink = system_key_sink();

    let mut engine = VoiceEngine::new();
    engine
        .on_control(ControlEvent::StreamStart)
        .map_err(|e| e.to_string())?;

    // 先关闭可能残留的语音输入，让 Win+H 从已知状态启动。
    let _ = key_sink.press_escape();
    std::thread::sleep(std::time::Duration::from_millis(150));

    core_log::log_line("[simulate] Win+H start");
    key_sink.press_win_h()?;
    // 播放音频前，给 Windows 语音输入一点时间准备就绪。
    std::thread::sleep(std::time::Duration::from_millis(400));

    let chunk = engine.feed_pcm(&samples);
    core_log::log_line(&format!(
        "[simulate] pushed output samples={}, pcm_samples={}",
        chunk.output_samples, chunk.pcm_samples
    ));
    sink.push(&chunk.output);

    // 让 sink 播完语音样本。
    core_log::log_debug(&format!(
        "[simulate] sleeping {} ms for playback",
        duration_ms + 300
    ));
    std::thread::sleep(std::time::Duration::from_millis(duration_ms + 300));

    engine
        .on_control(ControlEvent::StreamStop)
        .map_err(|e| e.to_string())?;
    core_log::log_line("[simulate] close voice typing (Escape)");
    key_sink.press_escape()?;

    core_log::log_line("[simulate] done");
    Ok(SimulatedVoiceResult {
        frames: chunk.complete_frames,
        pcm_samples: chunk.pcm_samples,
        output_samples: chunk.output_samples,
        win_h_toast: true,
        test_audio: audio_name,
        test_audio_ms: duration_ms,
    })
}
