//! ATVV（Android TV 蓝牙语音）协议常量与结构。
//!
//! 协议事实来自社区验证过的实现汇总
//!（参见 docs/PROTOCOL.md）。这里的值是互操作事实。

/// ATVV GATT 服务 UUID。
pub const ATVV_SERVICE_UUID: &str = "AB5E0001-5A21-4F05-BC7D-AF01F617B664";
/// 发送（TX）特征。
pub const ATVV_TRANSMIT_UUID: &str = "AB5E0002-5A21-4F05-BC7D-AF01F617B664";
/// 音频特征。
pub const ATVV_AUDIO_UUID: &str = "AB5E0003-5A21-4F05-BC7D-AF01F617B664";
/// 控制特征。
pub const ATVV_CONTROL_UUID: &str = "AB5E0004-5A21-4F05-BC7D-AF01F617B664";

/// RC003 编解码器的语音采样率。
pub const REMOTE_SAMPLE_RATE_HZ: u32 = 16_000;
/// 压缩格式。
pub const CODEC_NAME: &str = "IMA/DVI ADPCM";

/// 标称音频帧长度（字节）。
pub const DEFAULT_FRAME_BYTES: usize = 120;

// 设备 -> 主机控制操作码。
pub const OPCODE_AUDIO_STOP: u8 = 0x00;
pub const OPCODE_AUDIO_START: u8 = 0x04;
pub const OPCODE_MIC_BUTTON: u8 = 0x08;
pub const OPCODE_AUDIO_SYNC: u8 = 0x0A;
pub const OPCODE_CAPS: u8 = 0x0B;

/// 主机 -> 设备能力请求（v1.0）。
pub const GET_CAPABILITIES_V10: [u8; 6] = [0x0A, 0x01, 0x00, 0x00, 0x03, 0x03];

/// 主机 -> 设备 MIC_OPEN 命令。
pub fn mic_open_command(version: u16) -> Vec<u8> {
    if version >= 0x0100 {
        vec![0x0C, 0x00]
    } else {
        vec![0x0C, 0x00, 0x00]
    }
}

/// 主机 -> 设备 MIC_CLOSE 命令。
pub fn mic_close_command(version: u16, session_id: u8) -> Vec<u8> {
    if version >= 0x0100 {
        vec![0x0D, session_id]
    } else {
        vec![0x0D]
    }
}

/// 遥控器在能力协商期间报告的编解码器能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AtvvCapabilities {
    pub version: u16,
    pub codecs: u8,
    pub interaction: u8,
    pub frame_size: usize,
    pub selected_codec: u8,
    pub sample_rate_hz: u32,
}

impl AtvvCapabilities {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 7 || data[0] != OPCODE_CAPS {
            return None;
        }
        let version = (u16::from(data[1]) << 8) | u16::from(data[2]);

        let (codecs, interaction) = if version >= 0x0100 {
            let mut codecs = data[3];
            let mut interaction = data[4];
            if codecs == 0 && data.len() >= 9 && data[4] & 0x03 != 0 {
                codecs = data[4];
                interaction = 0x03;
            }
            (codecs, interaction)
        } else {
            if data.len() < 9 {
                return None;
            }
            (data[4], 0x00)
        };

        let raw_frame_size = (u16::from(data[5]) << 8) | u16::from(data[6]);
        let frame_size = if raw_frame_size == 0 {
            DEFAULT_FRAME_BYTES
        } else {
            raw_frame_size as usize
        };
        let selected_codec = if codecs & 0x02 != 0 { 0x02 } else { 0x01 };
        let sample_rate_hz = if selected_codec == 0x02 {
            16_000
        } else {
            8_000
        };

        Some(Self {
            version,
            codecs,
            interaction,
            frame_size,
            selected_codec,
            sample_rate_hz,
        })
    }
}

/// 会话状态机使用的语义事件（底层 API 保持稳定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlEvent {
    MicOpen,
    StreamStart,
    StreamStop,
    MicExtend,
}

/// 从 Control 特征通知解析出的原始事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawControlEvent {
    Caps(AtvvCapabilities),
    MicButtonPressed,
    AudioStarted { session_id: Option<u8> },
    AudioStopped,
    AudioSynced { predictor: i16, step_index: u8 },
    Unknown(u8),
}

/// 解析 Control 特征通知载荷。
pub fn parse_control(payload: &[u8]) -> Option<RawControlEvent> {
    let opcode = *payload.first()?;
    Some(match opcode {
        OPCODE_CAPS => RawControlEvent::Caps(AtvvCapabilities::parse(payload)?),
        OPCODE_MIC_BUTTON => RawControlEvent::MicButtonPressed,
        OPCODE_AUDIO_START => RawControlEvent::AudioStarted {
            session_id: payload.get(3).copied(),
        },
        OPCODE_AUDIO_STOP => RawControlEvent::AudioStopped,
        OPCODE_AUDIO_SYNC if payload.len() >= 7 => {
            let predictor = i16::from_be_bytes([payload[4], payload[5]]);
            RawControlEvent::AudioSynced {
                predictor,
                step_index: payload[6],
            }
        }
        other => RawControlEvent::Unknown(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_v10_caps() {
        // 0B 01 00 00 02 00 78 -> 版本 0x0100，codecs 0x02，帧长 120
        let caps = AtvvCapabilities::parse(&[0x0B, 0x01, 0x00, 0x02, 0x00, 0x00, 0x78]).unwrap();
        assert_eq!(caps.version, 0x0100);
        assert_eq!(caps.sample_rate_hz, 16_000);
        assert_eq!(caps.frame_size, 120);
        assert_eq!(caps.selected_codec, 0x02);
    }

    #[test]
    fn parse_control_recognizes_events() {
        assert_eq!(
            parse_control(&[OPCODE_MIC_BUTTON]),
            Some(RawControlEvent::MicButtonPressed)
        );
        assert_eq!(
            parse_control(&[OPCODE_AUDIO_START, 0, 0, 7]),
            Some(RawControlEvent::AudioStarted {
                session_id: Some(7)
            })
        );
        assert_eq!(
            parse_control(&[OPCODE_AUDIO_STOP]),
            Some(RawControlEvent::AudioStopped)
        );
        assert_eq!(parse_control(&[0x77]), Some(RawControlEvent::Unknown(0x77)));
    }

    #[test]
    fn mic_open_close_commands() {
        assert_eq!(mic_open_command(0x0100), vec![0x0C, 0x00]);
        assert_eq!(mic_open_command(0x0001), vec![0x0C, 0x00, 0x00]);
        assert_eq!(mic_close_command(0x0100, 7), vec![0x0D, 7]);
        assert_eq!(mic_close_command(0x0001, 7), vec![0x0D]);
    }

    #[test]
    fn uuids_are_upper_hex() {
        for uuid in [
            ATVV_SERVICE_UUID,
            ATVV_TRANSMIT_UUID,
            ATVV_AUDIO_UUID,
            ATVV_CONTROL_UUID,
        ] {
            assert_eq!(uuid, uuid.to_uppercase());
            assert!(uuid.chars().filter(|c| *c == '-').count() == 4);
        }
    }
}
