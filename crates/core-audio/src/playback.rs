//! Windows playback of a generated test tone through a selected output device.
//!
//! Uses `cpal` (WASAPI backend on Windows). The tone is short (1 s by default)
//! so it can be used to verify the virtual sound-card route:
//! CABLE Input is selected as the output device, then the tone should be
//! visible on CABLE Output (a.k.a. the virtual microphone).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::error::{AudioError, Result};

/// Play the default 1 kHz / 1 s test tone to an output device.
///
/// `device_name` filters by fuzzy device name (e.g. "CABLE Input");
/// `None` uses the system default output device.
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
    let err_fn = |err| eprintln!("audio stream error: {err}");

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

    // Leave the stream alive a bit longer than the tone so it finishes cleanly.
    std::thread::sleep(Duration::from_millis(1200));
    Ok(())
}

/// Play the test tone several times with a short gap between each run.
pub fn play_test_tone_loop(
    device_name: Option<&str>,
    repetitions: u32,
    gap_ms: u64,
) -> Result<()> {
    let repetitions = repetitions.clamp(1, 20);
    for i in 0..repetitions {
        play_test_tone(device_name)?;
        if i + 1 < repetitions && gap_ms > 0 {
            std::thread::sleep(Duration::from_millis(gap_ms));
        }
    }
    Ok(())
}

pub(crate) fn pick_output_device_public(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    pick_output_device(host, name)
}

fn pick_output_device(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    if let Some(name) = name {
        let name = name.trim();
        if !name.is_empty() {
            for device in host
                .output_devices()
                .map_err(|e| AudioError::Windows(format!("output_devices: {e}")))?
            {
                if let Ok(dev_name) = device.name() {
                    if dev_name.contains(name) {
                        return Ok(device);
                    }
                }
            }
        }
    }

    host.default_output_device()
        .ok_or(AudioError::NoEndpoint)
}
