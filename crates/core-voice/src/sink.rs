//! [`crate::ports::AudioSink`] 的真实实现：`core-audio` 的 CABLE 输出。
//!
//! 只是把具体类型包一层，让桥接层与模拟链路都依赖 trait，而不是直接依赖
//! `core_audio::sink::AudioSink`。

use core_audio::sink::AudioSink as CoreAudioSink;

use crate::ports::AudioSink;

/// 真实音频输出（WASAPI / cpal，目标通常是 `CABLE Input`）。
pub struct CableAudioSink {
    inner: CoreAudioSink,
}

impl CableAudioSink {
    /// 打开指定播放设备（`None` 表示默认输出）。
    pub fn open(device_name: Option<&str>) -> Result<Self, String> {
        let inner = CoreAudioSink::new(device_name).map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }
}

impl AudioSink for CableAudioSink {
    fn push(&self, frames: &[f32]) {
        self.inner.push(frames);
    }

    fn is_failed(&self) -> bool {
        self.inner.is_failed()
    }

    fn last_error(&self) -> Option<String> {
        self.inner.last_error()
    }

    fn format_label(&self) -> String {
        format!(
            "{}Hz/{}ch/{:?}",
            self.inner.sample_rate(),
            self.inner.channels(),
            self.inner.sample_format()
        )
    }
}

/// 打开真实音频输出，返回端口 trait 对象。
pub fn open_sink(device_name: &str) -> Result<Box<dyn AudioSink>, String> {
    Ok(Box::new(CableAudioSink::open(Some(device_name))?))
}
