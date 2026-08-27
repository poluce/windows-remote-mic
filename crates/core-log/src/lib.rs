//! Minimal shared file logger for Remote Mic.
//!
//! All debug traces are appended to:
//! `%LOCALAPPDATA%\RemoteMic\RC003\remote-mic.log`
//!
//! Each line includes a local timestamp and a level (`DEBUG` / `INFO` /
//! `WARN` / `ERROR`) so the file can be filtered with `findstr` or
//! `Select-String`.
//!
//! `DEBUG` logs are off by default. Enable them temporarily with either:
//! - environment variable `REMOTE_MIC_DEBUG=1`, or
//! - creating a file named `debug` in `%LOCALAPPDATA%\RemoteMic\RC003\`, or
//! - calling [`set_debug_enabled`] at runtime.
//!
//! The main log file is automatically rotated when it reaches
//! [`MAX_LOG_BYTES`]; old files are kept as `remote-mic.<timestamp>.log` and
//! pruned to [`KEEP_BACKUP_FILES`] backups.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI8, Ordering};
use std::sync::{Mutex, RwLock};

/// Default maximum size of `remote-mic.log` before it is rotated.
pub const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB

/// Number of rotated `remote-mic.*.log` backups to keep.
pub const KEEP_BACKUP_FILES: usize = 5;

/// Name of the active log file.
pub const LOG_FILE_NAME: &str = "remote-mic.log";

static LOG_MUTEX: Mutex<()> = Mutex::new(());

/// Runtime override for DEBUG logging: `-1` auto (env/file), `0` forced off,
/// `1` forced on.
static DEBUG_OVERRIDE: AtomicI8 = AtomicI8::new(-1);

/// Test/embedding override for the log directory. When set, it takes priority
/// over `%LOCALAPPDATA%\RemoteMic\RC003`.
static LOG_DIR_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Append a DEBUG-level line. Only written when temporary debug logging is on.
pub fn log_debug(line: &str) {
    if debug_enabled() {
        write_log("DEBUG", line);
    }
}

/// Append an INFO-level line.
pub fn log_line(line: &str) {
    write_log("INFO", line);
}

/// Append an INFO-level line.
pub fn log_info(line: &str) {
    write_log("INFO", line);
}

/// Append a WARN-level line.
pub fn log_warn(line: &str) {
    write_log("WARN", line);
}

/// Append an ERROR-level line.
pub fn log_error(line: &str) {
    write_log("ERROR", line);
}

/// Whether DEBUG logging is currently effective.
///
/// A runtime override set by [`set_debug_enabled`] wins; otherwise the
/// `REMOTE_MIC_DEBUG` environment variable or the `debug` marker file is used.
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

/// Force DEBUG logging on or off at runtime.
///
/// This is used by the diagnostics UI so the user can toggle verbose logs
/// without restarting the application.
pub fn set_debug_enabled(enabled: bool) {
    DEBUG_OVERRIDE.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
}

/// Reset the runtime DEBUG override back to automatic (env/file based).
pub fn reset_debug_enabled() {
    DEBUG_OVERRIDE.store(-1, Ordering::Relaxed);
}

/// Return the directory where log files live.
pub fn log_dir() -> PathBuf {
    if let Ok(guard) = LOG_DIR_OVERRIDE.read() {
        if let Some(dir) = guard.as_ref() {
            return dir.clone();
        }
    }
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    Path::new(&base).join("RemoteMic").join("RC003")
}

/// Return the active log file path.
pub fn log_path() -> PathBuf {
    log_dir().join(LOG_FILE_NAME)
}

/// Set a fixed log directory override.
///
/// This is mainly useful for tests and embedding scenarios. Pass `None` to
/// clear the override (the env-based default will be used again).
pub fn set_log_dir_override(dir: Option<PathBuf>) {
    if let Ok(mut guard) = LOG_DIR_OVERRIDE.write() {
        *guard = dir;
    }
}

/// List all Remote Mic log files (active + rotated backups) with basic info.
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
    // Active log first, then newest rotated backup first.
    files.sort_by(|a, b| {
        let a_active = a.name == LOG_FILE_NAME;
        let b_active = b.name == LOG_FILE_NAME;
        b_active.cmp(&a_active).then_with(|| b.name.cmp(&a.name))
    });
    files
}

/// Read the tail of the active log file, returning at most `max_bytes` bytes.
///
/// A partial first line is skipped so the result always starts at a line
/// boundary (unless the file is smaller than the requested window).
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
        // Drop the partial first line.
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

/// Clear the active log file.
pub fn clear_log() -> std::io::Result<()> {
    let _guard = LOG_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    File::create(log_path())?;
    Ok(())
}

/// Rotate the active log file if it exceeds [`MAX_LOG_BYTES`], then prune old
/// backups. Called automatically before each write.
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

    // Newest first (timestamp is embedded in the file name).
    backups.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    if backups.len() > KEEP_BACKUP_FILES {
        for old in backups.into_iter().skip(KEEP_BACKUP_FILES) {
            let _ = fs::remove_file(old);
        }
    }
}

/// Basic metadata for a log file shown in the diagnostics UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogFileInfo {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// Seconds since last modification, when known.
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
