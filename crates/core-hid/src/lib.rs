//! core-hid — RC003 HID usage 映射表与键盘报告解析（纯逻辑）。

use core_mapping::ButtonId;

/// 遥控器普通按键使用的键盘 usage page。
pub const HID_USAGE_PAGE_KEYBOARD: u16 = 0x07;

/// RC003 按键 -> HID 键盘 usage。
///
/// 麦克风通常通过 ATVV 控制通道到达，但设备也可以将其报告为
/// 键盘 F5（usage 0x3E）；我们将其保留为兜底，以便纯 HID 路径仍能检测到语音键。
pub const BUTTON_USAGE_MAP: [(u32, ButtonId); 14] = [
    (0x003E, ButtonId::Mic), // 语音键的 F5 兜底
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
    (0x007F, ButtonId::Power), // volume_mute usage 不是物理按键；并非打算映射为电源
];

/// 将 HID usage id 映射为物理按键。
pub fn usage_to_button(usage: u32) -> Option<ButtonId> {
    // 上面的 volume_mute 条目有意不参与解码：
    // 遥控器没有物理静音键。
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

/// 将物理按键反向映射为 HID usage id。
pub fn button_to_usage(button: ButtonId) -> Option<u32> {
    BUTTON_USAGE_MAP.iter().find_map(|(usage, b)| {
        if *b == button && usage_to_button(*usage) == Some(button) {
            Some(*usage)
        } else {
            None
        }
    })
}

/// 将 Windows 虚拟键反向解析为物理按键（usage 表的逆映射）。
///
/// 供按键调度器把 Raw Input 事件还原为物理按键；校准表可在此基础上
/// 覆盖。麦克风键（F5 兜底 116）刻意不在结果中：麦克风由 ATVV 控制
/// 流处理，避免 F5 等同虚拟键误触发语音。
pub fn vkey_to_button(vkey: u16) -> Option<ButtonId> {
    BUTTON_USAGE_MAP.iter().find_map(|&(usage, button)| {
        if button == ButtonId::Mic {
            return None;
        }
        if usage_to_vkey(usage) == Some(vkey) && usage_to_button(usage) == Some(button) {
            Some(button)
        } else {
            None
        }
    })
}

/// 解析单个 Raw Input 键盘报告，得到按下的按键列表。
///
/// 每个非零字节都是一个键盘 usage；未知 usage 会被忽略。
pub fn parse_keyboard_report(report: &[u8]) -> Vec<ButtonId> {
    report
        .iter()
        .filter_map(|&b| usage_to_button(u32::from(b)))
        .collect()
}

/// 测试器/映射层使用的 Windows 虚拟键（针对键盘页 usage）。
pub fn usage_to_vkey(usage: u32) -> Option<u16> {
    match usage {
        0x003E => Some(116), // F5 / 麦克风兜底
        0x00F1 => Some(166), // RC003 返回（厂商键盘 usage）
        0x0028 => Some(13),  // 回车
        0x0035 => Some(180), // 电视
        0x004A => Some(172), // 主页
        0x004F => Some(39),  // 右
        0x0050 => Some(37),  // 左
        0x0051 => Some(40),  // 下
        0x0052 => Some(38),  // 上
        0x0065 => Some(93),  // 菜单 / 应用
        0x0066 => Some(255), // 电源
        0x0080 => Some(175), // 音量加
        0x0081 => Some(174), // 音量减
        _ => None,
    }
}

/// 消费类控制（usage page 0x0C）-> Windows 虚拟键。
pub fn consumer_usage_to_vkey(usage: u32) -> Option<u16> {
    match usage {
        0x0224 => Some(166),          // 返回
        0x0223 | 0x018A => Some(172), // 主页
        0x00E9 => Some(175),          // 音量加
        0x00EA => Some(174),          // 音量减
        0x00E2 => Some(173),          // 静音
        0x0040 => Some(93),           // 菜单
        _ => None,
    }
}

fn push_unique(out: &mut Vec<u16>, vk: u16) {
    if !out.contains(&vk) {
        out.push(vk);
    }
}

/// RC003 上 HidOverGatt 特征读取 IOCTL 载荷：
/// 支持 3 字节前缀 [01 00 00] + usage 载荷，或直接的 usage 报告。
pub fn hogp_ioctl_payload(data: &[u8]) -> Option<&[u8]> {
    if data.starts_with(&[0x01, 0x00, 0x00]) && data.len() >= 3 {
        Some(&data[3..])
    } else if !data.is_empty() {
        Some(data)
    } else {
        None
    }
}

/// 从 HOGP 载荷中解析小端键盘页 usage。
pub fn hogp_payload_usages(payload: &[u8]) -> Vec<u16> {
    if payload.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    // 优先尝试 2 字节小端序（例如 F1 00 -> 0x00F1）
    for chunk in payload.chunks_exact(2) {
        let u = u16::from_le_bytes([chunk[0], chunk[1]]);
        if u != 0 {
            out.push(u);
        }
    }
    // 如果未能按 2 字节解析出任何有效 usage，尝试按 1 字节 usage 处理
    if out.is_empty() {
        for &b in payload {
            if b != 0 {
                out.push(u16::from(b));
            }
        }
    }
    out
}

/// 仅返回 / 音量加 / 音量减。方向键和确定键仍走 Raw Input。
pub fn hogp_special_usages(payload: &[u8]) -> Vec<u16> {
    hogp_payload_usages(payload)
        .into_iter()
        .filter(|u| matches!(*u, 0x00F1 | 0x0080 | 0x0081))
        .collect()
}

/// 从原始 HID 输入报告中提取 Windows 虚拟键。
///
/// 同时处理键盘页数组（字节 `0xF1` = 返回）和消费类
/// 16 位小端 usage（`0x0224` = 返回）。
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

    // 可选的报告 ID 前缀（1..15），随后是 16 位 usage。
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
        // 上(0x52)、确定(0x28)、返回(0xF1)
        let buttons = parse_keyboard_report(&[0x52, 0x28, 0xF1, 0x00]);
        assert_eq!(buttons, vec![ButtonId::Up, ButtonId::Ok, ButtonId::Back]);
    }

    #[test]
    fn unknown_usages_are_ignored() {
        assert_eq!(parse_keyboard_report(&[0x01, 0xFF, 0x00]), Vec::new());
    }

    #[test]
    fn vkey_reverse_map_resolves_buttons() {
        assert_eq!(vkey_to_button(38), Some(ButtonId::Up));
        assert_eq!(vkey_to_button(37), Some(ButtonId::Left));
        assert_eq!(vkey_to_button(13), Some(ButtonId::Ok));
        assert_eq!(vkey_to_button(166), Some(ButtonId::Back));
        assert_eq!(vkey_to_button(175), Some(ButtonId::VolumeUp));
        assert_eq!(vkey_to_button(174), Some(ButtonId::VolumeDown));
        assert_eq!(vkey_to_button(93), Some(ButtonId::Menu));
        assert_eq!(vkey_to_button(255), Some(ButtonId::Power));
        // 麦克风（F5）与未映射的静音键不参与反查
        assert_eq!(vkey_to_button(116), None);
        assert_eq!(vkey_to_button(173), None);
    }

    #[test]
    fn back_usage_maps_to_browser_back_vkey() {
        assert_eq!(usage_to_vkey(0x00F1), Some(166));
        assert_eq!(consumer_usage_to_vkey(0x0224), Some(166));
    }

    #[test]
    fn hid_report_detects_keyboard_back() {
        assert_eq!(
            parse_hid_report_vkeys(&[0x00, 0x00, 0xF1, 0x00, 0x00, 0x00, 0x00, 0x00]),
            vec![166]
        );
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
        assert_eq!(
            hogp_special_usages(hogp_ioctl_payload(&back).unwrap()),
            vec![0x00F1]
        );

        let vol = [0x01, 0x00, 0x00, 0x80, 0x00, 0x81, 0x00, 0x00, 0x00];
        let mut got = hogp_special_usages(hogp_ioctl_payload(&vol).unwrap());
        got.sort_unstable();
        assert_eq!(got, vec![0x0080, 0x0081]);

        let ok_only = [0x01, 0x00, 0x00, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert!(hogp_special_usages(hogp_ioctl_payload(&ok_only).unwrap()).is_empty());

        // 全 0 的直接报告（松开）应被解析为空 usage，而不是被丢弃。
        assert_eq!(
            hogp_ioctl_payload(&[0x00, 0x00, 0x00]),
            Some(&[0x00, 0x00, 0x00][..])
        );
        assert!(hogp_payload_usages(&[0x00, 0x00, 0x00]).is_empty());
    }
}

#[cfg(target_os = "windows")]
pub mod raw_input;

#[cfg(target_os = "windows")]
pub mod tap;
