//! Windows 流式音频输出：将连续帧播放到设备
//!（例如 CABLE Input）。使用 cpal/WASAPI，带内部帧队列。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, StreamTrait};

use crate::error::{AudioError, Result};

/// 输出队列上限（48 kHz 立体声浮点，约 0.5 秒）。
///
/// 播放回调停摆（设备被禁用/拔出、WASAPI 流错误）时，无界队列会以
/// 约 384 KB/s 的速度无限增长；同时也会让语音延迟越积越大。这里封顶后
/// 丢最旧的帧，保证内存与延迟都可控。
const MAX_QUEUED_SAMPLES: usize = 48_000;

/// 基于推送的音频输出，播放 32 位浮点立体声 48 kHz 帧。
pub struct AudioSink {
    queue: Arc<Mutex<VecDeque<f32>>>,
    /// 因队列溢出被丢弃的样本数。
    dropped_samples: Arc<AtomicU64>,
    /// 音频流是否报告过错误（设备丢失等）。
    stream_failed: Arc<AtomicBool>,
    _stream: cpal::Stream,
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

        let supported = device
            .default_output_config()
            .map_err(|e| AudioError::Windows(format!("default_output_config: {e}")))?;

        if supported.sample_format() != cpal::SampleFormat::F32 {
            return Err(AudioError::Windows(
                "sink currently supports 32-bit float output only".into(),
            ));
        }

        let config: cpal::StreamConfig = supported.into();
        let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
        let dropped_samples = Arc::new(AtomicU64::new(0));
        let stream_failed = Arc::new(AtomicBool::new(false));

        let q = queue.clone();
        let err_failed = stream_failed.clone();
        let err_fn = move |e| {
            err_failed.store(true, Ordering::SeqCst);
            core_log::log_error(&format!(
                "[audio] 音频流错误（输出设备可能被禁用/拔出，请检查 CABLE Input）: {e}"
            ));
        };

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if let Ok(mut q) = q.lock() {
                        for sample in data.iter_mut() {
                            *sample = q.pop_front().unwrap_or(0.0);
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| AudioError::Windows(format!("build_output_stream: {e}")))?;

        stream
            .play()
            .map_err(|e| AudioError::Windows(format!("stream.play: {e}")))?;

        Ok(Self {
            queue,
            dropped_samples,
            stream_failed,
            _stream: stream,
        })
    }

    /// 将帧加入播放队列（48 kHz 立体声浮点）。
    ///
    /// 队列超过上限时丢弃最旧的样本，避免设备异常时内存/延迟无界增长。
    pub fn push(&self, frames: &[f32]) {
        if let Ok(mut q) = self.queue.lock() {
            q.extend(frames);
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
    }

    /// 音频流是否发生过错误（设备丢失等）。调用方可据此给出提示。
    pub fn is_failed(&self) -> bool {
        self.stream_failed.load(Ordering::SeqCst)
    }

    /// 被丢弃的样本总数（队列溢出）。
    pub fn dropped_samples(&self) -> u64 {
        self.dropped_samples.load(Ordering::Relaxed)
    }
}
