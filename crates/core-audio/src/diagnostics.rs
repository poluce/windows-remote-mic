//! 音频诊断：端点可见性 + 虚拟声卡检查。

use serde::Serialize;

use crate::endpoint::{list_input_endpoints, list_output_endpoints, AudioEndpoint};

/// 音频链路健康状况快照。
#[derive(Debug, Clone, Serialize, Default)]
pub struct AudioDiagnostics {
    pub output_endpoints: Vec<AudioEndpoint>,
    pub input_endpoints: Vec<AudioEndpoint>,
    pub has_vb_cable: bool,
    pub cable_input_present: bool,
    pub cable_output_present: bool,
}

/// 运行端点枚举并得出虚拟声卡判断。
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

    #[cfg(not(target_os = "windows"))]
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
