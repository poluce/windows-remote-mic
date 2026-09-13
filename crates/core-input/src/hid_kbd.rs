//! 标准 USB HID Boot Keyboard 报告（8 字节），供虚拟 HID 键盘提交。
//!
//! 报告布局：
//! - byte0：修饰键位图（左/右 Ctrl/Shift/Alt/Win）
//! - byte1：保留
//! - byte2..=7：最多 6 个同时按下的 HID usage

pub const REPORT_LEN: usize = 8;

/// Consumer Control 报告长度（1 字节位图）。
pub const CONSUMER_REPORT_LEN: usize = 1;

/// 标准 Consumer Control（媒体键）报告描述符，单集合、无 Report ID。
///
/// 键盘页（0x07）没有播放/暂停这类 usage，媒体键必须走 Consumer 页（0x0C）。
/// 这里做成独立的一台虚拟 HID 设备，键盘设备保持 Boot Keyboard 原样不受影响。
pub const CONSUMER_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x0C, // Usage Page (Consumer)
    0x09, 0x01, // Usage (Consumer Control)
    0xA1, 0x01, // Collection (Application)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x04, //   Report Count (4)
    0x09, 0xCD, //   Usage (Play/Pause)
    0x09, 0xB5, //   Usage (Scan Next Track)
    0x09, 0xB6, //   Usage (Scan Previous Track)
    0x09, 0xB7, //   Usage (Stop)
    0x81, 0x06, //   Input (Data,Var,Rel)
    0x95, 0x04, //   Report Count (4)
    0x81, 0x01, //   Input (Const) ; padding
    0xC0, // End Collection
];

const CONSUMER_PLAY_PAUSE: u8 = 0x01;
const CONSUMER_NEXT: u8 = 0x02;
const CONSUMER_PREV: u8 = 0x04;
const CONSUMER_STOP: u8 = 0x08;

/// Consumer 页媒体键 token -> 报告位。返回 `None` 表示不是媒体键。
pub fn consumer_bit(tok: &str) -> Option<u8> {
    let t = tok.trim().to_ascii_lowercase();
    Some(match t.as_str() {
        "play_pause" | "media_play_pause" => CONSUMER_PLAY_PAUSE,
        "next_track" | "media_next" => CONSUMER_NEXT,
        "prev_track" | "media_prev" => CONSUMER_PREV,
        "media_stop" => CONSUMER_STOP,
        _ => return None,
    })
}

/// 媒体键状态（Consumer Control 位图）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConsumerState {
    bits: u8,
}

impl ConsumerState {
    pub fn report(&self) -> [u8; CONSUMER_REPORT_LEN] {
        [self.bits]
    }

    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }

    pub fn press_token(&mut self, tok: &str) {
        if let Some(bit) = consumer_bit(tok) {
            self.bits |= bit;
        }
    }

    pub fn release_token(&mut self, tok: &str) {
        if let Some(bit) = consumer_bit(tok) {
            self.bits &= !bit;
        }
    }
}

const MOD_LCTRL: u8 = 0x01;
const MOD_LSHIFT: u8 = 0x02;
const MOD_LALT: u8 = 0x04;
const MOD_LWIN: u8 = 0x08;
const MOD_RCTRL: u8 = 0x10;
const MOD_RSHIFT: u8 = 0x20;
const MOD_RALT: u8 = 0x40;
const MOD_RWIN: u8 = 0x80;

/// 标准 Boot Keyboard 报告描述符（无 Report ID）。
pub const REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x06, // Usage (Keyboard)
    0xA1, 0x01, // Collection (Application)
    0x05, 0x07, //   Usage Page (Keyboard)
    0x19, 0xE0, //   Usage Minimum (Left Control)
    0x29, 0xE7, //   Usage Maximum (Right GUI)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x08, //   Report Count (8)
    0x81, 0x02, //   Input (Data,Var,Abs) ; modifiers
    0x95, 0x01, //   Report Count (1)
    0x75, 0x08, //   Report Size (8)
    0x81, 0x01, //   Input (Const) ; reserved
    0x95, 0x06, //   Report Count (6)
    0x75, 0x08, //   Report Size (8)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x65, //   Logical Maximum (101)
    0x05, 0x07, //   Usage Page (Keyboard)
    0x19, 0x00, //   Usage Minimum (0)
    0x29, 0x65, //   Usage Maximum (101)
    0x81, 0x00, //   Input (Data,Array)
    0xC0, // End Collection
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HidPart {
    Modifier(u8),
    Key(u8),
}

fn token_to_part(tok: &str) -> Option<HidPart> {
    let t = tok.trim().to_ascii_lowercase();
    Some(match t.as_str() {
        "lctrl" | "ctrl" => HidPart::Modifier(MOD_LCTRL),
        "rctrl" => HidPart::Modifier(MOD_RCTRL),
        "lshift" | "shift" => HidPart::Modifier(MOD_LSHIFT),
        "rshift" => HidPart::Modifier(MOD_RSHIFT),
        "lalt" | "alt" => HidPart::Modifier(MOD_LALT),
        "ralt" => HidPart::Modifier(MOD_RALT),
        "lwin" | "win" => HidPart::Modifier(MOD_LWIN),
        "rwin" => HidPart::Modifier(MOD_RWIN),
        "a" => HidPart::Key(0x04),
        "b" => HidPart::Key(0x05),
        "c" => HidPart::Key(0x06),
        "d" => HidPart::Key(0x07),
        "e" => HidPart::Key(0x08),
        "f" => HidPart::Key(0x09),
        "g" => HidPart::Key(0x0A),
        "h" => HidPart::Key(0x0B),
        "i" => HidPart::Key(0x0C),
        "j" => HidPart::Key(0x0D),
        "k" => HidPart::Key(0x0E),
        "l" => HidPart::Key(0x0F),
        "m" => HidPart::Key(0x10),
        "n" => HidPart::Key(0x11),
        "o" => HidPart::Key(0x12),
        "p" => HidPart::Key(0x13),
        "q" => HidPart::Key(0x14),
        "r" => HidPart::Key(0x15),
        "s" => HidPart::Key(0x16),
        "t" => HidPart::Key(0x17),
        "u" => HidPart::Key(0x18),
        "v" => HidPart::Key(0x19),
        "w" => HidPart::Key(0x1A),
        "x" => HidPart::Key(0x1B),
        "y" => HidPart::Key(0x1C),
        "z" => HidPart::Key(0x1D),
        "1" => HidPart::Key(0x1E),
        "2" => HidPart::Key(0x1F),
        "3" => HidPart::Key(0x20),
        "4" => HidPart::Key(0x21),
        "5" => HidPart::Key(0x22),
        "6" => HidPart::Key(0x23),
        "7" => HidPart::Key(0x24),
        "8" => HidPart::Key(0x25),
        "9" => HidPart::Key(0x26),
        "0" => HidPart::Key(0x27),
        "enter" | "return" => HidPart::Key(0x28),
        "esc" | "escape" => HidPart::Key(0x29),
        "backspace" => HidPart::Key(0x2A),
        "tab" => HidPart::Key(0x2B),
        "space" => HidPart::Key(0x2C),
        "f1" => HidPart::Key(0x3A),
        "f2" => HidPart::Key(0x3B),
        "f3" => HidPart::Key(0x3C),
        "f4" => HidPart::Key(0x3D),
        "f5" => HidPart::Key(0x3E),
        "f6" => HidPart::Key(0x3F),
        "f7" => HidPart::Key(0x40),
        "f8" => HidPart::Key(0x41),
        "f9" => HidPart::Key(0x42),
        "f10" => HidPart::Key(0x43),
        "f11" => HidPart::Key(0x44),
        "f12" => HidPart::Key(0x45),
        "insert" | "ins" => HidPart::Key(0x49),
        "home" => HidPart::Key(0x4A),
        "pageup" | "pgup" => HidPart::Key(0x4B),
        "delete" | "del" => HidPart::Key(0x4C),
        "end" => HidPart::Key(0x4D),
        "pagedown" | "pgdn" => HidPart::Key(0x4E),
        "right" => HidPart::Key(0x4F),
        "left" => HidPart::Key(0x50),
        "down" => HidPart::Key(0x51),
        "up" => HidPart::Key(0x52),
        "apps" | "context_menu" => HidPart::Key(0x65),
        "volume_mute" => HidPart::Key(0x7F),
        "volume_up" => HidPart::Key(0x80),
        "volume_down" => HidPart::Key(0x81),
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyboardState {
    modifiers: u8,
    keys: [u8; 6],
}

impl KeyboardState {
    pub fn report(&self) -> [u8; REPORT_LEN] {
        [
            self.modifiers,
            0,
            self.keys[0],
            self.keys[1],
            self.keys[2],
            self.keys[3],
            self.keys[4],
            self.keys[5],
        ]
    }

    pub fn is_empty(&self) -> bool {
        self.modifiers == 0 && self.keys.iter().all(|&k| k == 0)
    }

    pub fn press_tokens(&mut self, tokens: &[&str]) -> Result<(), String> {
        for tok in tokens {
            match token_to_part(tok) {
                Some(HidPart::Modifier(bit)) => self.modifiers |= bit,
                Some(HidPart::Key(usage)) => self.add_key(usage)?,
                None => return Err(format!("不支持的快捷键：{tok}")),
            }
        }
        Ok(())
    }

    pub fn release_tokens(&mut self, tokens: &[&str]) -> Result<(), String> {
        for tok in tokens {
            match token_to_part(tok) {
                Some(HidPart::Modifier(bit)) => self.modifiers &= !bit,
                Some(HidPart::Key(usage)) => self.remove_key(usage),
                None => return Err(format!("不支持的快捷键：{tok}")),
            }
        }
        Ok(())
    }

    fn add_key(&mut self, usage: u8) -> Result<(), String> {
        if self.keys.contains(&usage) {
            return Ok(());
        }
        if let Some(slot) = self.keys.iter_mut().find(|k| **k == 0) {
            *slot = usage;
            Ok(())
        } else {
            Err("同时按下的键超过 6 个".into())
        }
    }

    fn remove_key(&mut self, usage: u8) {
        if let Some(idx) = self.keys.iter().position(|&k| k == usage) {
            for i in idx..5 {
                self.keys[i] = self.keys[i + 1];
            }
            self.keys[5] = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rctrl_hold_sets_right_ctrl_modifier_only() {
        let mut s = KeyboardState::default();
        s.press_tokens(&["rctrl"]).unwrap();
        assert_eq!(s.report(), [MOD_RCTRL, 0, 0, 0, 0, 0, 0, 0]);
        s.release_tokens(&["rctrl"]).unwrap();
        assert_eq!(s.report(), [0; 8]);
    }

    #[test]
    fn win_h_is_left_gui_plus_h() {
        let mut s = KeyboardState::default();
        s.press_tokens(&["win", "h"]).unwrap();
        assert_eq!(s.report()[0], MOD_LWIN);
        assert_eq!(s.report()[2], 0x0B);
        s.release_tokens(&["h", "win"]).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn unknown_token_errors() {
        let mut s = KeyboardState::default();
        assert!(s.press_tokens(&["nope"]).is_err());
    }

    #[test]
    fn consumer_tokens_are_not_keyboard_tokens() {
        let mut s = KeyboardState::default();
        // 媒体键必须由 vhid 路由到 Consumer 设备，键盘表不应接受。
        assert!(s.press_tokens(&["play_pause"]).is_err());
        assert_eq!(consumer_bit("play_pause"), Some(CONSUMER_PLAY_PAUSE));
        assert_eq!(consumer_bit("media_play_pause"), Some(CONSUMER_PLAY_PAUSE));
        assert_eq!(consumer_bit("next_track"), Some(CONSUMER_NEXT));
        assert_eq!(consumer_bit("prev_track"), Some(CONSUMER_PREV));
        assert_eq!(consumer_bit("media_stop"), Some(CONSUMER_STOP));
        assert_eq!(consumer_bit("rctrl"), None);
    }

    #[test]
    fn consumer_state_press_release() {
        let mut c = ConsumerState::default();
        assert!(c.is_empty());
        c.press_token("play_pause");
        assert_eq!(c.report(), [CONSUMER_PLAY_PAUSE]);
        c.release_token("play_pause");
        assert!(c.is_empty());
        assert_eq!(c.report(), [0]);
    }
}
