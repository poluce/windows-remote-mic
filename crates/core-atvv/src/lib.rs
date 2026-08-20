//! core-atvv — Android TV Voice-over-BLE protocol + IMA/DVI ADPCM voice decoder.

pub mod adpcm;
pub mod error;
pub mod frame;
pub mod protocol;
pub mod session;

pub use adpcm::ImaAdpcmDecoder;
pub use error::{AtvvError, Result};
pub use frame::AudioFrameAssembler;
pub use protocol::{AtvvCapabilities, ControlEvent};
pub use session::{VoiceSession, VoiceState};
