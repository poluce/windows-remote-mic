//! core-stats — 仅本地的使用统计（按键次数、语音时长）。
//!
//! # 为什么是「内存累计 + 定时落盘」
//!
//! 统计写在按键动作的热路径上（每次按键都会调用 [`StatsStore::record_key`]）。
//! 早期实现每次都「读文件 → 解析 JSON → 改 → 写临时文件 → rename」，
//! 一次按键等于两次文件 IO；磁盘卡顿时会直接拖慢按键注入。
//!
//! 现在改为：内存里累计，最多每 [`FLUSH_INTERVAL`] 落盘一次；
//! 进程退出/`Drop` 时再尽力 flush。统计不是关键数据，丢最后几秒可接受。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 两次落盘之间的最小间隔。
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// 一天的本地统计数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyStats {
    /// 物理按键 id / 逻辑动作名称 -> 按键次数。
    pub key_presses: HashMap<String, u64>,
    /// 语音输入总秒数。
    pub voice_seconds: u64,
    /// 已启动的语音会话数量。
    pub voice_sessions: u64,
}

/// 以某个目录为根目录的存储（例如 `%LOCALAPPDATA%\RemoteMic\RC003`）。
pub struct StatsStore {
    pub dir: PathBuf,
    /// 内存中的最新统计（None = 尚未从磁盘加载）。
    cache: Mutex<Option<HashMap<String, DailyStats>>>,
    /// 内存数据是否比磁盘新。
    dirty: AtomicBool,
    /// 上次成功落盘时间。
    last_flush: Mutex<Instant>,
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
        Ok(Self {
            dir,
            cache: Mutex::new(None),
            dirty: AtomicBool::new(false),
            last_flush: Mutex::new(Instant::now()),
        })
    }

    fn path(&self) -> PathBuf {
        self.dir.join("stats.json")
    }

    /// 当前本地日期键（从纪元起的天数）。
    pub fn date_key_now() -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = secs / 86_400;
        format!("{days}")
    }

    fn read_from_disk(&self) -> Result<HashMap<String, DailyStats>> {
        let path = self.path();
        if !path.exists() {
            return Ok(HashMap::new());
        }
        Ok(serde_json::from_str(&fs::read_to_string(&path)?)?)
    }

    /// 从磁盘读取统计（**不**包含尚未落盘的内存增量，供诊断/测试使用）。
    pub fn load(&self) -> Result<HashMap<String, DailyStats>> {
        self.read_from_disk()
    }

    fn write_to_disk(&self, stats: &HashMap<String, DailyStats>) -> Result<()> {
        let tmp = self.dir.join("stats.json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(stats)?)?;
        fs::rename(&tmp, self.path())?;
        self.dirty.store(false, Ordering::SeqCst);
        if let Ok(mut last) = self.last_flush.lock() {
            *last = Instant::now();
        }
        Ok(())
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, Option<HashMap<String, DailyStats>>> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 在内存里更新统计，必要时落盘。
    fn update<F>(&self, apply: F) -> Result<()>
    where
        F: FnOnce(&mut HashMap<String, DailyStats>),
    {
        let mut guard = self.lock_cache();
        if guard.is_none() {
            *guard = Some(self.read_from_disk()?);
        }
        if let Some(stats) = guard.as_mut() {
            apply(stats);
        }
        self.dirty.store(true, Ordering::SeqCst);

        let due = self
            .last_flush
            .lock()
            .map(|t| t.elapsed() >= FLUSH_INTERVAL)
            .unwrap_or(true);
        if due {
            if let Some(stats) = guard.as_ref() {
                self.write_to_disk(stats)?;
            }
        }
        Ok(())
    }

    pub fn record_key(&self, key: &str) -> Result<()> {
        let day = Self::date_key_now();
        self.update(|stats| {
            let day = stats.entry(day).or_default();
            *day.key_presses.entry(key.to_string()).or_insert(0) += 1;
        })
    }

    pub fn record_voice(&self, seconds: u64) -> Result<()> {
        let day = Self::date_key_now();
        self.update(|stats| {
            let day = stats.entry(day).or_default();
            day.voice_seconds += seconds;
            day.voice_sessions += 1;
        })
    }

    /// 立即把内存中的统计落盘（诊断自检 / 退出前调用）。
    pub fn flush(&self) -> Result<()> {
        let guard = self.lock_cache();
        if let Some(stats) = guard.as_ref() {
            if self.dirty.load(Ordering::SeqCst) {
                self.write_to_disk(stats)?;
            }
        }
        Ok(())
    }
}

impl Drop for StatsStore {
    fn drop(&mut self) {
        // 尽力落盘：退出/线程结束时不要把最后几秒统计丢掉。
        let _ = self.flush();
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
        s.flush().unwrap();

        let stats = s.load().unwrap();
        let day = stats.values().next().unwrap();
        assert_eq!(day.key_presses.get("up"), Some(&2));
        assert_eq!(day.voice_seconds, 12);
        assert_eq!(day.voice_sessions, 1);
    }

    #[test]
    fn records_are_batched_until_flush() {
        let (_d, s) = store();
        s.record_key("down").unwrap();
        // 未 flush 前磁盘上还没有增量（避免每次按键都做两次文件 IO）。
        let on_disk = s.load().unwrap();
        let pressed = on_disk
            .values()
            .map(|d| d.key_presses.get("down").copied().unwrap_or(0))
            .sum::<u64>();
        assert_eq!(pressed, 0, "批量写入前不应落盘");
        s.flush().unwrap();
        let after = s.load().unwrap();
        let pressed = after
            .values()
            .map(|d| d.key_presses.get("down").copied().unwrap_or(0))
            .sum::<u64>();
        assert_eq!(pressed, 1, "flush 后应可见");
    }

    #[test]
    fn drop_flushes_pending_records() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = StatsStore::new(dir.path()).unwrap();
            s.record_key("left").unwrap();
        }
        let s = StatsStore::new(dir.path()).unwrap();
        let stats = s.load().unwrap();
        let pressed = stats
            .values()
            .map(|d| d.key_presses.get("left").copied().unwrap_or(0))
            .sum::<u64>();
        assert_eq!(pressed, 1, "Drop 时应尽力落盘");
    }
}
