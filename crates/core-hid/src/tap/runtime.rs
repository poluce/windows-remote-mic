//! 旁路生命周期：单实例运行标志、注入器入口、启动准备（gadget / 配置落地）。
//!
//! 由语音桥在 ATVV 就绪后调用 `start_after_atvv`；`maybe_run_injector` 处理
//! 提权子进程的 `--hid-tap-inject` 入口。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::panic_payload_text;

use super::config::{
    eat_enabled, eat_mode_file, gadget_available, gadget_dir, gadget_dll_path, load_injected_pid,
    tap_port, trace_enabled, write_eat_mode_file, GADGET_SCRIPT,
};
use super::hub::hub_loop;
use super::inject::{inject_into, write_last_inject_error};
use super::status::{status, TapState};

pub(super) static RUNNING: AtomicBool = AtomicBool::new(false);

pub(super) static INJECTED_PID: Mutex<Option<u32>> = Mutex::new(None);
/// PID we already asked UAC / inject for. Never prompt again for the same host.
pub(super) static INJECT_ATTEMPTED_PID: Mutex<Option<u32>> = Mutex::new(None);

/// 如果本进程以 `--hid-tap-inject --pid N` 方式启动，则注入后退出。
/// 返回 true 时调用方不应启动 UI。
pub fn maybe_run_injector() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if !args.iter().any(|a| a == "--hid-tap-inject") {
        return false;
    }
    let pid = args
        .windows(2)
        .find(|pair| pair[0] == "--pid")
        .and_then(|pair| pair[1].parse::<u32>().ok());
    let Some(pid) = pid else {
        core_log::log_error("[hid-tap] 注入器缺少 --pid 参数");
        std::process::exit(2);
    };
    match inject_into(pid) {
        Ok(()) => {
            core_log::log_info(&format!("[hid-tap] 已注入 pid={pid}"));
            std::process::exit(0);
        }
        Err(e) => {
            core_log::log_error(&format!("[hid-tap] 注入 pid={pid} 失败: {e}"));
            write_last_inject_error(&e);
            std::process::exit(1);
        }
    }
}

/// 启动 localhost 服务，并在需要时请求提权以执行注入。
/// 可安全地多次调用。绝不打开 HID GATT 服务。
///
/// 缺少 Frida Gadget 时会在后台线程自动下载（不阻塞语音桥初始化）。
pub fn start_after_atvv() {
    if RUNNING.swap(true, Ordering::SeqCst) {
        core_log::log_info("[hid-tap] 旁路服务已在运行");
        return;
    }
    // 恢复上次已注入的 WUDFHost PID：若宿主进程未变，则跳过注入，不再弹 UAC。
    if let Some(pid) = load_injected_pid() {
        *INJECTED_PID.lock().unwrap_or_else(|e| e.into_inner()) = Some(pid);
    }

    if gadget_available() {
        status(TapState::Pending, "正在准备返回/音量旁路…");
    } else {
        status(
            TapState::Pending,
            "正在自动下载 Frida Gadget（首次需要联网）…",
        );
    }

    std::thread::Builder::new()
        .name("rc003-hid-tap".into())
        .spawn(|| {
            // 旁路线程是「守护型」服务：任何一次 panic 都不能让它静默消失，
            // 这里兜住并自动重启（UAC/注入都带幂等重试，重启是安全的）。
            loop {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Err(e) = prepare_runtime() {
                        status(TapState::Unavailable, &format!("返回/音量旁路未启用：{e}"));
                        return false;
                    }
                    if !gadget_available() {
                        status(
                            TapState::Unavailable,
                            "返回/音量旁路未启用：自动安装 Frida Gadget 后仍未找到 DLL",
                        );
                        return false;
                    }
                    status(TapState::Pending, "返回/音量旁路已启动（首次可能弹出 UAC）");
                    hub_loop();
                    true
                }));
                match outcome {
                    Ok(true) => break,
                    Ok(false) => {
                        RUNNING.store(false, Ordering::SeqCst);
                        break;
                    }
                    Err(payload) => {
                        core_log::log_error(&format!(
                            "[hid-tap] 旁路线程 panic 已隔离，2s 后自动重启：{}",
                            panic_payload_text(payload.as_ref())
                        ));
                        std::thread::sleep(Duration::from_secs(2));
                    }
                }
            }
        })
        .ok();
}

fn prepare_runtime() -> Result<(), String> {
    let dir = gadget_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // 先把目录 ACL 设好，确保普通用户可写入下载的 DLL / JS / config，
    // SYSTEM（WUDFHost）也能加载。
    ensure_runtime_acl(&dir)?;

    let mut wrote_new_file = false;

    // DLL 缺失时自动从 GitHub 下载官方 Gadget（版本/哈希与 fetch 脚本一致）。
    if !gadget_dll_path().is_file() {
        crate::gadget_fetch::ensure_frida_gadget(&dir)
            .map_err(|e| format!("自动获取 Frida Gadget 失败：{e}"))?;
        wrote_new_file = true;
    }

    // 写脚本。ProgramData 已给 Users Modify 权限，正常可写；失败则说明环境异常，阻断并提示。
    let script_path = dir.join("rc003-hid-tap.js");
    if !file_has_content(&script_path, GADGET_SCRIPT.as_bytes()) {
        std::fs::write(&script_path, GADGET_SCRIPT)
            .map_err(|e| format!("写入 rc003-hid-tap.js 失败: {e}"))?;
        wrote_new_file = true;
    }

    // 写 config（文件名必须与 DLL 名匹配：frida-gadget.config）
    // 吃掉模式热更新文件也一并就位（设置页可随时覆盖）。
    if !eat_mode_file().is_file() {
        write_eat_mode_file(eat_enabled());
    }
    let cfg = serde_json::json!({
        "interaction": {
            "type": "script",
            "path": "rc003-hid-tap.js",
            "parameters": {
                "host": "127.0.0.1",
                "port": tap_port(),
                "eat": eat_enabled(),
                "trace": trace_enabled(),
                "eat_file": eat_mode_file().to_string_lossy()
            },
            "on_change": "ignore"
        },
        "runtime": "qjs",
        "teardown": "minimal"
    });
    let cfg_bytes = serde_json::to_vec_pretty(&cfg).unwrap();
    let cfg_path = dir.join("frida-gadget.config");
    if !file_has_content(&cfg_path, &cfg_bytes) {
        std::fs::write(&cfg_path, cfg_bytes)
            .map_err(|e| format!("写入 frida-gadget.config 失败: {e}"))?;
        wrote_new_file = true;
    }

    // 新写入文件后再巩固一次 ACL（下载/写脚本可能改变继承）。
    if wrote_new_file {
        ensure_runtime_acl(&dir)?;
    }

    Ok(())
}

/// 文件存在且内容与给定字节一致时返回 true（用于跳过不必要的写入）。
fn file_has_content(path: &std::path::Path, expected: &[u8]) -> bool {
    match std::fs::read(path) {
        Ok(existing) => existing == expected,
        Err(_) => false,
    }
}

/// 确保运行时目录权限：
/// - SYSTEM / Administrators 完全控制（保证 WUDFHost 加载 DLL）
/// - Users 具备 Modify（保证普通用户能更新 JS/config）
///
/// 使用 `/grant` 追加，不破坏已有权限。
fn ensure_runtime_acl(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = std::process::Command::new("icacls.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .arg(path)
            .args([
                "/grant",
                "*S-1-5-18:(OI)(CI)F",
                "/grant",
                "*S-1-5-32-544:(OI)(CI)F",
                "/grant",
                "*S-1-5-32-545:(OI)(CI)M",
                "/C",
                "/Q",
            ])
            .output()
            .map_err(|e| format!("icacls 启动失败: {e}"))?;
        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!("设置 Gadget 目录 ACL 失败: {}", stdout.trim()));
        }
    }
    Ok(())
}
