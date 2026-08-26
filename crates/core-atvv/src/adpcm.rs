//! IMA/DVI ADPCM decoder used by the RC003 16 kHz voice channel.

/// IMA ADPCM step size table (89 entries).
pub const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41,
    45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190,
    209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724,
    796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132,
    7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500,
    20350, 22385, 24623, 27086, 29794, 32767,
];

/// IMA ADPCM index adjustment table.
pub const INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// Streaming decoder that carries predictor + step index between frames.
#[derive(Debug, Clone, Default)]
pub struct ImaAdpcmDecoder {
    predictor: i32,
    step_index: i32,
}

impl ImaAdpcmDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset predictor and index (e.g. on a sync frame / stream start).
    pub fn reset(&mut self) {
        self.predictor = 0;
        self.step_index = 0;
    }

    pub fn predictor(&self) -> i32 {
        self.predictor
    }

    pub fn step_index(&self) -> i32 {
        self.step_index
    }

    /// Decode one 4-bit nibble into a 16-bit PCM sample.
    pub fn decode_nibble(&mut self, nibble: u8) -> i16 {
        let nibble = nibble & 0x0F;
        let idx = self.step_index.clamp(0, (STEP_TABLE.len() - 1) as i32);
        let step = STEP_TABLE[idx as usize];

        let mut diff = step >> 3;
        if nibble & 0x01 != 0 {
            diff += step >> 2;
        }
        if nibble & 0x02 != 0 {
            diff += step >> 1;
        }
        if nibble & 0x04 != 0 {
            diff += step;
        }

        let mut pred = self.predictor;
        if nibble & 0x08 != 0 {
            pred -= diff;
        } else {
            pred += diff;
        }
        pred = pred.clamp(i32::from(i16::MIN), i32::from(i16::MAX));

        self.step_index = (self.step_index + INDEX_TABLE[nibble as usize])
            .clamp(0, (STEP_TABLE.len() - 1) as i32);
        self.predictor = pred;

        pred as i16
    }

    /// Decode a byte as two 16-bit PCM samples, **high nibble first**.
    pub fn decode_byte(&mut self, byte: u8) -> [i16; 2] {
        [self.decode_nibble(byte >> 4), self.decode_nibble(byte & 0x0F)]
    }

    /// Decode a whole buffer, producing 2 samples per byte.
    pub fn decode_bytes(&mut self, data: &[u8]) -> Vec<i16> {
        let mut out = Vec::with_capacity(data.len() * 2);
        for &byte in data {
            let pair = self.decode_byte(byte);
            out.push(pair[0]);
            out.push(pair[1]);
        }
        out
    }
}

/// Streaming IMA ADPCM encoder used to build simulated voice/test vectors.
#[derive(Debug, Clone, Default)]
pub struct ImaAdpcmEncoder {
    predictor: i32,
    step_index: i32,
}

impl ImaAdpcmEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.predictor = 0;
        self.step_index = 0;
    }

    pub fn encode_nibble(&mut self, sample: i16) -> u8 {
        let step = STEP_TABLE[self.step_index.clamp(0, STEP_TABLE.len() as i32 - 1) as usize];
        let mut temp = i32::from(sample) - self.predictor;
        let mut nibble = 0u8;
        if temp < 0 {
            nibble = 8;
            temp = -temp;
        }
        let mut step_local = step;
        if temp >= step_local {
            nibble |= 4;
            temp -= step_local;
        }
        step_local >>= 1;
        if temp >= step_local {
            nibble |= 2;
            temp -= step_local;
        }
        step_local >>= 1;
        if temp >= step_local {
            nibble |= 1;
        }

        let step = STEP_TABLE[self.step_index.clamp(0, STEP_TABLE.len() as i32 - 1) as usize];
        let diff = compute_diff(nibble, step);
        if nibble & 8 != 0 {
            self.predictor -= diff;
        } else {
            self.predictor += diff;
        }
        self.predictor = self.predictor.clamp(-32768, 32767);
        self.step_index =
            (self.step_index + INDEX_TABLE[nibble as usize]).clamp(0, 88);
        nibble
    }

    pub fn encode(&mut self, samples: &[i16]) -> Vec<u8> {
        let mut out = Vec::with_capacity(samples.len() / 2 + 1);
        for chunk in samples.chunks(2) {
            let high = self.encode_nibble(chunk[0]);
            let low = if chunk.len() > 1 {
                self.encode_nibble(chunk[1])
            } else {
                // Re-encode the last sample for the low nibble to keep
                // decoder state aligned.
                self.encode_nibble(chunk[0])
            };
            out.push((high << 4) | low);
        }
        out
    }
}

fn compute_diff(nibble: u8, step: i32) -> i32 {
    let mut diff = step >> 3;
    if nibble & 1 != 0 {
        diff += step >> 2;
    }
    if nibble & 2 != 0 {
        diff += step >> 1;
    }
    if nibble & 4 != 0 {
        diff += step;
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn decode_byte_produces_two_samples() {
        let mut decoder = ImaAdpcmDecoder::new();
        let pair = decoder.decode_byte(0x4C);
        assert_eq!(pair.len(), 2);
    }

    #[test]
    fn decode_len_is_twice_input() {
        let mut decoder = ImaAdpcmDecoder::new();
        let out = decoder.decode_bytes(&[0x00, 0x11, 0x22, 0x33]);
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn round_trip_smooth_sine_is_close() {
        let mut encoder = ImaAdpcmEncoder::new();
        // Smooth sine avoids the huge slew at a sawtooth wraparound, which is
        // where IMA adaptation legitimately lags.
        let samples: Vec<i16> = (0..800)
            .map(|i| {
                let t = i as f32 / 800.0 * std::f32::consts::TAU * 4.0;
                (t.sin() * 900.0) as i16
            })
            .collect();
        let bytes = encoder.encode(&samples);

        let mut decoder = ImaAdpcmDecoder::new();
        let decoded = decoder.decode_bytes(&bytes);
        assert_eq!(decoded.len(), samples.len());

        let mut max_err = 0i32;
        for (src, dst) in samples.iter().zip(&decoded) {
            let err = (i32::from(*src) - i32::from(*dst)).abs();
            max_err = max_err.max(err);
        }
        // IMA has bounded steady-state error for smooth signals.
        assert!(max_err < 96, "max sample error too large: {max_err}");
    }

    #[test]
    fn reset_clears_predictor() {
        let mut decoder = ImaAdpcmDecoder::new();
        decoder.decode_bytes(&[0xAA, 0x55]);
        assert!(decoder.predictor() != 0);
        decoder.reset();
        assert_eq!(decoder.predictor(), 0);
        assert_eq!(decoder.step_index(), 0);
    }
}
