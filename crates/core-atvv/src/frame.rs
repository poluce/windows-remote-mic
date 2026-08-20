//! Frame accumulation for the BLE audio characteristic.

/// Packs small BLE notifications into complete fixed-length audio frames.
#[derive(Debug, Clone)]
pub struct AudioFrameAssembler {
    frame_len: usize,
    pending: Vec<u8>,
}

impl AudioFrameAssembler {
    pub fn new(frame_len: usize) -> Self {
        Self {
            frame_len,
            pending: Vec::with_capacity(frame_len * 2),
        }
    }

    /// Feed bytes from a BLE notification; returns any complete frames.
    pub fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.pending.extend_from_slice(data);

        let mut frames = Vec::new();
        while self.pending.len() >= self.frame_len {
            let head: Vec<u8> = self.pending.drain(..self.frame_len).collect();
            frames.push(head);
        }

        frames
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_frame() {
        let mut a = AudioFrameAssembler::new(4);
        let frames = a.push(&[1, 2, 3, 4]);
        assert_eq!(frames, vec![vec![1, 2, 3, 4]]);
        assert_eq!(a.pending_len(), 0);
    }

    #[test]
    fn splits_fragmented_frames() {
        let mut a = AudioFrameAssembler::new(4);
        let frames = a.push(&[1, 2]);
        assert!(frames.is_empty());
        let frames = a.push(&[3, 4, 5, 6, 7]);
        assert_eq!(frames, vec![vec![1, 2, 3, 4]]);
        assert_eq!(a.pending_len(), 3); // [5,6,7]
    }
}
