//! Protocol capture: appends raw BLE Control/Audio bytes to timestamped log files.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Writes raw protocol bytes for later analysis without a real device.
#[derive(Debug, Clone)]
pub struct CaptureRecorder {
    dir: PathBuf,
}

impl CaptureRecorder {
    /// Create a recorder rooted at `dir` (e.g. `.../RemoteMic/RC003/captures`).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    /// Append one observation. `kind` is `"control"` or `"audio"`.
    pub fn record(&self, kind: &str, bytes: &[u8]) {
        let path = self.path_for(kind);
        let line = format!("{} {} {}\n", epoch_ms(), bytes.len(), hex(bytes));
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn path_for(&self, kind: &str) -> PathBuf {
        let date = date_key();
        self.dir.join(format!("{kind}-{date}.log"))
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn date_key() -> String {
    let secs = epoch_ms() / 1000;
    let days = secs / 86_400;
    format!("day{days}")
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02X}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_writes_file() {
        let dir = tempfile::tempdir().unwrap();
        let rec = CaptureRecorder::new(dir.path());
        rec.record("control", &[0x08]);
        rec.record("audio", &[0x11, 0x22]);

        assert!(!rec.path_for("control").exists() == false);
        let text = std::fs::read_to_string(rec.path_for("control")).unwrap();
        assert!(text.contains("08"));
    }
}
