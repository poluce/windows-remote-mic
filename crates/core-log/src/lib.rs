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
//! - creating a file named `debug` in `%LOCALAPPDATA%\RemoteMic\RC003\`.

use std::io::Write;
use std::path::Path;

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

fn debug_enabled() -> bool {
    let env_on = std::env::var("REMOTE_MIC_DEBUG")
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "on"))
        .unwrap_or(false);
    if env_on {
        return true;
    }

    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    Path::new(&base)
        .join("RemoteMic")
        .join("RC003")
        .join("debug")
        .exists()
}

fn write_log(level: &str, line: &str) {
    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string();
    let full = format!("[{now}] [{level}] {line}");

    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
    let dir = Path::new(&base).join("RemoteMic").join("RC003");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("remote-mic.log"))
    {
        let _ = writeln!(file, "{full}");
    }
}
