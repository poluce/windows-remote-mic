//! 标准 USB HID Boot Keyboard 报告（8 字节），供虚拟 HID 键盘提交。
//!
//! 报告布局：
//! - byte0：修饰键位图（左/右 Ctrl/Shift/Alt/Win）
//! - byte1：保留
//! - byte2..=7：最多 6 个同时按下的 HID usage

pub const REPORT_LEN: usize = 8;

/// Consumer Control 报告长度（1 字节位图）。
pub const CONSUMER_REPORT_LEN: usize = 1;

/// 标准 Consumer Control（媒体键 + 系统音量键）报告描述符，单集合、无 Report ID。
///
/// 键盘页（0x07）没有播放/暂停这类 usage，媒体键必须走 Consumer 页（0x0C）。
/// 系统音量键（volume_up / volume_down / volume_mute）也走这里：
/// 键盘页的 `0x7F/0x80/0x81` 只在声明了对应 usage 范围的键盘描述符里有效，
/// 而我们的 Boot Keyboard 描述符只声明到 `0x65`（标准键盘最大键位），
/// 直接塞 0x80 会被 Windows HID 栈忽略（实测音量加映射「日志执行了但系统无反应」）。
/// 把这几个 token 路由到 Consumer 设备的 `0xE9/0xEA/0xE2`，语义正确且系统原生支持。
///
/// 这里做成独立的一台虚拟 HID 设备，键盘设备保持 Boot Keyboard 原样不受影响。
pub const CONSUMER_REPORT_DESCRIPTOR: &[u8] = &[
    0x05, 0x0C, // Usage Page (Consumer)
    0x09, 0x01, // Usage (Consumer Control)
    0xA1, 0x01, // Collection (Application)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x07, //   Report Count (7)
    0x09, 0xCD, //   Usage (Play/Pause)
    0x09, 0xB5, //   Usage (Scan Next Track)
    0x09, 0xB6, //   Usage (Scan Previous Track)
    0x09, 0xB7, //   Usage (Stop)
    0x09, 0xE9, //   Usage (Volume Increment)
    0x09, 0xEA, //   Usage (Volume Decrement)
    0x09, 0xE2, //   Usage (Mute)
    0x81, 0x06, //   Input (Data,Var,Rel)
    0x95, 0x01, //   Report Count (1)
    0x81, 0x01, //   Input (Const) ; padding
    0xC0, // End Collection
];

const CONSUMER_PLAY_PAUSE: u8 = 0x01;
const CONSUMER_NEXT: u8 = 0x02;
const CONSUMER_PREV: u8 = 0x04;
const CONSUMER_STOP: u8 = 0x08;
const CONSUMER_VOLUME_UP: u8 = 0x10;
const CONSUMER_VOLUME_DOWN: u8 = 0x20;
const CONSUMER_VOLUME_MUTE: u8 = 0x40;

/// Consumer 页 media / 音量 token -> 报告位。返回 `None` 表示不是 Consumer 键。
pub fn consumer_bit(tok: &str) -> Option<u8> {
    let t = tok.trim().to_ascii_lowercase();
    Some(match t.as_str() {
        "play_pause" | "media_play_pause" => CONSUMER_PLAY_PAUSE,
        "next_track" | "media_next" => CONSUMER_NEXT,
        "prev_track" | "media_prev" => CONSUMER_PREV,
        "media_stop" => CONSUMER_STOP,
        "volume_up" => CONSUMER_VOLUME_UP,
        "volume_down" => CONSUMER_VOLUME_DOWN,
        "volume_mute" | "mute" => CONSUMER_VOLUME_MUTE,
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

    /// 无条件清空全部媒体键位（兜底：不需要知道按住了哪些键）。
    pub fn release_all(&mut self) {
        self.bits = 0;
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

/// 虚拟键盘（Boot Keyboard）报告里按键数组声明的 usage 上限。
///
/// HID Usage Tables 里虽然有 `0x80`（Keyboard Volume Up）这类标准编号，但
/// **设备能不能发，取决于它自己的报告描述符声明了什么**。本虚拟键盘使用
/// 标准 Boot Keyboard 描述符（usage `0x00–0x65`），超出范围的 usage 会被
/// Windows HID 栈**静默丢弃**（历史 bug：音量键原样抄遥控器的 `0x80` 注入，
/// 日志显示已执行、系统音量却毫无反应）。
///
/// 因此注入层的原则是「翻译」而不是「抄送」：
/// - 源设备（遥控器）的 usage 只用于**识别**，不能原样当目标 usage；
/// - 注入必须翻译到目标设备声明过的 usage，或改走 Consumer 页 / Win32 原语；
/// - [`KeyboardState::add_key`] 用本常量做运行期校验，越界直接报错，
///   避免再次出现「提交成功但系统不认」的静默失效。
///
/// 本常量同时用于构造 [`REPORT_DESCRIPTOR`]，保证「描述符声明」与
/// 「运行期校验」永远同源，有单测锁定。
pub const KEYBOARD_USAGE_MAX: u8 = 0x65;

/// 标准 Boot Keyboard 报告描述符（无 Report ID）。
pub const REPORT_DESCRIPTOR: &[u8] = &[
    0x05,
    0x01, // Usage Page (Generic Desktop)
    0x09,
    0x06, // Usage (Keyboard)
    0xA1,
    0x01, // Collection (Application)
    0x05,
    0x07, //   Usage Page (Keyboard)
    0x19,
    0xE0, //   Usage Minimum (Left Control)
    0x29,
    0xE7, //   Usage Maximum (Right GUI)
    0x15,
    0x00, //   Logical Minimum (0)
    0x25,
    0x01, //   Logical Maximum (1)
    0x75,
    0x01, //   Report Size (1)
    0x95,
    0x08, //   Report Count (8)
    0x81,
    0x02, //   Input (Data,Var,Abs) ; modifiers
    0x95,
    0x01, //   Report Count (1)
    0x75,
    0x08, //   Report Size (8)
    0x81,
    0x01, //   Input (Const) ; reserved
    0x95,
    0x06, //   Report Count (6)
    0x75,
    0x08, //   Report Size (8)
    0x15,
    0x00, //   Logical Minimum (0)
    0x25,
    KEYBOARD_USAGE_MAX, //   Logical Maximum (101)
    0x05,
    0x07, //   Usage Page (Keyboard)
    0x19,
    0x00, //   Usage Minimum (0)
    0x29,
    KEYBOARD_USAGE_MAX, //   Usage Maximum (101)
    0x81,
    0x00, //   Input (Data,Array)
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
        // 注意：音量三键不在这里 —— 它们走 Consumer 页（见 consumer_bit），
        // 键盘页 0x7F/0x80/0x81 超出本描述符声明的 usage 范围（0x00–0x65），
        // 塞进键盘报告会被系统忽略。
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
        if usage > KEYBOARD_USAGE_MAX {
            return Err(format!(
                "HID usage 0x{usage:02X} 超出虚拟键盘描述符声明范围（0x00–0x{KEYBOARD_USAGE_MAX:02X}）：\
                 源设备 usage 不能原样注入，需要翻译到目标设备支持的通道"
            ));
        }
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

    /// 无条件清空修饰键与 6 个按键槽位，回到「什么都没按」。
    ///
    /// 这是**兜底**用的：正常路径按 token 精确松开，但物理按键的松开事件
    /// 可能永远收不到（BLE 断链、调度暂停、映射热更新、应用退出）。此时
    /// 已注入的修饰键会一直按住并影响用户的**物理键盘**（例如 rctrl 卡住
    /// 后所有按键都变成 Ctrl+），只能在报告层强制归零。
    ///
    /// 与逐 token 松开不同，本方法不挑 token：report 全零必然代表「全部松开」，
    /// 多清不会清错。
    pub fn release_all(&mut self) {
        self.modifiers = 0;
        self.keys = [0; 6];
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
        // 媒体键 / 音量键必须由 vhid 路由到 Consumer 设备，键盘表不应接受。
        // 尤其音量键：键盘页 0x80 超出 Boot Keyboard 描述符的 usage 范围，
        // 误走键盘报告会被系统静默忽略（历史 bug：音量加映射无反应）。
        assert!(s.press_tokens(&["play_pause"]).is_err());
        assert!(s.press_tokens(&["volume_up"]).is_err());
        assert!(s.press_tokens(&["volume_down"]).is_err());
        assert!(s.press_tokens(&["volume_mute"]).is_err());
        assert_eq!(consumer_bit("play_pause"), Some(CONSUMER_PLAY_PAUSE));
        assert_eq!(consumer_bit("media_play_pause"), Some(CONSUMER_PLAY_PAUSE));
        assert_eq!(consumer_bit("next_track"), Some(CONSUMER_NEXT));
        assert_eq!(consumer_bit("prev_track"), Some(CONSUMER_PREV));
        assert_eq!(consumer_bit("media_stop"), Some(CONSUMER_STOP));
        assert_eq!(consumer_bit("volume_up"), Some(CONSUMER_VOLUME_UP));
        assert_eq!(consumer_bit("volume_down"), Some(CONSUMER_VOLUME_DOWN));
        assert_eq!(consumer_bit("volume_mute"), Some(CONSUMER_VOLUME_MUTE));
        assert_eq!(consumer_bit("mute"), Some(CONSUMER_VOLUME_MUTE));
        assert_eq!(consumer_bit("rctrl"), None);
    }

    #[test]
    fn consumer_volume_keys_use_separate_bits() {
        let mut c = ConsumerState::default();
        c.press_token("volume_up");
        assert_eq!(c.report(), [CONSUMER_VOLUME_UP]);
        c.release_token("volume_up");

        c.press_token("volume_down");
        assert_eq!(c.report(), [CONSUMER_VOLUME_DOWN]);
        c.release_token("volume_down");

        c.press_token("volume_mute");
        assert_eq!(c.report(), [CONSUMER_VOLUME_MUTE]);
        c.release_token("volume_mute");
        assert!(c.is_empty());

        // 音量与媒体键互不干扰。
        c.press_token("volume_up");
        c.press_token("play_pause");
        assert_eq!(c.report(), [CONSUMER_VOLUME_UP | CONSUMER_PLAY_PAUSE]);
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

    #[test]
    fn keyboard_release_all_clears_modifiers_and_keys() {
        let mut s = KeyboardState::default();
        // 修饰键卡住 + 按键槽位占满，是「虚拟键盘卡住」最坏的情况。
        s.press_tokens(&["rctrl", "lshift", "a", "b", "c", "d", "e", "f"])
            .unwrap();
        assert!(!s.is_empty());
        s.release_all();
        assert!(s.is_empty());
        assert_eq!(s.report(), [0; 8]);
    }

    #[test]
    fn keyboard_release_all_is_idempotent_and_keeps_usable() {
        let mut s = KeyboardState::default();
        s.press_tokens(&["lctrl"]).unwrap();
        s.release_all();
        s.release_all();
        // 兜底清空之后仍要能正常继续按键，否则等于把虚拟键盘弄坏。
        s.press_tokens(&["rctrl"]).unwrap();
        assert_eq!(s.report(), [MOD_RCTRL, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn keyboard_usage_max_is_single_source_with_descriptor() {
        assert_eq!(
            KEYBOARD_USAGE_MAX, 0x65,
            "Boot Keyboard 标准键位范围是 0x00–0x65"
        );
        let pairs: Vec<[u8; 2]> = REPORT_DESCRIPTOR.windows(2).map(|w| [w[0], w[1]]).collect();
        assert!(
            pairs.contains(&[0x29, KEYBOARD_USAGE_MAX]),
            "描述符 Usage Maximum 没跟着 KEYBOARD_USAGE_MAX 走"
        );
        assert!(
            pairs.contains(&[0x25, KEYBOARD_USAGE_MAX]),
            "描述符 Logical Maximum 没跟着 KEYBOARD_USAGE_MAX 走"
        );
    }

    #[test]
    fn keyboard_rejects_usage_outside_descriptor_range() {
        let mut s = KeyboardState::default();
        s.add_key(KEYBOARD_USAGE_MAX).expect("0x65 在声明范围内");
        // 0x80 就是曾经被当音量+原样注入键盘、然后被系统静默丢弃的 usage。
        let err = s.add_key(0x80).unwrap_err();
        assert!(err.contains("超出"), "越界 usage 必须明确报错：{err}");
        assert!(err.contains("0x80"), "{err}");
    }

    #[test]
    fn consumer_release_all_clears_all_bits() {
        let mut c = ConsumerState::default();
        c.press_token("play_pause");
        c.press_token("next_track");
        c.press_token("media_stop");
        c.press_token("volume_up");
        c.press_token("volume_mute");
        assert!(!c.is_empty());
        c.release_all();
        assert!(c.is_empty());
        assert_eq!(c.report(), [0]);
    }
}
