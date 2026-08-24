//! Windows-only SendInput helpers.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, INPUT, INPUT_0,
    INPUT_KEYBOARD, VIRTUAL_KEY, VK_H, VK_LWIN,
};

use crate::Result;

/// Press Win + H and release.
pub fn press_win_h() -> Result<()> {
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
