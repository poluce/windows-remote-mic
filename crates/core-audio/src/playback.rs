//! 通过选定的输出设备播放生成的测试音（Windows）。
//!
//! 使用 `cpal`（Windows 上为 WASAPI 后端）。测试音很短（默认 1 秒），
//! 可用于验证虚拟声卡链路：
//! 选择 CABLE Input 作为输出设备后，应该能在 CABLE Output
//!（即虚拟麦克风）上看到测试音。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::error::{AudioError, Result};

/// 向输出设备播放默认 1 kHz / 1 秒测试音。
///
/// `device_name` 按模糊设备名过滤（例如 "CABLE Input"）；
/// `None` 使用系统默认输出设备。
pub fn play_test_tone(device_name: Option<&str>) -> Result<()> {
    let host = cpal::default_host();
    let device = pick_output_device(&host, device_name)?;

    let supported = device
        .default_output_config()
        .map_err(|e| AudioError::Windows(format!("default_output_config: {e}")))?;

    if supported.sample_format() != cpal::SampleFormat::F32 {
        return Err(AudioError::Windows(
            "test tone currently supports 32-bit float output only".into(),
        ));
    }

    let config: cpal::StreamConfig = supported.into();
    let tone = Arc::new(crate::test_tone::default_test_tone());
    let pos = Arc::new(AtomicUsize::new(0));

    let tone_play = tone.clone();
    let pos_play = pos.clone();
    let err_fn = |err| core_log::log_error(&format!("[audio] 音频流错误: {err}"));

    let stream = device
        .build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if tone_play.is_empty() {
                    return;
                }
                for sample in data.iter_mut() {
                    let i = pos_play.fetch_add(1, Ordering::Relaxed);
                    *sample = tone_play[i % tone_play.len()];
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| AudioError::Windows(format!("build_output_stream: {e}")))?;

    stream
        .play()
        .map_err(|e| AudioError::Windows(format!("stream.play: {e}")))?;

    // 让流比测试音多存活一会儿，以便干净地结束。
    std::thread::sleep(Duration::from_millis(1200));
    Ok(())
}

/// 多次播放测试音，每次之间短暂间隔。
pub fn play_test_tone_loop(device_name: Option<&str>, repetitions: u32, gap_ms: u64) -> Result<()> {
    let repetitions = repetitions.clamp(1, 20);
    for i in 0..repetitions {
        play_test_tone(device_name)?;
        if i + 1 < repetitions && gap_ms > 0 {
            std::thread::sleep(Duration::from_millis(gap_ms));
        }
    }
    Ok(())
}

pub(crate) fn pick_output_device_public(
    host: &cpal::Host,
    name: Option<&str>,
) -> Result<cpal::Device> {
    pick_output_device(host, name)
}

/// 按名称严格查找输出设备：找不到返回 `None`，**不**回退默认设备。
///
/// 语音输出链路必须用这个版本：名称写的是 `CABLE Input` 时如果静默回退到
/// 系统默认播放设备，声音会从扬声器放出来，而豆包/语音输入完全收不到。
pub(crate) fn find_output_device_by_name(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    for device in host.output_devices().ok()? {
        if let Ok(dev_name) = device.name() {
            if dev_name.contains(name) {
                return Some(device);
            }
        }
    }
    None
}

fn pick_output_device(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    if let Some(name) = name {
        if let Some(device) = find_output_device_by_name(host, name) {
            return Ok(device);
        }
        core_log::log_warn(&format!(
            "[audio] 未找到名为「{name}」的输出设备，测试音将回退到系统默认输出"
        ));
    }

    host.default_output_device().ok_or(AudioError::NoEndpoint)
}
