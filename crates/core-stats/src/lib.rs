//! core-stats — local-only usage statistics (key presses, voice time).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// One day of local statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyStats {
    /// Physical button id / logical action name -> press count.
    pub key_presses: HashMap<String, u64>,
    /// Total voice input seconds.
    pub voice_seconds: u64,
    /// Number of voice sessions started.
    pub voice_sessions: u64,
}

/// Store rooted at a directory (e.g. `%LOCALAPPDATA%\RemoteMic\RC003`).
#[derive(Debug, Clone)]
pub struct StatsStore {
    pub dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum StatsError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StatsError>;

impl StatsStore {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self) -> PathBuf {
        self.dir.join("stats.json")
    }

    fn date_key() -> String {
        Self::date_key_now()
    }

    /// Current local day key (day-count from epoch; a real impl should use
    /// the local calendar date as YYYY-MM-DD).
    pub fn date_key_now() -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = secs / 86_400;
        format!("{days}")
    }

    pub fn load(&self) -> Result<HashMap<String, DailyStats>> {
        let path = self.path();
        if !path.exists() {
            return Ok(HashMap::new());
        }
        Ok(serde_json::from_str(&fs::read_to_string(&path)?)?)
    }

    pub fn save(&self, stats: &HashMap<String, DailyStats>) -> Result<()> {
        let tmp = self.dir.join("stats.json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(stats)?)?;
        fs::rename(&tmp, self.path())?;
        Ok(())
    }

    pub fn record_key(&self, key: &str) -> Result<()> {
        let mut stats = self.load()?;
        let day = stats.entry(Self::date_key()).or_default();
        *day.key_presses.entry(key.to_string()).or_insert(0) += 1;
        self.save(&stats)
    }

    pub fn record_voice(&self, seconds: u64) -> Result<()> {
        let mut stats = self.load()?;
        let day = stats.entry(Self::date_key()).or_default();
        day.voice_seconds += seconds;
        day.voice_sessions += 1;
        self.save(&stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, StatsStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = StatsStore::new(dir.path()).unwrap();
        (dir, s)
    }

    #[test]
    fn record_key_and_voice_roundtrip() {
        let (_d, s) = store();
        s.record_key("up").unwrap();
        s.record_key("up").unwrap();
        s.record_voice(12).unwrap();

        let stats = s.load().unwrap();
        let day = stats.values().next().unwrap();
        assert_eq!(day.key_presses.get("up"), Some(&2));
        assert_eq!(day.voice_seconds, 12);
        assert_eq!(day.voice_sessions, 1);
    }
}
