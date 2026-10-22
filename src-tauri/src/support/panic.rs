//! panic 兜底：写 panic.log + 异步释放残留按键。

/// 直接把崩溃信息追加到 `panic.log`。
///
/// 这里刻意不走 `core_log`：如果 panic 恰好发生在日志锁内部，再调日志会死锁，
/// 崩溃现场就永远写不出来了。
fn write_panic_log(text: &str) {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let dir = std::path::Path::new(&base).join("RemoteMic/RC003");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("panic.log");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(file, "[{ts}] {text}");
    }
}

/// 全局 panic 兜底：
/// 1. 记录崩溃位置与原因（panic.log）；
/// 2. 异步补发一次「全部松开」，避免 panic 时按住的虚拟键残留（应用被 abort /
///    退出兜底路径来不及执行时，这是最后一道保险）。
pub(crate) fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let text = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "未知 panic".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        write_panic_log(&format!("panic at {location}: {text}"));

        // 独立线程里做释放：panic 现场可能正持有某把锁，重入会死锁。
        std::thread::spawn(|| {
            core_hid::tap::clear_active_usages();
            core_input::release_all_keys();
        });

        default_hook(info);
    }));
}
