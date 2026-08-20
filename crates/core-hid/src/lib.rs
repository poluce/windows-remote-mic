//! core-hid — RC003 HID usage map and keyboard-report parsing (pure logic).

use core_mapping::ButtonId;

/// Keyboard usage page for the remote's ordinary buttons.
pub const HID_USAGE_PAGE_KEYBOARD: u16 = 0x07;

/// RC003 button -> HID keyboard usage.
///
/// The microphone normally arrives on the ATVV control channel, but the
/// device can also report it as Keyboard F5 (usage 0x3E); we keep it as a
/// fallback so a pure-HID path can still detect the voice key.
pub const BUTTON_USAGE_MAP: [(u32, ButtonId); 14] = [
    (0x003E, ButtonId::Mic),     // F5 fallback for voice key
    (0x00F1, ButtonId::Back),
    (0x0028, ButtonId::Ok),
    (0x0035, ButtonId::Tv),
    (0x004A, ButtonId::Home),
    (0x004F, ButtonId::Right),
    (0x0050, ButtonId::Left),
    (0x0051, ButtonId::Down),
    (0x0052, ButtonId::Up),
    (0x0065, ButtonId::Menu),
    (0x0066, ButtonId::Power),
    (0x0080, ButtonId::VolumeUp),
    (0x0081, ButtonId::VolumeDown),
    (0x007F, ButtonId::Power), // volume_mute usage is not a physical key; remap to Power is NOT intended
];

/// Map a HID usage id to a physical button.
pub fn usage_to_button(usage: u32) -> Option<ButtonId> {
    // The volume_mute entry above is intentionally excluded from decoding:
    // the remote has no physical mute key.
    match usage {
        0x003E => Some(ButtonId::Mic),
        0x00F1 => Some(ButtonId::Back),
        0x0028 => Some(ButtonId::Ok),
        0x0035 => Some(ButtonId::Tv),
        0x004A => Some(ButtonId::Home),
        0x004F => Some(ButtonId::Right),
        0x0050 => Some(ButtonId::Left),
        0x0051 => Some(ButtonId::Down),
        0x0052 => Some(ButtonId::Up),
        0x0065 => Some(ButtonId::Menu),
        0x0066 => Some(ButtonId::Power),
        0x0080 => Some(ButtonId::VolumeUp),
        0x0081 => Some(ButtonId::VolumeDown),
        _ => None,
    }
}

/// Map a physical button back to its HID usage id.
pub fn button_to_usage(button: ButtonId) -> Option<u32> {
    BUTTON_USAGE_MAP
        .iter()
        .find_map(|(usage, b)| {
            if *b == button && usage_to_button(*usage) == Some(button) {
                Some(*usage)
            } else {
                None
            }
        })
}

/// Parse a single Raw Input keyboard report into pressed buttons.
///
/// Each non-zero byte is a keyboard usage; usages we don't know are ignored.
pub fn parse_keyboard_report(report: &[u8]) -> Vec<ButtonId> {
    report
        .iter()
        .filter_map(|&b| usage_to_button(u32::from(b)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_physical_buttons_have_usages() {
        for button in ButtonId::ALL {
            assert!(
                usage_to_button(button_to_usage(button).unwrap()).is_some(),
                "missing usage for {button:?}"
            );
        }
    }

    #[test]
    fn maps_back_and_volume_usages() {
        assert_eq!(usage_to_button(0x00F1), Some(ButtonId::Back));
        assert_eq!(usage_to_button(0x0080), Some(ButtonId::VolumeUp));
        assert_eq!(usage_to_button(0x0081), Some(ButtonId::VolumeDown));
        assert_eq!(usage_to_button(0x003E), Some(ButtonId::Mic));
    }

    #[test]
    fn parse_report_maps_known_buttons() {
        // up(0x52), ok(0x28), back(0xF1)
        let buttons = parse_keyboard_report(&[0x52, 0x28, 0xF1, 0x00]);
        assert_eq!(
            buttons,
            vec![ButtonId::Up, ButtonId::Ok, ButtonId::Back]
        );
    }

    #[test]
    fn unknown_usages_are_ignored() {
        assert_eq!(parse_keyboard_report(&[0x01, 0xFF, 0x00]), Vec::new());
    }
}
