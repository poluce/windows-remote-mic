//! Windows-only helpers for temporarily switching the default microphone.
//!
//! Win+H listens to the default input device, so while the app is driving a
//! voice session it switches the default input to CABLE Output and restores
//! the previous device when the session ends.

use std::io::Write;
use std::process::Command;

use crate::endpoint::list_input_endpoints;
use crate::error::{AudioError, Result};

/// PowerShell script that uses the undocumented `IPolicyConfig` COM interface
/// to set the default audio endpoint. Deliberately ASCII-only.
const SET_DEFAULT_PS1: &str = r#"param([string]$DeviceId)
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

[ComImport, Guid(""870af99c-171d-4f9e-af0d-e63df40c2bc9"")]
public class PolicyConfigClient { }

[ComImport, Guid(""f8679f50-850a-41cf-9c72-430f290290c8""), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IPolicyConfig {
    [PreserveSig] int GetMixFormat([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, IntPtr ppFormat);
    [PreserveSig] int GetDeviceFormat([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, bool bDefault, IntPtr ppFormat);
    [PreserveSig] int ResetDeviceFormat([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName);
    [PreserveSig] int SetDeviceFormat([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, IntPtr pEndpointFormat, IntPtr MixFormat);
    [PreserveSig] int GetProcessingPeriod([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, bool bDefault, IntPtr pmftDefaultPeriod, IntPtr pmftMinimumPeriod);
    [PreserveSig] int SetProcessingPeriod([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, IntPtr pmftPeriod);
    [PreserveSig] int GetShareMode([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, IntPtr pMode);
    [PreserveSig] int SetShareMode([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, IntPtr mode);
    [PreserveSig] int GetPropertyValue([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, bool bFxStore, IntPtr key, IntPtr pv);
    [PreserveSig] int SetPropertyValue([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, bool bFxStore, IntPtr key, IntPtr pv);
    [PreserveSig] int SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)] string wszDeviceId, int eRole);
    [PreserveSig] int SetEndpointVisibility([MarshalAs(UnmanagedType.LPWStr)] string pszDeviceName, bool bVisible);
}
"@
$client = New-Object PolicyConfigClient
$policy = [IPolicyConfig]$client
foreach ($role in @(0, 1, 2)) {
    $hr = $policy.SetDefaultEndpoint($DeviceId, $role)
    if ($hr -ne 0) {
        Write-Error "SetDefaultEndpoint failed with HRESULT $hr"
        exit $hr
    }
}
"#;

/// RAII guard: switches the default input to CABLE Output, then restores the
/// previous default input when dropped.
pub struct DefaultInputGuard {
    previous: Option<String>,
}

impl DefaultInputGuard {
    /// Find CABLE Output, remember the previous default input, and switch.
    pub fn switch_to_cable_output() -> Result<Self> {
        let cable_id = find_cable_output_id()?;
        let previous = crate::wasapi::default_input_endpoint_id().ok();

        if previous.as_deref() == Some(cable_id.as_str()) {
            return Ok(Self { previous: None });
        }

        set_default_input(&cable_id)?;
        core_log::log_line(&format!(
            "[default-device] switched default input to CABLE Output (previous={:?})",
            previous
        ));

        // Let Windows propagate the endpoint change before verifying/using it.
        std::thread::sleep(std::time::Duration::from_millis(300));

        let current = crate::endpoint::default_input_name();
        core_log::log_line(&format!(
            "[default-device] after switch default_input={:?}",
            current
        ));
        if let Some(name) = current {
            if !name.to_lowercase().contains("cable output") {
                core_log::log_warn(
                    "[default-device] CABLE Output is NOT the active default input after switch",
                );
            }
        }

        Ok(Self { previous })
    }
}

impl Drop for DefaultInputGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            if let Err(e) = set_default_input(previous) {
                core_log::log_error(&format!(
                    "[default-device] restore default input failed: {e}"
                ));
            } else {
                core_log::log_line("[default-device] restored previous default input");
            }
        }
    }
}

/// Set the default input (microphone) endpoint to `endpoint_id`.
pub fn set_default_input(endpoint_id: &str) -> Result<()> {
    let script_path = std::env::temp_dir()
        .join(format!("remote_mic_set_default_device_{}.ps1", std::process::id()));
    {
        let mut file = std::fs::File::create(&script_path)
            .map_err(|e| AudioError::Windows(format!("write policy script: {e}")))?;
        file.write_all(SET_DEFAULT_PS1.as_bytes())
            .map_err(|e| AudioError::Windows(format!("write policy script: {e}")))?;
    }

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script_path)
        .arg("-DeviceId")
        .arg(endpoint_id)
        .output()
        .map_err(|e| AudioError::Windows(format!("run policy script: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AudioError::Windows(format!(
            "set default input failed: {stderr}"
        )));
    }

    Ok(())
}

fn find_cable_output_id() -> Result<String> {
    let inputs = list_input_endpoints()?;
    inputs
        .into_iter()
        .find(|e| e.name.to_lowercase().contains("cable output"))
        .map(|e| e.id)
        .ok_or_else(|| {
            AudioError::Windows("CABLE Output endpoint not found".to_string())
        })
}