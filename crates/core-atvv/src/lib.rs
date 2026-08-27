//! core-atvv — Android TV 蓝牙语音（ATVV）协议 + IMA/DVI ADPCM 语音解码器。

pub mod adpcm;
pub mod error;
pub mod frame;
pub mod protocol;
pub mod session;

pub use adpcm::{ImaAdpcmDecoder, ImaAdpcmEncoder};
pub use error::{AtvvError, Result};
pub use frame::AudioFrameAssembler;
pub use protocol::{AtvvCapabilities, ControlEvent};
pub use session::{VoiceSession, VoiceState};
