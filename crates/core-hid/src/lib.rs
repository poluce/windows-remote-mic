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

/// Windows virtual-key the tester / mapping layer uses for a keyboard-page usage.
pub fn usage_to_vkey(usage: u32) -> Option<u16> {
    match usage {
        0x003E => Some(116), // F5 / mic fallback
        0x00F1 => Some(166), // RC003 Back (vendor keyboard usage)
        0x0028 => Some(13),  // Enter
        0x0035 => Some(180), // TV
        0x004A => Some(172), // Home
        0x004F => Some(39),  // Right
        0x0050 => Some(37),  // Left
        0x0051 => Some(40),  // Down
        0x0052 => Some(38),  // Up
        0x0065 => Some(93),  // Menu / App
        0x0066 => Some(255), // Power
        0x0080 => Some(175), // Volume up
        0x0081 => Some(174), // Volume down
        _ => None,
    }
}

/// Consumer Control (usage page 0x0C) -> Windows virtual-key.
pub fn consumer_usage_to_vkey(usage: u32) -> Option<u16> {
    match usage {
        0x0224 => Some(166), // AC Back
        0x0223 | 0x018A => Some(172), // AC Home
        0x00E9 => Some(175), // Volume increment
        0x00EA => Some(174), // Volume decrement
        0x00E2 => Some(173), // Mute
        0x0040 => Some(93),  // Menu
        _ => None,
    }
}

fn push_unique(out: &mut Vec<u16>, vk: u16) {
    if !out.contains(&vk) {
        out.push(vk);
    }
}

/// HidOverGatt characteristic-read IOCTL payload on RC003: 3-byte prefix + 6-byte usage array.
pub fn hogp_ioctl_payload(data: &[u8]) -> Option<&[u8]> {
    if data.len() == 9 && data.starts_with(&[0x01, 0x00, 0x00]) {
        Some(&data[3..9])
    } else {
        None
    }
}

/// Little-endian keyboard-page usages from a 6-byte HOGP payload.
pub fn hogp_payload_usages(payload: &[u8]) -> Vec<u16> {
    if payload.len() != 6 {
        return Vec::new();
    }
    payload
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .filter(|u| *u != 0)
        .collect()
}

/// Back / Volume+ / Volume- only. Direction and OK stay on Raw Input.
pub fn hogp_special_usages(payload: &[u8]) -> Vec<u16> {
    hogp_payload_usages(payload)
        .into_iter()
        .filter(|u| matches!(*u, 0x00F1 | 0x0080 | 0x0081))
        .collect()
}

/// Extract Windows virtual-keys from a raw HID input report.
///
/// Handles both keyboard-page arrays (byte `0xF1` = Back) and Consumer
/// 16-bit little-endian usages (`0x0224` = AC Back).
pub fn parse_hid_report_vkeys(report: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    if report.is_empty() {
        return out;
    }

    for &b in report {
        if b == 0 {
            continue;
        }
        if let Some(vk) = usage_to_vkey(u32::from(b)) {
            push_unique(&mut out, vk);
        }
    }

    for chunk in report.chunks_exact(2) {
        let usage = u16::from_le_bytes([chunk[0], chunk[1]]) as u32;
        if usage == 0 {
            continue;
        }
        if let Some(vk) = usage_to_vkey(usage).or_else(|| consumer_usage_to_vkey(usage)) {
            push_unique(&mut out, vk);
        }
    }

    // Optional report-id prefix (1..15) then 16-bit usages.
    if report.len() >= 3 && report[0] > 0 && report[0] < 16 {
        for chunk in report[1..].chunks_exact(2) {
            let usage = u16::from_le_bytes([chunk[0], chunk[1]]) as u32;
            if usage == 0 {
                continue;
            }
            if let Some(vk) = usage_to_vkey(usage).or_else(|| consumer_usage_to_vkey(usage)) {
                push_unique(&mut out, vk);
            }
        }
    }

    out
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

    #[test]
    fn back_usage_maps_to_browser_back_vkey() {
        assert_eq!(usage_to_vkey(0x00F1), Some(166));
        assert_eq!(consumer_usage_to_vkey(0x0224), Some(166));
    }

    #[test]
    fn hid_report_detects_keyboard_back() {
        assert_eq!(parse_hid_report_vkeys(&[0x00, 0x00, 0xF1, 0x00, 0x00, 0x00, 0x00, 0x00]), vec![166]);
    }

    #[test]
    fn hid_report_detects_consumer_ac_back() {
        assert_eq!(parse_hid_report_vkeys(&[0x24, 0x02]), vec![166]);
        assert_eq!(parse_hid_report_vkeys(&[0x01, 0x24, 0x02]), vec![166]);
    }

    #[test]
    fn hid_report_release_is_empty() {
        assert!(parse_hid_report_vkeys(&[0x00, 0x00, 0x00, 0x00]).is_empty());
    }

    #[test]
    fn hogp_ioctl_keeps_only_back_and_volume() {
        let back = [0x01, 0x00, 0x00, 0xF1, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(hogp_special_usages(hogp_ioctl_payload(&back).unwrap()), vec![0x00F1]);

        let vol = [0x01, 0x00, 0x00, 0x80, 0x00, 0x81, 0x00, 0x00, 0x00];
        let mut got = hogp_special_usages(hogp_ioctl_payload(&vol).unwrap());
        got.sort_unstable();
        assert_eq!(got, vec![0x0080, 0x0081]);

        let ok_only = [0x01, 0x00, 0x00, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(hogp_special_usages(hogp_ioctl_payload(&ok_only).unwrap()).is_empty());

        assert!(hogp_ioctl_payload(&[0x00, 0x00, 0x00]).is_none());
    }
}

#[cfg(target_os = "windows")]
pub mod raw_input;

#[cfg(target_os = "windows")]
pub mod tap;
