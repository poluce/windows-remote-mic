//! ATVV (Android TV Voice-over-BLE) protocol constants and structures.
//!
//! NOTE: command *opcode byte values* are not hard-coded here. They must be
//! captured on real hardware before being finalised; until then the session
//! layer is driven by semantic `ControlEvent`s, which is what the rest of the
//! pipeline consumes anyway.

/// ATVV GATT Service UUID.
pub const ATVV_SERVICE_UUID: &str = "AB5E0001-5A21-4F05-BC7D-AF01F617B664";
/// Transmit (TX) characteristic.
pub const ATVV_TRANSMIT_UUID: &str = "AB5E0002-5A21-4F05-BC7D-AF01F617B664";
/// Audio characteristic.
pub const ATVV_AUDIO_UUID: &str = "AB5E0003-5A21-4F05-BC7D-AF01F617B664";
/// Control characteristic.
pub const ATVV_CONTROL_UUID: &str = "AB5E0004-5A21-4F05-BC7D-AF01F617B664";

/// Voice sample rate of the RC003 codec.
pub const REMOTE_SAMPLE_RATE_HZ: u32 = 16_000;
/// Compression format.
pub const CODEC_NAME: &str = "IMA/DVI ADPCM";

/// Nominal audio frame length in bytes (IMPORTANT: to be confirmed on hardware).
pub const AUDIO_FRAME_BYTES: usize = 120;

/// Codec capabilities the remote reports during capability negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AtvvCapabilities {
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub frames_per_second: u32,
}

impl AtvvCapabilities {
    /// Defaults matching the RC003 16 kHz mono profile.
    pub fn rc003_16k() -> Self {
        Self {
            sample_rate_hz: REMOTE_SAMPLE_RATE_HZ,
            channels: 1,
            frames_per_second: 60, // 120 bytes * 60 = 7200 B/s ≈ 16k mono 4-bit
        }
    }
}

/// Voice-control commands used by the session state machine.
///
/// These are semantic events; the mapping to raw BLE control bytes is filled
/// in once confirmed against a real remote (see module note above).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlEvent {
    MicOpen,
    StreamStart,
    StreamStop,
    MicExtend,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc003_capabilities_defaults() {
        let caps = AtvvCapabilities::rc003_16k();
        assert_eq!(caps.sample_rate_hz, 16_000);
        assert_eq!(caps.channels, 1);
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
