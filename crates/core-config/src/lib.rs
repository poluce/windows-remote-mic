//! core-config — 应用配置持久化。
//!
//! 写入是原子的（临时文件 + fsync + rename），因此崩溃不会留下
//! 写入一半的 JSON。

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use core_mapping::MappingConfig;

/// 语音输入目标。目前从 Windows 内置语音输入（Win+H）开始。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceMode {
    WindowsVoiceTyping,
    // ImeDoubao、ImeWeChat —— 预留待后续使用。
}

/// 用户校准后的物理按键特征。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyCalibration {
    pub button: String,
    pub code: String,
    pub key: String,
    pub vkey: Option<u32>,
}

/// 顶层应用配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub selected_device_id: Option<String>,
    pub output_endpoint_id: Option<String>,
    pub voice_mode: VoiceMode,
    pub gain_db: f32,
    pub auto_reconnect: bool,
    pub mapping: MappingConfig,
    #[serde(default)]
    pub key_calibrations: std::collections::HashMap<String, KeyCalibration>,
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
            key_calibrations: std::collections::HashMap::new(),
        }
    }
}

/// 绑定到某个目录的配置存储（例如 `%LOCALAPPDATA%\RemoteMic\RC003`）。
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
    /// 创建以 `dir` 为根目录的存储（目录不存在时会创建）。
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    /// 加载配置；文件不存在时返回默认值。
    pub fn load(&self) -> Result<Config> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = fs::read_to_string(&path)?;
        serde_json::from_str(&text).map_err(|e| {
            // 保持“最后一次可用”行为：配置损坏时返回默认值
            // （损坏文件原样保留，便于诊断）。
            ConfigError::Json(e)
        })
    }

    pub fn load_or_default(&self) -> Config {
        self.load().unwrap_or_default()
    }

    /// 原子保存：写入 `<config.json.tmp>` -> fsync -> rename。
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

        // 尽力对目录执行 fsync，确保 rename 持久化。
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
        let cfg = Config {
            output_endpoint_id: Some("cable-input".into()),
            gain_db: 12.0,
            selected_device_id: Some("device-1".into()),
            ..Config::default()
        };
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
