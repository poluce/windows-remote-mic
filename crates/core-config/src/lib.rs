//! core-config — 应用配置持久化。
//!
//! 写入是原子的（临时文件 + fsync + rename），因此崩溃不会留下
//! 写入一半的 JSON。

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use core_mapping::{MappingConfig, VoiceTarget};

/// 用户校准后的物理按键特征。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyCalibration {
    pub button: String,
    pub code: String,
    pub key: String,
    pub vkey: Option<u32>,
}

/// 用户命名的自定义快捷键。
///
/// **名字只是显示层**：映射绑定里存的仍然是 `combo:lctrl+k` 这种原始写法
/// （见 `core_mapping::combo_action_key`），后端解析、调度器执行、应用专属
/// 配置文件全都不认识这个名字。所以删掉或改掉一条命名，已有绑定照样能用，
/// 只是显示时回退成原始按键。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedShortcut {
    /// 用户起的名字，例如「发送消息」。
    pub name: String,
    /// 规范化后的组合键 token，例如 `["lctrl", "enter"]`。
    /// 也是这条记录的身份——同一个组合只留一条，再存就是改名。
    pub keys: Vec<String>,
}

/// 顶层应用配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// 上次连接的设备 ID（预留：暂无自动重连读取路径）。
    pub selected_device_id: Option<String>,
    pub mapping: MappingConfig,
    #[serde(default)]
    pub key_calibrations: std::collections::HashMap<String, KeyCalibration>,
    /// 拦截 HID 按键信号：驱动层清零 HOGP 报告，系统不响应遥控器按键，
    /// 只由本应用注入映射动作。默认开启。
    #[serde(default = "default_hid_tap_eat")]
    pub hid_tap_eat: bool,
    /// 长按判定阈值（毫秒）。
    #[serde(default = "default_long_press_ms")]
    pub long_press_ms: u64,
    /// 双击判定窗口（毫秒）。
    #[serde(default = "default_double_click_ms")]
    pub double_click_ms: u64,
    /// 语音识别目标：麦克风键 Voice 动作唤起哪一家的语音输入。
    /// 默认 WindowsVoice（Win+H），向后兼容；第三方输入法待真机验证。
    #[serde(default)]
    pub voice_target: VoiceTarget,
    /// 用户命名的自定义快捷键库。旧的 config.json 没有这一项，`serde(default)`
    /// 让它读成空列表，不会因为加字段而丢掉整个配置。
    #[serde(default)]
    pub shortcuts: Vec<NamedShortcut>,
    /// 上次**成功**检查更新的 Unix 秒时间戳（0 表示还没成功检查过）。
    ///
    /// 自动检查（启动 / 运行中定时）要求距上次成功检查 ≥1 小时才真正联网；
    /// 手动点「检查更新」不受此限制。放这里是为了重启后仍然生效。
    #[serde(default)]
    pub last_update_check_at: u64,
    /// 配置写入版本号。每次经 `ConfigStore::update` 成功写盘后 +1；
    /// 用于 UI / MCP 的乐观并发与撤销校验。旧配置读成 0。
    #[serde(default)]
    pub revision: u64,
    /// 是否允许外部智能体通过 MCP 接口配置本应用。默认关闭。
    #[serde(default)]
    pub agent_api_enabled: bool,
}

fn default_hid_tap_eat() -> bool {
    true
}

fn default_long_press_ms() -> u64 {
    550
}

fn default_double_click_ms() -> u64 {
    250
}

impl Default for Config {
    fn default() -> Self {
        Self {
            selected_device_id: None,
            mapping: MappingConfig::default(),
            key_calibrations: std::collections::HashMap::new(),
            hid_tap_eat: true,
            long_press_ms: 550,
            double_click_ms: 250,
            voice_target: VoiceTarget::default(),
            shortcuts: Vec::new(),
            last_update_check_at: 0,
            revision: 0,
            agent_api_enabled: false,
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

/// 进程内所有 `config.json` 写操作的全局锁。
///
/// 现有命令是「各自 load → 改 → save」；UI 与 MCP 并发时没有这把锁会互相覆盖。
/// 所有写路径必须走 `ConfigStore::update`。
static CONFIG_WRITE_LOCK: Mutex<()> = Mutex::new(());

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

    fn backup_path(&self) -> PathBuf {
        self.dir.join("config.json.bak")
    }

    fn load_from(&self, path: &std::path::Path) -> Result<Config> {
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    /// 加载配置；文件不存在时返回默认值。
    ///
    /// 主文件损坏时自动回退到上一次成功保存的 `config.json.bak`，
    /// 避免一次写坏/断电就让用户的全部映射与校准静默丢失。
    pub fn load(&self) -> Result<Config> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(Config::default());
        }
        match self.load_from(&path) {
            Ok(cfg) => Ok(cfg),
            Err(primary_err) => {
                let backup = self.backup_path();
                if backup.exists() {
                    match self.load_from(&backup) {
                        Ok(cfg) => {
                            core_log::log_warn(&format!(
                                "[config] config.json 损坏（{primary_err}），已回退到上次成功保存的备份 config.json.bak"
                            ));
                            return Ok(cfg);
                        }
                        Err(e) => core_log::log_error(&format!(
                            "[config] 配置文件与备份均损坏：主文件={primary_err}，备份={e}"
                        )),
                    }
                }
                Err(primary_err)
            }
        }
    }

    pub fn load_or_default(&self) -> Config {
        self.load().unwrap_or_else(|e| {
            core_log::log_warn(&format!("[config] 读取配置失败，本次使用默认配置：{e}"));
            Config::default()
        })
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

        // 覆盖前把当前文件复制一份作为 last-known-good：
        // 用 copy 而不是 rename，即使后续 rename 失败也不会丢掉原配置。
        if path.exists() {
            let backup = self.backup_path();
            let _ = fs::remove_file(&backup);
            if let Err(e) = fs::copy(&path, &backup) {
                // 备份失败不阻断主流程，但必须留下痕迹。
                core_log::log_warn(&format!("[config] 备份旧配置失败（继续保存）: {e}"));
            }
        }

        fs::rename(&tmp, &path)?;

        // 尽力对目录执行 fsync，确保 rename 持久化。
        #[cfg(unix)]
        if let Ok(dir_file) = File::open(&self.dir) {
            let _ = dir_file.sync_all();
        }

        Ok(())
    }

    /// 在全局写锁内执行一次「load → 修改 → save」，成功后 revision +1。
    ///
    /// - `f` 返回 `Err` 时不写盘、不递增 revision；
    /// - 返回 `(f 的返回值, 新 revision)`，调用方据此热更新调度器 / 返回给 MCP；
    /// - 这是所有配置写操作的唯一入口，避免 UI 与智能体并发丢更新。
    pub fn update<R>(
        &self,
        f: impl FnOnce(&mut Config) -> std::result::Result<R, String>,
    ) -> std::result::Result<(R, u64), String> {
        let (value, revision) = self.update_opt(|cfg| f(cfg).map(Some))?;
        Ok((value.expect("update_opt 在 Some 分支必有值"), revision))
    }

    /// 与 `update` 相同，但允许闭包返回 `None` 表示「没有实际变化」。
    ///
    /// 无变化时不写盘、不递增 revision、不触碰 `.bak`——保持旧命令的
    /// 「重复删除 / 重复设置同一状态」无副作用语义。
    pub fn update_opt<R>(
        &self,
        f: impl FnOnce(&mut Config) -> std::result::Result<Option<R>, String>,
    ) -> std::result::Result<(Option<R>, u64), String> {
        // 闭包里可能 panic；锁中毒时继续用内部值，避免配置写永久锁死。
        let _guard = CONFIG_WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut cfg = self.load().map_err(|e| e.to_string())?;
        let Some(value) = f(&mut cfg)? else {
            return Ok((None, cfg.revision));
        };
        cfg.revision = cfg.revision.saturating_add(1);
        self.save(&cfg).map_err(|e| e.to_string())?;
        Ok((Some(value), cfg.revision))
    }
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
        assert!(cfg.selected_device_id.is_none());
        assert!(cfg.hid_tap_eat, "拦截 HID 按键信号默认开启");
    }

    #[test]
    fn save_then_load_roundtrip() {
        let (_dir, store) = temp_store();
        let cfg = Config {
            hid_tap_eat: false,
            selected_device_id: Some("device-1".into()),
            ..Config::default()
        };
        store.save(&cfg).unwrap();

        let loaded = store.load().unwrap();
        assert!(!loaded.hid_tap_eat);
        assert_eq!(loaded.selected_device_id.as_deref(), Some("device-1"));
        assert_eq!(loaded.mapping.bindings.len(), 14);
    }

    #[test]
    fn corrupt_file_falls_back_without_panic() {
        let (_dir, store) = temp_store();
        fs::write(store.config_path(), "{not valid json").unwrap();
        let cfg = store.load_or_default();
        assert!(cfg.hid_tap_eat);
    }

    #[test]
    fn legacy_config_without_voice_target_defaults_to_windows_voice() {
        let (_dir, store) = temp_store();
        // 旧版本 config.json 没有 voice_target 字段（mapping 等字段真实存在）。
        fs::write(
            store.config_path(),
            r#"{"mapping":{"bindings":[]},"hid_tap_eat":true,"long_press_ms":550,"double_click_ms":300}"#,
        )
        .unwrap();
        let cfg = store.load().unwrap();
        assert_eq!(cfg.voice_target, VoiceTarget::WindowsVoice);
        // 「上次成功检查更新」是后加的字段：旧配置读成 0（从未检查）。
        assert_eq!(cfg.last_update_check_at, 0);
        assert_eq!(cfg.revision, 0);
        assert!(!cfg.agent_api_enabled);
    }

    #[test]
    fn update_check_timestamp_roundtrip() {
        let (_dir, store) = temp_store();
        let cfg = Config {
            last_update_check_at: 1_760_000_000,
            ..Config::default()
        };
        store.save(&cfg).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.last_update_check_at, 1_760_000_000);
    }

    #[test]
    fn voice_target_roundtrip_and_unknown_falls_back() {
        let (_dir, store) = temp_store();
        let cfg = Config {
            voice_target: VoiceTarget::ImeWechat,
            ..Config::default()
        };
        store.save(&cfg).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.voice_target, VoiceTarget::ImeWechat);
    }

    #[test]
    fn invalid_voice_target_json_falls_back_without_panic() {
        let (_dir, store) = temp_store();
        fs::write(
            store.config_path(),
            r#"{"mapping":{"bindings":[]},"voice_target":"no_such_target","hid_tap_eat":true}"#,
        )
        .unwrap();
        // 未知枚举值应整体回落默认，而不是 panic。
        let cfg = store.load_or_default();
        assert_eq!(cfg.voice_target, VoiceTarget::WindowsVoice);
    }

    #[test]
    fn corrupt_primary_falls_back_to_backup() {
        let (_dir, store) = temp_store();
        let cfg = Config {
            hid_tap_eat: false,
            selected_device_id: Some("backup-device".into()),
            ..Config::default()
        };
        store.save(&cfg).unwrap();
        // 第二次保存才会产生 .bak（内容与第一次一致）。
        store.save(&cfg).unwrap();
        assert!(store.backup_path().exists());
        // 把主文件写坏，应能从备份恢复。
        fs::write(store.config_path(), "{broken").unwrap();
        let loaded = store.load().expect("应回退到备份");
        assert_eq!(loaded.selected_device_id.as_deref(), Some("backup-device"));
        assert!(!loaded.hid_tap_eat);
    }

    #[test]
    fn atomic_save_leaves_no_tmp_file() {
        let (_dir, store) = temp_store();
        let cfg = Config::default();
        store.save(&cfg).unwrap();
        assert!(!store.dir.join("config.json.tmp").exists());
        assert!(store.config_path().exists());
    }
    #[test]
    fn update_bumps_revision_and_persists() {
        let (_dir, store) = temp_store();
        let ((), rev1) = store
            .update(|cfg| {
                cfg.hid_tap_eat = false;
                Ok(())
            })
            .unwrap();
        assert_eq!(rev1, 1);
        let ((), rev2) = store
            .update(|cfg| {
                cfg.long_press_ms = 600;
                Ok(())
            })
            .unwrap();
        assert_eq!(rev2, 2);

        let loaded = store.load().unwrap();
        assert_eq!(loaded.revision, 2);
        assert!(!loaded.hid_tap_eat);
        assert_eq!(loaded.long_press_ms, 600);
    }

    #[test]
    fn update_error_does_not_write_or_bump_revision() {
        let (_dir, store) = temp_store();
        let result: std::result::Result<((), u64), String> =
            store.update(|_cfg| Err("校验失败".into()));
        assert!(result.is_err());
        assert!(!store.config_path().exists(), "失败时不应写盘");
        assert_eq!(store.load().unwrap().revision, 0);
    }
    #[test]
    fn update_opt_none_does_not_write_or_bump_revision() {
        let (_dir, store) = temp_store();
        let (value, revision) = store
            .update_opt(|cfg| {
                cfg.hid_tap_eat = false;
                Ok(None::<()>)
            })
            .unwrap();
        assert!(value.is_none());
        assert_eq!(revision, 0);
        assert!(!store.config_path().exists(), "无变化时不应写盘");
    }

    #[test]
    fn update_opt_some_writes_and_bumps_revision() {
        let (_dir, store) = temp_store();
        let (value, revision) = store
            .update_opt(|cfg| {
                cfg.hid_tap_eat = false;
                Ok(Some("changed"))
            })
            .unwrap();
        assert_eq!(value, Some("changed"));
        assert_eq!(revision, 1);
        let loaded = store.load().unwrap();
        assert!(!loaded.hid_tap_eat);
        assert_eq!(loaded.revision, 1);
    }
}
