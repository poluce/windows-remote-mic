//! Windows Low-Level Keyboard Hook (WH_KEYBOARD_LL).
//! Captures all hardware keystrokes (including BrowserBack, Media, App keys)
//! directly from the OS before WebView2 or other windows can consume them.

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
    CallNextHookEx, GetMessageW, SetWindowsHookExW, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[cfg(target_os = "windows")]
#[allow(dead_code)]
struct SendHhook(HHOOK);
#[cfg(target_os = "windows")]
unsafe impl Send for SendHhook {}

#[cfg(target_os = "windows")]
static HOOK_HANDLE: Mutex<Option<SendHhook>> = Mutex::new(None);

/// Start listening to global keyboard events via WH_KEYBOARD_LL.
pub fn start_key_hook(callback: impl Fn(RawKeyEvent) + Send + 'static) -> Result<(), String> {
    *HOOK_CALLBACK.lock().unwrap() = Some(Box::new(callback));

    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(|| unsafe {
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0);

            match hook {
                Ok(h) => {
                    *HOOK_HANDLE.lock().unwrap() = Some(SendHhook(h));
                    crate::log_line("[hook] WH_KEYBOARD_LL global hook registered successfully");
                    let mut msg: MSG = std::mem::zeroed();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                        windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                    }
                }
                Err(e) => {
                    crate::log_error(&format!("[hook] SetWindowsHookExW failed: {e}"));
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

/// Virtual-keys WebView2 often eats or never turns into a KeyboardEvent.
#[cfg(target_os = "windows")]
fn is_gap_vkey(vk: u32) -> bool {
    matches!(
        vk,
        93 | 166..=183 | 255 // Apps, Browser*, Volume*, Media*, Launch*, Power
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
            crate::log_debug(&format!(
                "[hook] LowLevelKey: vkCode={}, scanCode=0x{:02X}, flags=0x{:02X}, is_down={}",
                vk, scan, flags, is_down
            ));
            // Do not forward ordinary typing. The tester was collecting laptop
            // keystrokes as remote calibration; only gap keys that WebView2
            // swallows (Back / Home / Volume / App / Power) go to the UI.
            if !is_gap_vkey(vk) {
                return CallNextHookEx(None, code, wparam, lparam);
            }
            crate::log_line(&format!(
                "[hook] forward gap key: vkCode={}, is_down={}",
                vk, is_down
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
