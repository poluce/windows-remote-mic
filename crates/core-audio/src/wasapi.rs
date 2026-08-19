//! Windows WASAPI output endpoint enumeration.
//!
//! Only compiled and used on `target_os = "windows"`.

use windows::core::PWSTR;
use windows::Win32::Media::Audio::{
    eRender, DEVICE_STATE_ACTIVE, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;

use crate::endpoint::{AudioEndpoint, EndpointKind};
use crate::error::Result;

/// Enumerate currently-active WASAPI playback (render) endpoints.
///
/// Each endpoint is exposed to the app as `AudioEndpoint{id, name, Output}`.
/// The `id` is the Windows WASAPI device id, which is what we persist so the
/// chosen virtual sound card survives restarts.
pub fn list_output_endpoints() -> Result<Vec<AudioEndpoint>> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let collection = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let count = collection.GetCount()?;

        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            if let Some(endpoint) = collect_endpoint(i, &collection)? {
                out.push(endpoint);
            }
        }
        Ok(out)
    }
}

unsafe fn collect_endpoint(
    index: u32,
    collection: &windows::Win32::Media::Audio::IMMDeviceCollection,
) -> Result<Option<AudioEndpoint>> {
    unsafe {
        let device = collection.Item(index)?;

        let id_pw: PWSTR = device.GetId()?;
        let id = id_pw.to_string().map_err(|e| {
            crate::error::AudioError::Windows(format!("bad device id: {e}"))
        })?;
        // IMMDevice::GetId allocates with CoTaskMemAlloc — the caller frees it.
        CoTaskMemFree(Some(id_pw.as_ptr() as *const core::ffi::c_void));

        let store = device.OpenPropertyStore(STGM_READ)?;
        let mut pv: PROPVARIANT = store.GetValue(&PKEY_Device_FriendlyName)?;
        let name = propvar_to_string(&pv);
        let result = PropVariantClear(&mut pv);
        if result.is_err() {
            // Clearing failure is not fatal; we still take what we read.
        }

        let name = if name.is_empty() { id.clone() } else { name };

        Ok(Some(AudioEndpoint {
            id,
            name,
            kind: EndpointKind::Output,
        }))
    }
}

/// Read a `VT_LPWSTR` PROPVARIANT as a Rust String.
unsafe fn propvar_to_string(pv: &PROPVARIANT) -> String {
    unsafe {
        if pv.Anonymous.Anonymous.vt == VT_LPWSTR {
            let pw = pv.Anonymous.Anonymous.Anonymous.pwszVal;
            pw.to_string().unwrap_or_default()
        } else {
            String::new()
        }
    }
}
