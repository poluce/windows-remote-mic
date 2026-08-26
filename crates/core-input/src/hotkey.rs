//! Windows-only SendInput helpers.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, INPUT, INPUT_0,
    INPUT_KEYBOARD, VIRTUAL_KEY, VK_ESCAPE, VK_H, VK_LWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowTextW,
};

use crate::Result;

/// Press Win + H and release.
pub fn press_win_h() -> Result<()> {
    crate::log_line("[input] press Win+H");
    log_foreground_window("before");
    let down = |vk| keyboard_input(vk, false);
    let up = |vk| keyboard_input(vk, true);

    let inputs = [
        down(VK_LWIN),
        down(VK_H),
        up(VK_H),
        up(VK_LWIN),
    ];

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    crate::log_line(&format!("[input] SendInput returned {sent} events"));
    log_foreground_window("after");
    if sent != inputs.len() as u32 {
        crate::log_error(&format!(
            "[input] SendInput only inserted {sent}/{} events",
            inputs.len()
        ));
        return Err(crate::error::InputError::Windows(format!(
            "SendInput only inserted {sent}/{} events",
            inputs.len()
        )));
    }
    Ok(())
}

/// Press Escape (used to close Windows voice typing).
pub fn press_escape() -> Result<()> {
    crate::log_line("[input] press Escape");
    let down = keyboard_input(VK_ESCAPE, false);
    let up = keyboard_input(VK_ESCAPE, true);
    let inputs = [down, up];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    crate::log_line(&format!("[input] Escape SendInput returned {sent} events"));
    if sent != inputs.len() as u32 {
        return Err(crate::error::InputError::Windows(format!(
            "SendInput only inserted {sent}/{} events",
            inputs.len()
        )));
    }
    Ok(())
}

fn keyboard_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let mut input = INPUT::default();
    input.r#type = INPUT_KEYBOARD;
    let ki = KEYBDINPUT {
        wVk: vk,
        wScan: 0,
        dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
        time: 0,
        dwExtraInfo: 0,
    };
    input.Anonymous = INPUT_0 { ki };
    input
}

/// Log the current foreground window title/class before/after a key press.
fn log_foreground_window(tag: &str) {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut title_buf = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title_buf);
        let title = String::from_utf16_lossy(&title_buf[..title_len.max(0) as usize]);

        let mut class_buf = [0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class_buf);
        let class = String::from_utf16_lossy(&class_buf[..class_len.max(0) as usize]);

        crate::log_line(&format!(
            "[input] foreground {tag}: hwnd={:?}, title={:?}, class={:?}",
            hwnd, title, class
        ));
    }
}

/// Play a sequence of virtual-key presses (modifiers + keys, press then release).
pub fn send_key_combo(tokens: &[&str]) -> Result<()> {
    let keys: Vec<(VIRTUAL_KEY, bool)> = {
        let mut seq = Vec::new();
        for tok in tokens {
            if let Some(vk) = token_to_vk(tok) {
                seq.push((vk, false));
            }
        }
        // release in reverse order
        let releases: Vec<(VIRTUAL_KEY, bool)> = seq
            .iter()
            .rev()
            .map(|(vk, _)| (*vk, true))
            .collect();
        seq.extend(releases);
        seq
    };

    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|(vk, up)| keyboard_input(*vk, *up))
        .collect();

    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
    Ok(())
}

/// Open an app via the shell (`cmd /c start "" <name>`).
pub fn open_app(name: &str) -> Result<()> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", name])
        .spawn()
        .map_err(|e| crate::error::InputError::Windows(e.to_string()))?;
    Ok(())
}

fn token_to_vk(tok: &str) -> Option<VIRTUAL_KEY> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    match tok.to_lowercase().as_str() {
        "win" | "lwin" => Some(VK_LWIN),
        "rwin" => Some(VK_RWIN),
        "ctrl" | "lctrl" => Some(VK_LCONTROL),
        "rctrl" => Some(VK_RCONTROL),
        "alt" | "lalt" => Some(VK_LMENU),
        "ralt" => Some(VK_RMENU),
        "shift" | "lshift" => Some(VK_LSHIFT),
        "rshift" => Some(VK_RSHIFT),
        "h" => Some(VK_H),
        "d" => Some(VK_D),
        "tab" => Some(VK_TAB),
        "enter" | "return" => Some(VK_RETURN),
        "esc" => Some(VK_ESCAPE),
        "space" => Some(VK_SPACE),
        "up" => Some(VK_UP),
        "down" => Some(VK_DOWN),
        "left" => Some(VK_LEFT),
        "right" => Some(VK_RIGHT),
        "backspace" => Some(VK_BACK),
        _ => None,
    }
}
