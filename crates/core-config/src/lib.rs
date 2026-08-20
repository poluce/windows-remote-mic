//! core-config — application configuration persistence.
//!
//! Writes are atomic (temp file + fsync + rename) so a crash never leaves a
//! half-written JSON.

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use core_mapping::MappingConfig;

/// Voice input target. We start with Windows built-in voice typing (Win+H).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceMode {
    WindowsVoiceTyping,
    // ImeDoubao, ImeWeChat — reserved for later.
}

/// Top-level application configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub selected_device_id: Option<String>,
    pub output_endpoint_id: Option<String>,
    pub voice_mode: VoiceMode,
    pub gain_db: f32,
    pub auto_reconnect: bool,
    pub mapping: MappingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            selected_device_id: None,
            output_endpoint_id: None,
            voice_mode: VoiceMode::WindowsVoiceTyping,
            gain_db: 10.0,
            auto_reconnect: true,
            mapping: MappingConfig::default(),
        }
    }
}

/// Configuration store bound to a directory (e.g. `%LOCALAPPDATA%\RemoteMic\RC003`).
#[derive(Debug, Clone)]
pub struct ConfigStore {
    pub dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("not a directory: {0}")]
    NotADirectory(PathBuf),
}

pub type Result<T> = std::result::Result<T, ConfigError>;

impl ConfigStore {
    /// Create a store rooted at `dir` (created if missing).
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    /// Load config; returns defaults when the file does not exist.
    pub fn load(&self) -> Result<Config> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = fs::read_to_string(&path)?;
        serde_json::from_str(&text).map_err(|e| {
            // Keep last-good behavior: on corruption return defaults (the
            // corrupt file is left untouched for diagnosis).
            ConfigError::Json(e)
        })
    }

    pub fn load_or_default(&self) -> Config {
        self.load().unwrap_or_default()
    }

    /// Atomically save: write `<config.json.tmp>` -> fsync -> rename.
    pub fn save(&self, config: &Config) -> Result<()> {
        let path = self.config_path();
        let tmp = self.dir.join("config.json.tmp");
        let json = serde_json::to_string_pretty(config)?;

        {
            let mut file = File::create(&tmp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
        }

        fs::rename(&tmp, &path)?;

        // Best-effort fsync of the directory so the rename is durable.
        #[cfg(unix)]
        if let Ok(dir_file) = File::open(&self.dir) {
            let _ = dir_file.sync_all();
        }

        Ok(())
    }
}

pub fn default_config_dir_name() -> &'static str {
    "RemoteMic/RC003"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (tempfile::TempDir, ConfigStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(dir.path()).unwrap();
        (dir, store)
    }

    #[test]
    fn missing_file_returns_defaults() {
        let (_dir, store) = temp_store();
        let cfg = store.load().unwrap();
        assert_eq!(cfg.voice_mode, VoiceMode::WindowsVoiceTyping);
        assert_eq!(cfg.gain_db, 10.0);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let (_dir, store) = temp_store();
        let mut cfg = Config::default();
        cfg.output_endpoint_id = Some("cable-input".into());
        cfg.gain_db = 12.0;
        cfg.selected_device_id = Some("device-1".into());
        store.save(&cfg).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded.output_endpoint_id.as_deref(), Some("cable-input"));
        assert_eq!(loaded.gain_db, 12.0);
        assert_eq!(loaded.selected_device_id.as_deref(), Some("device-1"));
        assert_eq!(loaded.mapping.bindings.len(), 13);
    }

    #[test]
    fn corrupt_file_falls_back_without_panic() {
        let (_dir, store) = temp_store();
        fs::write(store.config_path(), "{not valid json").unwrap();
        let cfg = store.load_or_default();
        assert_eq!(cfg.voice_mode, VoiceMode::WindowsVoiceTyping);
    }

    #[test]
    fn atomic_save_leaves_no_tmp_file() {
        let (_dir, store) = temp_store();
        let cfg = Config::default();
        store.save(&cfg).unwrap();
        assert!(!store.dir.join("config.json.tmp").exists());
        assert!(store.config_path().exists());
    }
}
