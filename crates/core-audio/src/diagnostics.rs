//! Audio diagnostics: endpoint visibility + virtual sound-card checks.

use serde::Serialize;

use crate::endpoint::{list_input_endpoints, list_output_endpoints, AudioEndpoint};

/// Snapshot of the audio route health.
#[derive(Debug, Clone, Serialize, Default)]
pub struct AudioDiagnostics {
    pub output_endpoints: Vec<AudioEndpoint>,
    pub input_endpoints: Vec<AudioEndpoint>,
    pub has_vb_cable: bool,
    pub cable_input_present: bool,
    pub cable_output_present: bool,
}

/// Run endpoint enumeration and derive the virtual sound-card verdict.
pub fn run() -> AudioDiagnostics {
    let output = list_output_endpoints().unwrap_or_default();
    let input = list_input_endpoints().unwrap_or_default();

    let cable_input_present = output.iter().any(|e| is_cable(&e.name));
    let cable_output_present = input.iter().any(|e| is_cable(&e.name));

    AudioDiagnostics {
        output_endpoints: output,
        input_endpoints: input,
        has_vb_cable: cable_input_present && cable_output_present,
        cable_input_present,
        cable_output_present,
    }
}

fn is_cable(name: &str) -> bool {
    name.to_lowercase().contains("cable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_windows_diagnostics_sees_placeholder_cable_input() {
        let d = run();
        assert!(d.cable_input_present);
    }

    #[test]
    fn cable_detection_is_case_insensitive() {
        assert!(is_cable("CABLE Input (VB-Audio)"));
        assert!(is_cable("vb-cable output"));
        assert!(!is_cable("Realtek Audio"));
    }
}
