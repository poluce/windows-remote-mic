//! Windows-only SendInput helpers.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, INPUT, INPUT_0,
    INPUT_KEYBOARD, VIRTUAL_KEY, VK_H, VK_LWIN,
};

use crate::Result;

/// Press Win + H and release.
pub fn press_win_h() -> Result<()> {
    eprintln!("[input] press Win+H");
    let down = |vk| keyboard_input(vk, false);
    let up = |vk| keyboard_input(vk, true);

    let inputs = [
        down(VK_LWIN),
        down(VK_H),
        up(VK_H),
        up(VK_LWIN),
    ];

    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
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
