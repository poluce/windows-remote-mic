//! Windows 流式音频输出：将连续帧播放到设备
//!（例如 CABLE Input）。使用 cpal/WASAPI，带内部帧队列。
//!
//! # 格式协商
//!
//! 语音引擎固定输出 **48 kHz / 立体声 / f32**。真实设备（尤其用户手工改过
//! 格式的 CABLE Input）可能不是这个组合：
//! - 优先选设备支持的 48k/2ch/f32 配置；
//! - 退而求其次选受支持的 48k 配置（f32/i16/u16）；
//! - 采样率不是 48k 时做**软件线性重采样**（否则会变调）；
//! - 声道数不是 2 时做混音/补齐（否则会静音或速度异常）。
//!
//! 早期实现只接受 f32 且直接用默认配置：设备格式一变就要么报错、要么变调，
//! 而且没有任何提示。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::error::{AudioError, Result};

/// 语音引擎输出格式：48 kHz、立体声、f32。
const SOURCE_RATE: u32 = 48_000;
const SOURCE_CHANNELS: usize = 2;

/// 输出队列上限（按输出设备的样本数计，约 0.5 秒@48k）。
///
/// 播放回调停摆（设备被禁用/拔出、WASAPI 流错误）时，无界队列会以
/// 约 384 KB/s 的速度无限增长；同时也会让语音延迟越积越大。这里封顶后
/// 丢最旧的帧，保证内存与延迟都可控。
const MAX_QUEUED_SAMPLES: usize = 48_000;

/// 基于推送的音频输出（48 kHz 立体声 f32 输入，自动适配设备格式）。
pub struct AudioSink {
    queue: Arc<Mutex<VecDeque<f32>>>,
    /// 因队列溢出被丢弃的样本数。
    dropped_samples: Arc<AtomicU64>,
    /// 音频流是否报告过错误（设备丢失等）。
    stream_failed: Arc<AtomicBool>,
    /// 最近一次流错误的可读描述。
    last_error: Arc<Mutex<Option<String>>>,
    /// 输出设备采样率。
    out_rate: u32,
    /// 输出设备声道数。
    out_channels: u16,
    /// 输出设备采样格式。
    out_format: SampleFormat,
    /// 重采样 / 声道映射状态。
    converter: Mutex<Converter>,
    _stream: cpal::Stream,
}

/// 输入（48k 立体声）→ 输出（设备格式）的转换状态。
#[derive(Debug)]
struct Converter {
    resampler: Option<LinearResampler>,
    out_channels: u16,
}

impl AudioSink {
    /// 打开选定的输出设备（`None` = 默认设备）。
    ///
    /// 显式指定设备名时**严格匹配**：找不到就返回错误，绝不静默回退到
    /// 系统默认输出（否则语音会从扬声器放出来且用户无从察觉）。
    pub fn new(device_name: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();
        let device = match device_name {
            Some(name) if !name.trim().is_empty() => {
                super::playback::find_output_device_by_name(&host, name.trim()).ok_or_else(
                    || {
                        AudioError::Windows(format!(
                    "找不到输出设备「{name}」：请确认 VB-CABLE 已安装且 CABLE Input 未被禁用"
                ))
                    },
                )?
            }
            _ => super::playback::pick_output_device_public(&host, None)?,
        };

        let supported = choose_stream_config(&device)?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let out_rate = config.sample_rate.0;
        let out_channels = config.channels;

        if out_rate != SOURCE_RATE || out_channels as usize != SOURCE_CHANNELS {
            core_log::log_warn(&format!(
                "[audio] 输出设备格式与语音引擎不同（设备 {out_rate}Hz/{out_channels}ch/{sample_format:?}，引擎 {SOURCE_RATE}Hz/{SOURCE_CHANNELS}ch/f32）——已启用软件重采样/声道适配"
            ));
        } else {
            core_log::log_info(&format!(
                "[audio] 输出设备格式匹配：{out_rate}Hz/{out_channels}ch/{sample_format:?}"
            ));
        }

        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let dropped_samples = Arc::new(AtomicU64::new(0));
        let stream_failed = Arc::new(AtomicBool::new(false));
        let last_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        let stream = build_typed_stream(
            &device,
            &config,
            sample_format,
            queue.clone(),
            stream_failed.clone(),
            last_error.clone(),
        )?;

        stream
            .play()
            .map_err(|e| AudioError::Windows(format!("stream.play: {e}")))?;

        let converter = Converter {
            resampler: (out_rate != SOURCE_RATE)
                .then(|| LinearResampler::new(SOURCE_RATE, out_rate)),
            out_channels,
        };

        Ok(Self {
            queue,
            dropped_samples,
            stream_failed,
            last_error,
            out_rate,
            out_channels,
            out_format: sample_format,
            converter: Mutex::new(converter),
            _stream: stream,
        })
    }

    /// 将帧加入播放队列（固定 48 kHz 立体声浮点，交错排列）。
    ///
    /// 内部会按设备格式重采样/声道映射；队列超过上限时丢弃最旧的样本，
    /// 避免设备异常时内存/延迟无界增长。
    pub fn push(&self, frames: &[f32]) {
        if frames.is_empty() {
            return;
        }

        // 最常见路径（CABLE 默认 48k/2ch）：零转换直接入队。
        if self.out_rate == SOURCE_RATE && self.out_channels as usize == SOURCE_CHANNELS {
            self.enqueue(frames);
            return;
        }

        let mut converted: Vec<f32> = Vec::with_capacity(frames.len());
        {
            let mut converter = match self.converter.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            let resampled: Vec<f32> = match converter.resampler.as_mut() {
                Some(resampler) => {
                    let mut out = Vec::with_capacity(frames.len());
                    resampler.process(frames, &mut out);
                    out
                }
                None => frames.to_vec(),
            };
            map_channels(&resampled, converter.out_channels, &mut converted);
        }
        self.enqueue(&converted);
    }

    fn enqueue(&self, samples: &[f32]) {
        let mut q = match self.queue.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        q.extend(samples.iter().copied());
        if q.len() > MAX_QUEUED_SAMPLES {
            let overflow = q.len() - MAX_QUEUED_SAMPLES;
            let dropped_before = self
                .dropped_samples
                .fetch_add(overflow as u64, Ordering::Relaxed);
            q.drain(..overflow);
            if dropped_before == 0 {
                core_log::log_warn(&format!(
                    "[audio] 输出队列积压超过 {} 样本，开始丢弃最旧音频（设备播放异常？）",
                    MAX_QUEUED_SAMPLES
                ));
            }
        }
    }

    /// 音频流是否发生过错误（设备丢失等）。调用方可据此给出提示/重建。
    pub fn is_failed(&self) -> bool {
        self.stream_failed.load(Ordering::SeqCst)
    }

    /// 最近一次音频流错误的描述（没有则 None）。
    pub fn last_error(&self) -> Option<String> {
        match self.last_error.lock() {
            Ok(g) => g.clone(),
            Err(e) => e.into_inner().clone(),
        }
    }

    /// 被丢弃的样本总数（队列溢出）。
    pub fn dropped_samples(&self) -> u64 {
        self.dropped_samples.load(Ordering::Relaxed)
    }

    /// 输出设备采样率（Hz）。
    pub fn sample_rate(&self) -> u32 {
        self.out_rate
    }

    /// 输出设备声道数。
    pub fn channels(&self) -> u16 {
        self.out_channels
    }

    /// 输出设备采样格式。
    pub fn sample_format(&self) -> SampleFormat {
        self.out_format
    }
}

fn is_preferred_format(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
    )
}

fn supports_rate(range: &cpal::SupportedStreamConfigRange, rate: u32) -> bool {
    range.min_sample_rate().0 <= rate && rate <= range.max_sample_rate().0
}

fn clamp_rate(range: &cpal::SupportedStreamConfigRange, rate: u32) -> u32 {
    rate.clamp(range.min_sample_rate().0, range.max_sample_rate().0)
}

/// 选择尽量接近「48k / 2ch / f32」且设备支持的配置。
fn choose_stream_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig> {
    let default = device
        .default_output_config()
        .map_err(|e| AudioError::Windows(format!("读取输出设备默认格式失败: {e}")))?;

    // 已经是最理想组合。
    if default.sample_rate().0 == SOURCE_RATE
        && default.channels() as usize == SOURCE_CHANNELS
        && default.sample_format() == SampleFormat::F32
    {
        return Ok(default);
    }

    let ranges: Vec<cpal::SupportedStreamConfigRange> = match device.supported_output_configs() {
        Ok(iter) => iter.collect(),
        Err(_) => Vec::new(),
    };

    // 1) 48k / 2ch / f32
    for range in &ranges {
        if range.channels() as usize == SOURCE_CHANNELS
            && range.sample_format() == SampleFormat::F32
            && supports_rate(range, SOURCE_RATE)
        {
            return Ok((*range).with_sample_rate(cpal::SampleRate(SOURCE_RATE)));
        }
    }

    // 2) 48k / 任意声道 / 受支持格式（声道由软件映射）
    for range in &ranges {
        if is_preferred_format(range.sample_format()) && supports_rate(range, SOURCE_RATE) {
            return Ok((*range).with_sample_rate(cpal::SampleRate(SOURCE_RATE)));
        }
    }

    // 3) 默认格式若受支持，就直接用它（后续软件重采样/声道映射兜底）
    if is_preferred_format(default.sample_format()) {
        return Ok(default);
    }

    // 4) 最后从支持列表里挑一个受支持格式（用默认采样率夹到范围内）
    let fallback = ranges
        .iter()
        .find(|r| is_preferred_format(r.sample_format()))
        .map(|range| {
            let rate = clamp_rate(range, default.sample_rate().0);
            (*range).with_sample_rate(cpal::SampleRate(rate))
        });
    match fallback {
        Some(cfg) => Ok(cfg),
        None => Err(AudioError::Windows(format!(
            "输出设备不支持 f32/i16/u16 采样格式（默认格式 {:?}）",
            default.sample_format()
        ))),
    }
}

fn build_typed_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: SampleFormat,
    queue: Arc<Mutex<VecDeque<f32>>>,
    stream_failed: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream> {
    match format {
        SampleFormat::F32 => build_stream::<f32>(device, config, queue, stream_failed, last_error),
        SampleFormat::I16 => build_stream::<i16>(device, config, queue, stream_failed, last_error),
        SampleFormat::U16 => build_stream::<u16>(device, config, queue, stream_failed, last_error),
        other => Err(AudioError::Windows(format!(
            "不支持的输出采样格式: {other:?}"
        ))),
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    queue: Arc<Mutex<VecDeque<f32>>>,
    stream_failed: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let err_fn = move |e: cpal::StreamError| {
        stream_failed.store(true, Ordering::SeqCst);
        let message = format!("音频流错误（输出设备可能被禁用/拔出，请检查 CABLE Input）: {e}");
        core_log::log_error(&format!("[audio] {message}"));
        if let Ok(mut slot) = last_error.lock() {
            *slot = Some(message);
        }
    };

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let mut q = match queue.lock() {
                    Ok(g) => g,
                    Err(e) => e.into_inner(),
                };
                for sample in data.iter_mut() {
                    *sample = T::from_sample(q.pop_front().unwrap_or(0.0));
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| AudioError::Windows(format!("创建输出流失败: {e}")))
}

/// 交错立体声 → 设备声道数的交错样本。
fn map_channels(stereo: &[f32], out_channels: u16, out: &mut Vec<f32>) {
    match out_channels {
        1 => out.extend(
            stereo
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| (c[0] + c[1]) * 0.5),
        ),
        2 => out.extend_from_slice(stereo),
        n => {
            for c in stereo.as_chunks::<2>().0 {
                out.push(c[0]);
                out.push(c[1]);
                for _ in 2..n {
                    out.push(0.0);
                }
            }
        }
    }
}

/// 线性插值重采样器（块间用上一块的最后一帧做前驱，避免边界爆音）。
#[derive(Debug)]
struct LinearResampler {
    /// 每个输出样本对应的输入帧步进（in_rate / out_rate）。
    step: f64,
    /// 当前输出样本在「本块帧空间」中的位置。
    pos: f64,
    /// 上一块最后一帧。
    prev: Option<[f32; 2]>,
}

impl LinearResampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            step: f64::from(in_rate) / f64::from(out_rate.max(1)),
            pos: 0.0,
            prev: None,
        }
    }

    /// 输入交错立体声 f32，输出重采样后的交错立体声 f32。
    fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let frames: Vec<[f32; 2]> = input
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| [c[0], c[1]])
            .collect();
        if frames.is_empty() {
            return;
        }
        let n = frames.len();
        let mut data: Vec<[f32; 2]> = Vec::with_capacity(n + 1);
        data.push(self.prev.unwrap_or(frames[0]));
        data.extend_from_slice(&frames);

        while self.pos < n as f64 {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = data[i];
            let b = data[i + 1];
            out.push(a[0] + (b[0] - a[0]) * frac);
            out.push(a[1] + (b[1] - a[1]) * frac);
            self.pos += self.step;
        }
        self.pos -= n as f64;
        self.prev = Some(frames[n - 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_downmix_to_mono() {
        let stereo = [1.0, -1.0, 0.5, 0.5];
        let mut out = Vec::new();
        map_channels(&stereo, 1, &mut out);
        assert_eq!(out, vec![0.0, 0.5]);
    }

    #[test]
    fn channels_passthrough_for_stereo() {
        let stereo = [1.0, 2.0, 3.0, 4.0];
        let mut out = Vec::new();
        map_channels(&stereo, 2, &mut out);
        assert_eq!(out, stereo.to_vec());
    }

    #[test]
    fn channels_pad_extra_channels_with_silence() {
        let stereo = [1.0, 2.0];
        let mut out = Vec::new();
        map_channels(&stereo, 4, &mut out);
        assert_eq!(out, vec![1.0, 2.0, 0.0, 0.0]);
    }

    #[test]
    fn resampler_halves_sample_count_for_half_rate() {
        // 48k -> 24k：输出样本数应约为输入的一半（允许 1 帧误差）。
        let mut r = LinearResampler::new(48_000, 24_000);
        let mut input = Vec::new();
        for i in 0..480 {
            let v = i as f32 / 480.0;
            input.push(v);
            input.push(v);
        }
        let mut out = Vec::new();
        r.process(&input, &mut out);
        let out_frames = out.len() / 2;
        assert!(
            (out_frames as i64 - 240).abs() <= 2,
            "期望约 240 帧，实际 {out_frames}"
        );
    }

    #[test]
    fn resampler_is_continuous_across_blocks() {
        // 分两块喂入同一个线性信号，块边界不应出现大幅跳变。
        let mut r = LinearResampler::new(48_000, 44_100);
        let make_block = |start: usize| -> Vec<f32> {
            let mut v = Vec::new();
            for i in start..start + 240 {
                let s = i as f32 * 0.01;
                v.push(s);
                v.push(s);
            }
            v
        };
        let mut out = Vec::new();
        r.process(&make_block(0), &mut out);
        r.process(&make_block(240), &mut out);
        let left: Vec<f32> = out.as_chunks::<2>().0.iter().map(|c| c[0]).collect();
        for w in left.windows(2) {
            assert!(
                (w[1] - w[0]).abs() < 0.05,
                "块边界出现跳变: {:?} -> {:?}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn resampler_keeps_count_at_same_rate() {
        let mut r = LinearResampler::new(48_000, 48_000);
        let input = [0.0, 0.0, 0.5, 0.5, 1.0, 1.0];
        let mut out = Vec::new();
        r.process(&input, &mut out);
        let out_frames = out.len() / 2;
        assert!(
            (out_frames as i64 - 3).abs() <= 1,
            "同采样率应近似直通，实际 {out_frames} 帧"
        );
    }
}
