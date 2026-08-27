//! Remote Mic 的最小化共享文件日志器。
//!
//! 所有调试跟踪信息都会追加到：
//! `%LOCALAPPDATA%\RemoteMic\RC003\remote-mic.log`
//!
//! 每行包含本地时间戳和一个级别（`DEBUG` / `INFO` /
//! `WARN` / `ERROR`），因此可以用 `findstr` 或
//! `Select-String` 过滤日志文件。
//!
//! `DEBUG` 日志默认关闭。可通过以下任一方式临时启用：
//! - 环境变量 `REMOTE_MIC_DEBUG=1`，或
//! - 在 `%LOCALAPPDATA%\RemoteMic\RC003\` 下创建名为 `debug` 的文件，或
//! - 在运行时调用 [`set_debug_enabled`]。
//!
//! 主日志文件达到 [`MAX_LOG_BYTES`] 时自动轮转；
//! 旧文件保存为 `remote-mic.<timestamp>.log`，
//! 并保留 [`KEEP_BACKUP_FILES`] 个备份，更旧的会被清理。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI8, Ordering};
use std::sync::{Mutex, RwLock};

/// `remote-mic.log` 轮转前的默认最大大小。
pub const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB

/// 要保留的轮转备份 `remote-mic.*.log` 文件数量。
pub const KEEP_BACKUP_FILES: usize = 5;

/// 当前活动日志文件的名称。
pub const LOG_FILE_NAME: &str = "remote-mic.log";

static LOG_MUTEX: Mutex<()> = Mutex::new(());

/// DEBUG 日志的运行时覆盖：`-1` 自动（环境变量/文件），`0` 强制关闭，
/// `1` 强制开启。
static DEBUG_OVERRIDE: AtomicI8 = AtomicI8::new(-1);

/// 供测试/嵌入场景使用的日志目录覆盖。设置后优先于
/// `%LOCALAPPDATA%\RemoteMic\RC003`。
static LOG_DIR_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 追加一条 DEBUG 级别日志。仅在临时调试日志开启时写入。
pub fn log_debug(line: &str) {
    if debug_enabled() {
        write_log("DEBUG", line);
    }
}

/// 追加一条 INFO 级别日志。
pub fn log_line(line: &str) {
    write_log("INFO", line);
}

/// 追加一条 INFO 级别日志。
pub fn log_info(line: &str) {
    write_log("INFO", line);
}

/// 追加一条 WARN 级别日志。
pub fn log_warn(line: &str) {
    write_log("WARN", line);
}

/// 追加一条 ERROR 级别日志。
pub fn log_error(line: &str) {
    write_log("ERROR", line);
}

/// DEBUG 日志当前是否生效。
///
/// 优先使用 [`set_debug_enabled`] 设置的运行时覆盖；否则使用
/// `REMOTE_MIC_DEBUG` 环境变量或 `debug` 标记文件。
pub fn debug_enabled() -> bool {
    match DEBUG_OVERRIDE.load(Ordering::Relaxed) {
        1 => return true,
        0 => return false,
        _ => {}
    }

    let env_on = std::env::var("REMOTE_MIC_DEBUG")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "on"))
        .unwrap_or(false);
    if env_on {
        return true;
    }

    log_dir().join("debug").exists()
}

/// 在运行时强制开启或关闭 DEBUG 日志。
///
/// 供诊断 UI 使用，使用户无需重启应用即可切换详细日志。
pub fn set_debug_enabled(enabled: bool) {
    DEBUG_OVERRIDE.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
}

/// 将运行时 DEBUG 覆盖重置为自动模式（基于环境变量/文件）。
pub fn reset_debug_enabled() {
    DEBUG_OVERRIDE.store(-1, Ordering::Relaxed);
}

/// 返回日志文件所在的目录。
pub fn log_dir() -> PathBuf {
    if let Ok(guard) = LOG_DIR_OVERRIDE.read() {
        if let Some(dir) = guard.as_ref() {
            return dir.clone();
        }
    }
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    Path::new(&base).join("RemoteMic").join("RC003")
}

/// 返回当前活动日志文件的路径。
pub fn log_path() -> PathBuf {
    log_dir().join(LOG_FILE_NAME)
}

/// 设置固定的日志目录覆盖。
///
/// 主要用于测试和嵌入场景。传入 `None` 可清除覆盖
/// （将重新使用基于环境变量的默认目录）。
pub fn set_log_dir_override(dir: Option<PathBuf>) {
    if let Ok(mut guard) = LOG_DIR_OVERRIDE.write() {
        *guard = dir;
    }
}

/// 列出所有 Remote Mic 日志文件（活动日志 + 轮转备份）及基本信息。
pub fn log_files() -> Vec<LogFileInfo> {
    let dir = log_dir();
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
                continue;
            };
            if name == LOG_FILE_NAME || name.starts_with("remote-mic.") && name.ends_with(".log") {
                if let Ok(meta) = fs::metadata(&path) {
                    files.push(LogFileInfo {
                        name,
                        path: path.display().to_string(),
                        size: meta.len(),
                        modified: meta
                            .modified()
                            .ok()
                            .and_then(|t| t.elapsed().ok())
                            .map(|d| d.as_secs()),
                    });
                }
            }
        }
    }
    // 活动日志排在前面，然后按轮转备份从新到旧排列。
    files.sort_by(|a, b| {
        let a_active = a.name == LOG_FILE_NAME;
        let b_active = b.name == LOG_FILE_NAME;
        b_active.cmp(&a_active).then_with(|| b.name.cmp(&a.name))
    });
    files
}

/// 读取活动日志文件的尾部，最多返回 `max_bytes` 字节。
///
/// 会跳过不完整的第一行，因此结果始终从行边界开始
/// （除非文件小于请求的窗口大小）。
pub fn read_log_tail(max_bytes: usize) -> String {
    let path = log_path();
    let max_bytes = max_bytes.max(1024);
    let mut file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };

    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes as u64);

    if start > 0 {
        if file.seek(SeekFrom::Start(start)).is_err() {
            return String::new();
        }
        let mut buf = Vec::with_capacity(max_bytes);
        if file.read_to_end(&mut buf).is_err() {
            return String::new();
        }
        // 丢弃不完整的第一行。
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            buf.drain(..=pos);
        }
        String::from_utf8_lossy(&buf).to_string()
    } else {
        let mut buf = Vec::with_capacity(len as usize);
        if file.read_to_end(&mut buf).is_err() {
            return String::new();
        }
        String::from_utf8_lossy(&buf).to_string()
    }
}

/// 清空当前活动日志文件。
pub fn clear_log() -> std::io::Result<()> {
    let _guard = LOG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    File::create(log_path())?;
    Ok(())
}

/// 如果活动日志文件超过 [`MAX_LOG_BYTES`] 则进行轮转，然后清理旧备份。
/// 每次写入前会自动调用。
pub fn rotate_log_if_needed() {
    let _guard = LOG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    rotate_if_needed(&log_path());
}

fn write_log(level: &str, line: &str) {
    let _guard = LOG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string();
    let full = format!("[{now}] [{level}] {line}");

    let dir = log_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(LOG_FILE_NAME);
    rotate_if_needed(&path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{full}");
    }
}

fn rotate_if_needed(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() >= MAX_LOG_BYTES {
            let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
            let backup = path.with_file_name(format!("remote-mic.{ts}.log"));
            let _ = fs::rename(path, backup);
            prune_backups();
        }
    }
}

fn prune_backups() {
    let dir = log_dir();
    let mut backups: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(String::from) else {
                continue;
            };
            if name.starts_with("remote-mic.") && name.ends_with(".log") && name != LOG_FILE_NAME {
                backups.push(path);
            }
        }
    }

    // 最新文件排在前面（时间戳嵌入在文件名中）。
    backups.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    if backups.len() > KEEP_BACKUP_FILES {
        for old in backups.into_iter().skip(KEEP_BACKUP_FILES) {
            let _ = fs::remove_file(old);
        }
    }
}

/// 诊断 UI 中显示的日志文件基本信息。
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogFileInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// 距上次修改的秒数（如果已知）。
    pub modified: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn test_dir(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join("remote-mic-core-log-tests");
        let dir = base.join(tag);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_and_reads_tail() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = test_dir("writes_and_reads_tail");
        set_log_dir_override(Some(dir.clone()));
        log_line("hello");
        log_error("boom");
        let text = read_log_tail(4096);
        assert!(text.contains("hello"));
        assert!(text.contains("boom"));
        assert!(text.contains("[INFO]"));
        assert!(text.contains("[ERROR]"));
        set_log_dir_override(None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_truncates() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = test_dir("clear_truncates");
        set_log_dir_override(Some(dir.clone()));
        log_line("before");
        assert!(!read_log_tail(4096).is_empty());
        clear_log().unwrap();
        assert!(read_log_tail(4096).is_empty());
        set_log_dir_override(None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn debug_override_works() {
        set_debug_enabled(true);
        assert!(debug_enabled());
        set_debug_enabled(false);
        assert!(!debug_enabled());
        reset_debug_enabled();
    }
}
