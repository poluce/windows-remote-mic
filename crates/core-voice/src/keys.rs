//! [`crate::ports::KeySink`] 的真实实现：虚拟 HID 注入 + Win+H。
//!
//! 桥接层与模拟链路都通过这个实现做按键副作用，不再各自直接调用
//! `core_hid` / `core_input`。

use crate::ports::KeySink;

/// 真实按键注入（WinUHid 虚拟键盘 / HOGP 旁路 / Windows 语音输入）。
pub struct SystemKeySink;

impl KeySink for SystemKeySink {
    fn start_after_atvv(&self) {
        core_hid::tap::start_after_atvv();
    }

    fn clear_active_usages(&self) {
        core_hid::tap::clear_active_usages();
    }

    fn release_all_keys(&self) {
        core_input::release_all_keys();
    }

    fn release_mic_on_audio_stop(&self) {
        core_hid::tap::release_mic_on_audio_stop();
    }

    fn touch_mic_activity(&self) {
        core_hid::tap::touch_mic_activity();
    }

    fn press_win_h(&self) -> Result<(), String> {
        core_input::press_win_h().map_err(|e| e.to_string())
    }

    fn press_escape(&self) -> Result<(), String> {
        core_input::press_escape().map_err(|e| e.to_string())
    }
}

/// 构造真实按键注入端口。
pub fn system_key_sink() -> Box<dyn KeySink> {
    Box::new(SystemKeySink)
}
