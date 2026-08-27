//! Windows streaming audio sink: play continuous frames into a device
//! (e.g. CABLE Input). Uses cpal/WASAPI with an internal frame queue.

use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use cpal::traits::{DeviceTrait, StreamTrait};

use crate::error::{AudioError, Result};

/// A push-based audio sink that plays 32-bit-float stereo 48 kHz frames.
pub struct AudioSink {
    queue: Arc<Mutex<VecDeque<f32>>>,
    _stream: cpal::Stream,
    started: Arc<AtomicBool>,
}

impl AudioSink {
    /// Open the selected output device (`None` = default).
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
        let err_fn = |e| core_log::log_error(&format!("[audio] stream error: {e}"));

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

    /// Queue frames for playback (48 kHz stereo float).
    pub fn push(&self, frames: &[f32]) {
        if let Ok(mut q) = self.queue.lock() {
            q.extend(frames);
        }
    }

    /// Whether the audio callback has run at least once.
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Relaxed)
    }
}
