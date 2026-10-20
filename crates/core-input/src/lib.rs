//! core-input — Windows 键盘注入（虚拟 HID 键盘，不再使用 SendInput）。

pub mod error;
pub mod hid_kbd;
pub mod hook;
pub mod uia;
pub use error::{InputError, Result};
pub use hook::{start_key_hook, RawKeyEvent};
pub use uia::{focus_foreground_input, FocusOutcome};

#[cfg(target_os = "windows")]
mod vhid;

/// 向 `%LOCALAPPDATA%\RemoteMic\RC003\remote-mic.log` 追加一行日志。
///
/// 这是对 `core_log` 的薄封装，让现有调用方可以继续使用
/// `core_input::log_line`。
pub fn log_line(line: &str) {
    core_log::log_line(line);
}

/// 追加一行 DEBUG 级别日志。仅在临时调试日志开启时写入。
pub fn log_debug(line: &str) {
    core_log::log_debug(line);
}

/// 向共享的 Remote Mic 日志文件追加一行 ERROR 级别日志。
pub fn log_error(line: &str) {
    core_log::log_error(line);
}

/// 向共享的 Remote Mic 日志文件追加一行 WARN 级别日志。
pub fn log_warn(line: &str) {
    core_log::log_warn(line);
}

/// 按下 Win + H 启动 Windows 自带语音输入。
#[cfg(target_os = "windows")]
pub fn press_win_h() -> Result<()> {
    crate::vhid::press_win_h()
}

/// 非 Windows 平台桩实现，使 crate 在所有平台都能编译。
#[cfg(not(target_os = "windows"))]
pub fn press_win_h() -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

/// 打开 Windows 语音输入（Win+H）。每次都直接按 Win+H：
/// 弹窗关闭时打开；弹窗已打开时只会重置当前输入会话（实测不会关闭弹窗），
/// 可接受。关闭弹窗用 Esc / ✕。
#[cfg(target_os = "windows")]
pub fn open_voice_typing() -> Result<()> {
    crate::log_line("[input] 语音输入 -> 开启 (Win+H)");
    crate::vhid::press_win_h()
}

/// 非 Windows 平台桩实现。
#[cfg(not(target_os = "windows"))]
pub fn open_voice_typing() -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

/// 按语音识别目标分发「开启语音输入」（Tap 语义：按一次开启/重置会话）。
///
/// 目前只有 Windows 语音键入（Win+H）可用；第三方输入法（微信/豆包/搜狗）
/// 的唤起热键与 PTT 语义需真机验证后接入，验证前返回明确错误，避免臆测注入。
#[cfg(target_os = "windows")]
pub fn voice_open(target: core_mapping::VoiceTarget) -> Result<()> {
    use core_mapping::VoiceTarget as VT;
    match target {
        VT::WindowsVoice => open_voice_typing(),
        VT::ImeWechat => Err(InputError::Windows(
            "微信输入法语音尚未接入（需真机验证唤起热键）".to_string(),
        )),
        VT::ImeDoubao => Err(InputError::Windows(
            "豆包输入法语音尚未接入（需真机验证唤起热键）".to_string(),
        )),
        VT::ImeSogou => Err(InputError::Windows(
            "搜狗输入法语音尚未接入（需真机验证唤起热键）".to_string(),
        )),
    }
}

/// 非 Windows 平台桩实现。
#[cfg(not(target_os = "windows"))]
pub fn voice_open(_target: core_mapping::VoiceTarget) -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

/// 麦克风键 PTT 语义的「按住说话」开始（长按识别后触发）。
///
/// Windows 语音：长按达到阈值即按一次 Win+H 开启/重置语音会话
/// （与历史 `Press → Voice` 行为一致）。
/// 第三方输入法若支持按住说话，真机验证后在此接入保持按下的原语。
#[cfg(target_os = "windows")]
pub fn voice_press(target: core_mapping::VoiceTarget) -> Result<()> {
    use core_mapping::VoiceTarget as VT;
    match target {
        VT::WindowsVoice => open_voice_typing(),
        VT::ImeWechat => Err(InputError::Windows(
            "微信输入法语音尚未接入（需真机验证 PTT 热键）".to_string(),
        )),
        VT::ImeDoubao => Err(InputError::Windows(
            "豆包输入法语音尚未接入（需真机验证 PTT 热键）".to_string(),
        )),
        VT::ImeSogou => Err(InputError::Windows(
            "搜狗输入法语音尚未接入（需真机验证 PTT 热键）".to_string(),
        )),
    }
}

/// 非 Windows 平台桩实现。
#[cfg(not(target_os = "windows"))]
pub fn voice_press(_target: core_mapping::VoiceTarget) -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

/// 麦克风键 PTT 语义的「松开说话」（长按结束后触发）。
///
/// Windows 语音：松手即按一次 Win+H 停止/收尾当前语音会话
/// （与历史 `Release → Voice` 行为一致）。第三方输入法接入见 `voice_press`。
#[cfg(target_os = "windows")]
pub fn voice_release(target: core_mapping::VoiceTarget) -> Result<()> {
    use core_mapping::VoiceTarget as VT;
    match target {
        VT::WindowsVoice => open_voice_typing(),
        VT::ImeWechat => Err(InputError::Windows(
            "微信输入法语音尚未接入（需真机验证 PTT 热键）".to_string(),
        )),
        VT::ImeDoubao => Err(InputError::Windows(
            "豆包输入法语音尚未接入（需真机验证 PTT 热键）".to_string(),
        )),
        VT::ImeSogou => Err(InputError::Windows(
            "搜狗输入法语音尚未接入（需真机验证 PTT 热键）".to_string(),
        )),
    }
}

/// 非 Windows 平台桩实现。
#[cfg(not(target_os = "windows"))]
pub fn voice_release(_target: core_mapping::VoiceTarget) -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

/// 按下 Escape 关闭 Windows 语音输入。
#[cfg(target_os = "windows")]
pub fn press_escape() -> Result<()> {
    crate::vhid::press_escape()
}

/// 非 Windows 平台桩实现。
#[cfg(not(target_os = "windows"))]
pub fn press_escape() -> Result<()> {
    Err(InputError::Windows(
        "input injection is only implemented on Windows".to_string(),
    ))
}

#[cfg(target_os = "windows")]
mod hotkey;

/// 根据按键标记发送快捷键，例如 ["win","d"] 或 ["ctrl","space"]。
#[cfg(target_os = "windows")]
pub fn send_key_combo(tokens: &[&str]) -> Result<()> {
    crate::vhid::key_tap(tokens)
}

/// 只按下不松开，用于麦克风 Press（按住说话）。
#[cfg(target_os = "windows")]
pub fn send_key_down(tokens: &[&str]) -> Result<()> {
    crate::vhid::key_down(tokens)
}

/// 只松开，用于麦克风 Release（松手结束）。
#[cfg(target_os = "windows")]
pub fn send_key_up(tokens: &[&str]) -> Result<()> {
    crate::vhid::key_up(tokens)
}

/// 强制释放全部虚拟键（键盘 + 媒体键），回到「什么都没按」。
///
/// 兜底路径专用：物理按键的松开事件丢失时（BLE 断链、调度暂停、映射热更新、
/// 应用退出），已注入的修饰键会一直按住，把用户的**物理键盘**拖成 Ctrl+ 组合。
/// 本函数直接提交全零报告，不依赖「知道按住了哪些键」。
///
/// 不会为了释放去创建虚拟 HID 设备；返回是否真的清理过处于按下状态的键。
#[cfg(target_os = "windows")]
pub fn release_all_keys() -> bool {
    crate::vhid::release_all()
}

#[cfg(not(target_os = "windows"))]
pub fn release_all_keys() -> bool {
    false
}

/// 单实例互斥量句柄在进程生命周期内一直持有（不主动关闭）。
#[cfg(target_os = "windows")]
static SINGLE_INSTANCE_HANDLE: std::sync::Mutex<Option<usize>> = std::sync::Mutex::new(None);

/// 尝试获取单实例互斥量。
///
/// 返回 `false` 表示本机已经有 Remote Mic 在运行；调用方应提示并退出。
/// 创建失败时返回 `true`（宁可允许启动，也不要因为权限/命名问题打不开应用）。
#[cfg(target_os = "windows")]
pub fn acquire_single_instance() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = "Global\\RemoteMic-SingleInstance\0"
        .encode_utf16()
        .collect();
    unsafe {
        match CreateMutexW(None, true, PCWSTR(name.as_ptr())) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = CloseHandle(handle);
                    false
                } else {
                    if let Ok(mut guard) = SINGLE_INSTANCE_HANDLE.lock() {
                        *guard = Some(handle.0 as usize);
                    }
                    true
                }
            }
            Err(e) => {
                core_log::log_warn(&format!("[app] 单实例互斥量创建失败，跳过单实例保护: {e}"));
                true
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn acquire_single_instance() -> bool {
    true
}

/// 提示用户「应用已在运行」。
#[cfg(target_os = "windows")]
pub fn notify_already_running() {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    let text: Vec<u16> = "Remote Mic 已在运行，请切换到已打开的窗口。\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "Remote Mic\0".encode_utf16().collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            windows::core::PCWSTR(text.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn notify_already_running() {}

#[cfg(target_os = "windows")]
pub fn vhid_probe() -> String {
    crate::vhid::probe()
}

/// 虚拟 HID 状态 + 运行环境诊断（Secure Boot / 内存完整性 / 负载是否就位）。
#[cfg(target_os = "windows")]
pub fn vhid_diagnostics() -> crate::vhid::VhidDiagnostics {
    crate::vhid::diagnostics()
}

#[cfg(not(target_os = "windows"))]
pub fn vhid_probe() -> String {
    "虚拟 HID 仅 Windows 可用".into()
}

/// 通过 shell 打开应用/文件（仅 Windows）。
#[cfg(target_os = "windows")]
pub fn open_app(name: &str) -> Result<()> {
    crate::hotkey::open_app(name)
}

#[cfg(not(target_os = "windows"))]
pub fn send_key_combo(_tokens: &[&str]) -> Result<()> {
    Err(InputError::Windows(
        "key injection only on Windows".to_string(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn send_key_down(_tokens: &[&str]) -> Result<()> {
    Err(InputError::Windows(
        "key injection only on Windows".to_string(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn send_key_up(_tokens: &[&str]) -> Result<()> {
    Err(InputError::Windows(
        "key injection only on Windows".to_string(),
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn open_app(_name: &str) -> Result<()> {
    Err(InputError::Windows("open_app only on Windows".to_string()))
}
