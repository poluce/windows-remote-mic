//! BLE 音频特征的帧累积。

/// 将小的 BLE 通知打包为完整的定长音频帧。
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

    /// 输入来自 BLE 通知的字节；返回所有完整帧。
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

    /// 热切换帧长（不同固件可能是 120/126/134/160 字节）。
    ///
    /// 帧长变化后旧的半帧数据不再可信，直接丢弃等待重新对齐——
    /// 继续按旧偏移切分会持续产出错位的「伪帧」，听感是噪音。
    pub fn set_frame_len(&mut self, frame_len: usize) {
        if frame_len == 0 || frame_len == self.frame_len {
            return;
        }
        self.frame_len = frame_len;
        self.pending.clear();
        self.pending.reserve(frame_len * 2);
    }

    pub fn frame_len(&self) -> usize {
        self.frame_len
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
