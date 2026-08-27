//! 测试音生成（纯 DSP，完全可单元测试）。

/// 生成测试音的参数。
#[derive(Debug, Clone, Copy)]
pub struct ToneSpec {
    pub frequency_hz: f32,
    pub duration_secs: f32,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
}

impl Default for ToneSpec {
    fn default() -> Self {
        Self {
            frequency_hz: 1_000.0,
            duration_secs: 1.0,
            sample_rate_hz: crate::dsp::TARGET_SAMPLE_RATE,
            channels: crate::OUTPUT_CHANNELS,
            fade_in_ms: 5,
            fade_out_ms: 5,
        }
    }
}

/// 生成指定时长的正弦音，输出为交织采样。
pub fn generate_test_tone(spec: ToneSpec) -> Vec<f32> {
    let channels = spec.channels.max(1) as usize;
    let total_samples = (spec.sample_rate_hz as f32 * spec.duration_secs) as usize;
    let fade_in = (spec.sample_rate_hz as u64 * u64::from(spec.fade_in_ms) / 1000) as usize;
    let fade_out = (spec.sample_rate_hz as u64 * u64::from(spec.fade_out_ms) / 1000) as usize;

    let mut out = Vec::with_capacity(total_samples * channels);
    for i in 0..total_samples {
        let t = i as f32 / spec.sample_rate_hz as f32;
        let mut value = (std::f32::consts::TAU * spec.frequency_hz * t).sin();

        // 淡入/淡出以避免爆音。
        if i < fade_in && fade_in > 0 {
            value *= i as f32 / fade_in as f32;
        }
        let from_end = total_samples - i;
        if from_end <= fade_out && fade_out > 0 {
            value *= from_end as f32 / fade_out as f32;
        }

        for _ in 0..channels {
            out.push(value);
        }
    }
    out
}

/// 生成可流式播放的测试音，打包为 f32 交织立体声。
pub fn default_test_tone() -> Vec<f32> {
    generate_test_tone(ToneSpec::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tone_length_equals_sr_seconds_channels() {
        let spec = ToneSpec::default();
        let tone = generate_test_tone(spec);
        let expected =
            (spec.sample_rate_hz as f32 * spec.duration_secs) as usize * spec.channels as usize;
        assert_eq!(tone.len(), expected);
    }

    #[test]
    fn samples_stay_in_unit_range() {
        let tone = default_test_tone();
        assert!(tone.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn fade_in_avoids_click() {
        let tone = default_test_tone();
        // 由于淡入，第一个采样应接近零。
        assert!(tone[0].abs() < 0.05);
    }

    #[test]
    fn mono_spec_uses_one_channel() {
        let spec = ToneSpec {
            channels: 1,
            ..Default::default()
        };
        let tone = generate_test_tone(spec);
        assert_eq!(tone.len(), spec.sample_rate_hz as usize);
    }
}
