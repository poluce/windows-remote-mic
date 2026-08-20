//! Audio endpoint model.
//!
//! An endpoint is a playback (or capture) device Windows exposes, e.g.
//! `CABLE Input` from VB-CABLE. The whole audio layer talks to this model,
//! so swapping to another virtual audio driver only changes this module.

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Direction of an audio endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndpointKind {
    /// A playback device we write the remote audio into (e.g. CABLE Input).
    Output,
    /// A capture device seen as a microphone by apps (e.g. CABLE Output).
    Input,
}

/// A single audio endpoint (device).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioEndpoint {
    /// Stable device id used for persistence (never the raw HW id).
    pub id: String,
    /// Human-readable name shown in the settings UI.
    pub name: String,
    pub kind: EndpointKind,
}

/// Default placeholder endpoint used before WASAPI enumeration is wired in.
pub fn placeholder_output() -> AudioEndpoint {
    AudioEndpoint {
        id: "cable-input".to_string(),
        name: "CABLE Input (VB-CABLE)".to_string(),
        kind: EndpointKind::Output,
    }
}

/// List output endpoints the app can write voice into.
///
/// Windows: enumerate WASAPI playback endpoints and expose the ones the user
/// can pick as a virtual sound card (VB-CABLE's CABLE Input).
/// Non-Windows: return a single placeholder so the UI can be previewed.
pub fn list_output_endpoints() -> Result<Vec<AudioEndpoint>> {
    #[cfg(target_os = "windows")]
    {
        crate::wasapi::list_output_endpoints()
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(vec![placeholder_output()])
    }
}

/// List capture endpoints (virtual microphone side, e.g. CABLE Output).
///
/// Windows: enumerate WASAPI capture endpoints.
/// Non-Windows: return an empty list so diagnostics stay honest in preview.
pub fn list_input_endpoints() -> Result<Vec<AudioEndpoint>> {
    #[cfg(target_os = "windows")]
    {
        crate::wasapi::list_input_endpoints()
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(Vec::new())
    }
}

/// Find the endpoint with the given persisted id.
pub fn find_endpoint_by_id(id: &str) -> Result<AudioEndpoint> {
    list_output_endpoints()?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| crate::error::AudioError::EndpointNotFound(id.to_string()))
}
