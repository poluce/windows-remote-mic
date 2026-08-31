//! Windows 低层键盘钩子（WH_KEYBOARD_LL）。
//! 在 WebView2 或其他窗口消费按键之前，直接从操作系统捕获所有硬件按键
//! （包括浏览器返回、媒体键、应用键等）。

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RawKeyEvent {
    pub vkey: u32,
    pub pressed: bool,
}

type HookCallback = Box<dyn Fn(RawKeyEvent) + Send>;

static HOOK_CALLBACK: Mutex<Option<HookCallback>> = Mutex::new(None);

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[cfg(target_os = "windows")]
#[allow(dead_code)]
struct SendHhook(HHOOK);
#[cfg(target_os = "windows")]
unsafe impl Send for SendHhook {}

#[cfg(target_os = "windows")]
static HOOK_HANDLE: Mutex<Option<SendHhook>> = Mutex::new(None);

/// 通过 WH_KEYBOARD_LL 开始监听全局键盘事件。
pub fn start_key_hook(callback: impl Fn(RawKeyEvent) + Send + 'static) -> Result<(), String> {
    *HOOK_CALLBACK.lock().unwrap() = Some(Box::new(callback));

    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(|| unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0);

            match hook {
                Ok(h) => {
                    *HOOK_HANDLE.lock().unwrap() = Some(SendHhook(h));
                    crate::log_line("[hook] 全局键盘钩子（WH_KEYBOARD_LL）注册成功");
                    let mut msg: MSG = std::mem::zeroed();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                        windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                    }
                }
                Err(e) => {
                    crate::log_error(&format!(
                        "[hook] 安装键盘钩子（SetWindowsHookExW）失败: {e}"
                    ));
                }
            }
        });
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

/// WebView2 经常吞掉或不会转为 KeyboardEvent 的虚拟键。
#[cfg(target_os = "windows")]
fn is_gap_vkey(vk: u32) -> bool {
    matches!(
        vk,
        93 | 166..=183 | 255 // 应用键、浏览器、音量、媒体、启动、电源
    )
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        if is_down || is_up {
            let kbd = *(lparam.0 as *const KBDLLHOOKSTRUCT);
            let vk = kbd.vkCode;
            let scan = kbd.scanCode;
            let flags = kbd.flags.0;
            // 本应用或系统注入的按键（SendInput）带 LLKHF_INJECTED 标记。
            // 跳过它们，避免注入动作被钩子当作新的遥控器按键回声转发。
            if (kbd.flags & LLKHF_INJECTED).0 != 0 {
                return CallNextHookEx(None, code, wparam, lparam);
            }
            // 不转发普通打字输入。测试器曾收集笔记本键盘
            // 作为遥控器校准；只有 WebView2 会吞掉的补充键
            // （返回 / 主页 / 音量 / 应用 / 电源）才转发到 UI。
            if !is_gap_vkey(vk) {
                return CallNextHookEx(None, code, wparam, lparam);
            }
            crate::log_debug(&format!(
                "[hook] 捕获遥控器补充按键: vkCode={}, scanCode=0x{:02X}, flags=0x{:02X}, 按下={}",
                vk, scan, flags, is_down
            ));
            let event = RawKeyEvent {
                vkey: vk,
                pressed: is_down,
            };
            if let Ok(guard) = HOOK_CALLBACK.lock() {
                if let Some(cb) = guard.as_ref() {
                    cb(event);
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
