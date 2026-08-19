//! Portable DSP used before writing decoded remote audio to the output.

/// Remote (RC003) voice sample rate.
pub const REMOTE_SAMPLE_RATE: u32 = 16_000;
/// Sample rate Windows apps commonly prefer (also what VB-CABLE endpoints use).
pub const TARGET_SAMPLE_RATE: u32 = 48_000;

/// 16 kHz -> 48 kHz linear-interpolation resampler (factor = 3).
pub fn resample_16k_to_48k(input: &[f32]) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }

    let out_len = input.len() * 3;
    let mut out = Vec::with_capacity(out_len);

    for n in 0..out_len {
        // Map output sample n back to a 16k time position.
        let pos = n as f32 / 3.0; // 0..input.len()-1
        let i0 = pos.floor() as usize;
        let frac = pos - i0 as f32;

        let s0 = input[i0.min(input.len() - 1)];
        let s1 = input[(i0 + 1).min(input.len() - 1)];
        out.push(s0 + (s1 - s0) * frac);
    }

    out
}

/// Apply a fixed gain in dB (e.g. +10 dB) in place.
pub fn apply_gain_db(samples: &mut [f32], db: f32) {
    let linear = 10f32.powf(db / 20.0);
    for s in samples.iter_mut() {
        *s *= linear;
    }
}

/// One-pole high-pass (DC blocker) ~20 Hz.
#[derive(Debug, Clone, Copy)]
pub struct DcBlock {
    x_prev: f32,
    y_prev: f32,
}

impl Default for DcBlock {
    fn default() -> Self {
        Self {
            x_prev: 0.0,
            y_prev: 0.0,
        }
    }
}

impl DcBlock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter coefficient (≈20 Hz at 48 kHz). A larger R lowers the cutoff.
    const R: f32 = 0.995;

    /// Process one sample and return the filtered value.
    pub fn process(&mut self, sample: f32) -> f32 {
        let y = sample - self.x_prev + Self::R * self.y_prev;
        self.x_prev = sample;
        self.y_prev = y;
        y
    }

    pub fn process_batch(&mut self, samples: &mut [f32]) {
        for s in samples.iter_mut() {
            *s = self.process(*s);
        }
    }
}

/// Duplicate mono samples into the given channel count (e.g. stereo).
pub fn to_channels(input: &[f32], channels: u16) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    let mut out = Vec::with_capacity(input.len() * channels);
    for &s in input {
        for _ in 0..channels {
            out.push(s);
        }
    }
    out
}

/// Clamp samples to avoid clipping.
pub fn hard_limiter(samples: &mut [f32], limit: f32) {
    for s in samples.iter_mut() {
        *s = s.clamp(-limit, limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_triples_length() {
        let input = vec![0.0, 1.0, 0.0];
        let out = resample_16k_to_48k(&input);
        assert_eq!(out.len(), input.len() * 3);
    }

    #[test]
    fn resample_keeps_bounds() {
        let input = vec![-1.0, 0.5, 1.0];
        let out = resample_16k_to_48k(&input);
        assert!(out.iter().all(|s| (-1.0..=1.0).contains(s)));
    }

    #[test]
    fn gain_is_monotonic() {
        let mut a = vec![0.1, 0.2];
        let mut b = vec![0.1, 0.2];
        apply_gain_db(&mut a, 0.0);
        apply_gain_db(&mut b, 10.0);
        assert!(b.iter().zip(&a).all(|(x, y)| x > y));
    }

    #[test]
    fn dc_block_stabilizes() {
        let mut block = DcBlock::new();
        let mut samples = vec![1.0; 1000];
        block.process_batch(&mut samples);
        let tail = samples[samples.len() - 10..].to_vec();
        assert!(tail.iter().all(|s| s.abs() < 0.01));
    }

    #[test]
    fn mono_to_stereo_duplicates() {
        let input = vec![0.1, 0.2];
        let out = to_channels(&input, 2);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2]);
    }
}
