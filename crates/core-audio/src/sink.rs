//! Windows 流式音频输出：将连续帧播放到设备
//!（例如 CABLE Input）。使用 cpal/WASAPI，带内部帧队列。

use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use cpal::traits::{DeviceTrait, StreamTrait};

use crate::error::{AudioError, Result};

/// 基于推送的音频输出，播放 32 位浮点立体声 48 kHz 帧。
pub struct AudioSink {
    queue: Arc<Mutex<VecDeque<f32>>>,
    _stream: cpal::Stream,
    started: Arc<AtomicBool>,
}

impl AudioSink {
    /// 打开选定的输出设备（`None` = 默认设备）。
    pub fn new(device_name: Option<&str>) -> Result<Self> {
        let host = cpal::default_host();
        let device = super::playback::pick_output_device_public(&host, device_name)?;

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
        let started = Arc::new(AtomicBool::new(false));

        let q = queue.clone();
        let started_play = started.clone();
        let err_fn = |e| core_log::log_error(&format!("[audio] 音频流错误: {e}"));

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    started_play.store(true, Ordering::Relaxed);
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
            _stream: stream,
            started,
        })
    }

    /// 将帧加入播放队列（48 kHz 立体声浮点）。
    pub fn push(&self, frames: &[f32]) {
        if let Ok(mut q) = self.queue.lock() {
            q.extend(frames);
        }
    }

    /// 音频回调是否至少运行过一次。
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Relaxed)
    }
}
