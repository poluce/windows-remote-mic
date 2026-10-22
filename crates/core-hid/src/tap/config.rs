//! 旁路配置与路径：端口 / 吃掉模式热文件 / Frida gadget 路径 / 注入 PID 记忆。

use std::path::PathBuf;
use std::time::Duration;

pub(super) const TAP_PORT: u16 = 17331;
pub(super) const INPUT_GRACE: Duration = Duration::from_millis(800);
pub(super) const GADGET_SCRIPT: &str = include_str!("../rc003_hid_tap.js");

pub(super) fn tap_port() -> u16 {
    std::env::var("REMOTE_MIC_HID_TAP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(TAP_PORT)
}

/// 是否开启「吃掉」模式：驱动层把 HOGP 报告清零，系统不响应遥控器按键，
/// 只由本应用注入映射动作。
///
/// 优先级：`eat-mode.txt`（设置页写入，Frida 脚本热更新）>
/// 环境变量 `REMOTE_MIC_HID_TAP_EAT`（开发覆盖）> 默认开启。
pub(super) fn eat_enabled() -> bool {
    if let Some(v) = read_eat_mode_file() {
        return v;
    }
    std::env::var("REMOTE_MIC_HID_TAP_EAT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(true)
}

/// 吃掉模式热更新文件：设置页/启动时写入，Frida 脚本每秒轮询，
/// 无需重新注入 WUDFHost 即可生效。
pub(super) fn eat_mode_file() -> PathBuf {
    gadget_dir().join("eat-mode.txt")
}

pub(super) fn read_eat_mode_file() -> Option<bool> {
    let text = std::fs::read_to_string(eat_mode_file()).ok()?;
    let text = text.trim();
    if text == "1" || text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text == "0" || text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// 写入吃掉模式热更新文件（设置页切换与启动时调用）。
pub fn write_eat_mode_file(enabled: bool) {
    let _ = std::fs::create_dir_all(gadget_dir());
    let _ = std::fs::write(eat_mode_file(), if enabled { "1" } else { "0" });
}

/// 是否开启 IOCTL/模块追踪：枚举 WUDFHost 的 HID/BTH 模块导出并记录所有
/// IOCTL 码与缓冲区，用于定位报告转发点。
/// 通过环境变量 `REMOTE_MIC_HID_TAP_TRACE=1` 开启，默认关闭。
pub(super) fn trace_enabled() -> bool {
    std::env::var("REMOTE_MIC_HID_TAP_TRACE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub(super) fn gadget_dir() -> PathBuf {
    // 运行时文件放在 ProgramData：
    // - SYSTEM（WUDFHost）一定能读取/加载 DLL；
    // - 通过 ensure_runtime_acl 给 Users 加 Modify 权限，普通用户也能更新 JS/config；
    // - 避免 LocalAppData 下 SYSTEM 访问不确定的问题。
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(base).join("RemoteMic").join("hid-tap")
}

pub(super) fn gadget_dll_path() -> PathBuf {
    gadget_dir().join("frida-gadget.dll")
}

/// 已注入的 WUDFHost PID 持久化文件。应用重启后据此跳过重复注入，
/// 避免每次连接都弹 UAC（同一 WUDFHost 只需注入一次）。
pub(super) fn injected_pid_file() -> PathBuf {
    gadget_dir().join("injected-pid.txt")
}

pub(super) fn load_injected_pid() -> Option<u32> {
    std::fs::read_to_string(injected_pid_file())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

pub(super) fn save_injected_pid(pid: u32) {
    let _ = std::fs::write(injected_pid_file(), pid.to_string());
}

pub fn gadget_available() -> bool {
    gadget_dll_path().is_file()
}
