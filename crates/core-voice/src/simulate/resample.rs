//! 线性重采样到语音链路使用的 16 kHz。

/// 简单的线性重采样器，转换为语音链路使用的 16 kHz 采样率。
pub(super) fn resample_to_16k(samples: &[i16], from_rate: u32) -> Vec<i16> {
    if from_rate == 16_000 {
        return samples.to_vec();
    }

    let out_len =
        ((samples.len() as f64 * 16_000.0 / f64::from(from_rate)).round() as usize).max(1);
    let step = f64::from(from_rate) / 16_000.0;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let pos = i as f64 * step;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let a = f64::from(samples.get(idx).copied().unwrap_or(0));
        let b = samples.get(idx + 1).copied().map(f64::from).unwrap_or(a);
        out.push((a + (b - a) * frac).round() as i16);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_8k_to_16k_preserves_length_ratio() {
        let input = vec![0i16; 800];
        let out = resample_to_16k(&input, 8000);
        assert_eq!(out.len(), 1600);
    }
}
