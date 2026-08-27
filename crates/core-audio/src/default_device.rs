//! 仅 Windows 的辅助函数，用于临时切换默认麦克风。
//!
//! Win+H 监听默认输入设备，因此当应用驱动语音会话时，
//! 它会将默认输入切换到 CABLE Output，并在会话结束时恢复
//! 之前的设备。

#![allow(non_snake_case, non_upper_case_globals)]

use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows_core::{interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};

use crate::endpoint::list_input_endpoints;
use crate::error::{AudioError, Result};

/// `PolicyConfigClient` coclass 的 CLSID。
const CLSID_PolicyConfigClient: GUID = GUID::from_values(
    0x870af99c,
    0x171d,
    0x4f9e,
    [0xaf, 0x0d, 0xe6, 0x3d, 0xf4, 0x0c, 0x2b, 0xc9],
);

/// 用于设置默认端点的未文档化 `IPolicyConfig` COM 接口。
#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut core::ffi::c_void) -> HRESULT;
    fn GetDeviceFormat(
        &self,
        device: PCWSTR,
        def: bool,
        format: *mut *mut core::ffi::c_void,
    ) -> HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(
        &self,
        device: PCWSTR,
        endpoint: *mut core::ffi::c_void,
        mix: *mut core::ffi::c_void,
    ) -> HRESULT;
    fn GetProcessingPeriod(
        &self,
        device: PCWSTR,
        def: bool,
        default: *mut *mut core::ffi::c_void,
        min: *mut *mut core::ffi::c_void,
    ) -> HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, period: *mut core::ffi::c_void) -> HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut *mut core::ffi::c_void) -> HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn GetPropertyValue(
        &self,
        device: PCWSTR,
        store: bool,
        key: *const core::ffi::c_void,
        value: *mut *mut core::ffi::c_void,
    ) -> HRESULT;
    fn SetPropertyValue(
        &self,
        device: PCWSTR,
        store: bool,
        key: *const core::ffi::c_void,
        value: *const core::ffi::c_void,
    ) -> HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: i32) -> HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: bool) -> HRESULT;
}

/// RAII 守卫：将默认输入切换到 CABLE Output，并在析构时恢复
/// 之前的默认输入。
pub struct DefaultInputGuard {
    previous: Option<String>,
}

impl DefaultInputGuard {
    /// 查找 CABLE Output，记住之前的默认输入，然后切换。
    pub fn switch_to_cable_output() -> Result<Self> {
        let cable_id = find_cable_output_id()?;
        let previous = crate::wasapi::default_input_endpoint_id().ok();
        let previous_name = crate::endpoint::default_input_name();
        core_log::log_line(&format!(
            "[default-device] previous default input name={:?}",
            previous_name
        ));

        if previous.as_deref() == Some(cable_id.as_str()) {
            return Ok(Self { previous: None });
        }

        set_default_input(&cable_id)?;
        core_log::log_line(&format!(
            "[default-device] switched default input to CABLE Output (previous_id={:?})",
            previous
        ));

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

#[inline]
pub(crate) fn ensure_com_initialized() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
}

/// 将默认输入（麦克风）端点设置为 `endpoint_id`（适用于所有角色）。
pub fn set_default_input(endpoint_id: &str) -> Result<()> {
    ensure_com_initialized();
    unsafe {
        let policy: IPolicyConfig = CoCreateInstance(&CLSID_PolicyConfigClient, None, CLSCTX_ALL)?;

        let mut device_wide: Vec<u16> = endpoint_id.encode_utf16().collect();
        device_wide.push(0);
        let device = PCWSTR(device_wide.as_ptr());

        for role in [0i32, 1, 2] {
            policy.SetDefaultEndpoint(device, role).ok()?;
        }

        Ok(())
    }
}

fn find_cable_output_id() -> Result<String> {
    let inputs = list_input_endpoints()?;
    inputs
        .into_iter()
        .find(|e| e.name.to_lowercase().contains("cable output"))
        .map(|e| e.id)
        .ok_or_else(|| AudioError::Windows("CABLE Output endpoint not found".to_string()))
}
