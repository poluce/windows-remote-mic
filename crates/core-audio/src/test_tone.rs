//! Test-tone generation (pure DSP, fully unit-testable).

/// Parameters for generating a test tone.
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

/// Generate `duration` of a sine tone as interleaved samples.
pub fn generate_test_tone(spec: ToneSpec) -> Vec<f32> {
    let channels = spec.channels.max(1) as usize;
    let total_samples = (spec.sample_rate_hz as f32 * spec.duration_secs) as usize;
    let fade_in = (spec.sample_rate_hz as u64 * u64::from(spec.fade_in_ms) / 1000) as usize;
    let fade_out = (spec.sample_rate_hz as u64 * u64::from(spec.fade_out_ms) / 1000) as usize;

    let mut out = Vec::with_capacity(total_samples * channels);
    for i in 0..total_samples {
        let t = i as f32 / spec.sample_rate_hz as f32;
        let mut value = (std::f32::consts::TAU * spec.frequency_hz * t).sin();

        // Fade in / out to avoid clicks.
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

/// Generate a streamable test tone packed as f32 interleaved stereo.
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
        let expected = (spec.sample_rate_hz as f32 * spec.duration_secs) as usize
            * spec.channels as usize;
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
        // First sample should be near zero because of the fade-in.
        assert!(tone[0].abs() < 0.05);
    }

    #[test]
    fn mono_spec_uses_one_channel() {
        let mut spec = ToneSpec::default();
        spec.channels = 1;
        let tone = generate_test_tone(spec);
        assert_eq!(tone.len(), spec.sample_rate_hz as usize);
    }
}
