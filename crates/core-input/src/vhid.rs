//! 通过 WinUHid 虚拟 HID 键盘向系统提交报告（不是 SendInput）。
//!
//! 驱动：https://github.com/cgutman/WinUHid （UMDF + 收件箱 vhf.sys）
//! 应用加载 `WinUHid.dll`，创建一把标准 Boot Keyboard，再提交 8 字节报告。
//! 这样按键走 HID 类驱动，豆包等输入法会当成真实键盘。

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use windows::core::{s, w, PCSTR, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, FreeLibrary, GetLastError, GENERIC_READ, GENERIC_WRITE, HMODULE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetProcAddress, LoadLibraryW};

use crate::error::{InputError, Result};
use crate::hid_kbd::{
    consumer_bit, ConsumerState, KeyboardState, CONSUMER_REPORT_DESCRIPTOR, CONSUMER_REPORT_LEN,
    REPORT_DESCRIPTOR, REPORT_LEN,
};

const VID: u16 = 0x1209;
const PID: u16 = 0x524B; // 'RK'

const KEYBOARD_INSTANCE: &str = "RemoteMic.VirtualKeyboard";
const CONSUMER_INSTANCE: &str = "RemoteMic.VirtualConsumer";

#[repr(C, packed)]
struct WinUHidDeviceConfig {
    supported_events: i32,
    vendor_id: u16,
    product_id: u16,
    version_number: u16,
    report_descriptor_length: u16,
    report_descriptor: *const c_void,
    container_id: [u8; 16],
    instance_id: *const u16,
    hardware_ids: *const u16,
    read_report_period_us: u32,
}

type PDevice = *mut c_void;
type FnGetVer = unsafe extern "C" fn() -> u32;
type FnCreate = unsafe extern "C" fn(*const WinUHidDeviceConfig) -> PDevice;
type FnSubmit = unsafe extern "C" fn(PDevice, *const c_void, u32) -> i32;
type FnStart = unsafe extern "C" fn(PDevice, *mut c_void, *mut c_void) -> i32;
type FnDestroy = unsafe extern "C" fn(PDevice);

struct Api {
    _dll: HMODULE,
    create: FnCreate,
    submit: FnSubmit,
    start: FnStart,
    destroy: FnDestroy,
}

struct Device {
    api: Api,
    handle: PDevice,
    keyboard: KeyboardState,
    consumer: ConsumerState,
}

unsafe impl Send for Device {}

/// 键盘设备（Boot Keyboard）。保持既有描述符不变，避免影响已验证的按键行为。
static KEYBOARD: Mutex<Option<Device>> = Mutex::new(None);
/// 媒体键设备（Consumer Control）。键盘页没有播放/暂停类 usage，单独一台设备承载。
static CONSUMER: Mutex<Option<Device>> = Mutex::new(None);

fn dll_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut buf = [0u16; 520];
    let n = unsafe { GetModuleFileNameW(None, &mut buf) };
    if n > 0 {
        let exe = String::from_utf16_lossy(&buf[..n as usize]);
        if let Some(dir) = PathBuf::from(exe).parent() {
            // 安装器把整个虚拟 HID 包释放到 <安装目录>\vhid（见 windows\hooks.nsh）。
            out.push(dir.join("vhid").join("WinUHid.dll"));
            out.push(dir.join("WinUHid.dll"));
            out.push(dir.join("winuhid").join("WinUHid.dll"));
        }
    }
    // 开发期回退：scripts\build-winuhid.ps1 会把 DLL 放到这里。
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join("RemoteMic\\WinUHid\\WinUHid.dll"));
    }
    if let Ok(pf) = std::env::var("ProgramFiles") {
        out.push(PathBuf::from(&pf).join("WinUHid\\WinUHid.dll"));
        out.push(PathBuf::from(&pf).join("RemoteMic\\WinUHid\\WinUHid.dll"));
    }
    out
}

fn load_api() -> Result<Api> {
    let mut last = "找不到 WinUHid.dll".to_string();
    for path in dll_candidates() {
        if !path.is_file() {
            continue;
        }
        let wide: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let dll = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }.map_err(|e| {
            InputError::Windows(format!("LoadLibrary {} 失败：{e}", path.display()))
        })?;
        if dll.is_invalid() {
            last = format!("LoadLibrary {} 失败", path.display());
            continue;
        }
        unsafe fn proc<T>(dll: HMODULE, name: PCSTR) -> Option<T> {
            GetProcAddress(dll, name).map(|p| std::mem::transmute_copy(&p))
        }
        let get_ver: Option<FnGetVer> =
            unsafe { proc(dll, s!("WinUHidGetDriverInterfaceVersion")) };
        let create: Option<FnCreate> = unsafe { proc(dll, s!("WinUHidCreateDevice")) };
        let submit: Option<FnSubmit> = unsafe { proc(dll, s!("WinUHidSubmitInputReport")) };
        let start: Option<FnStart> = unsafe { proc(dll, s!("WinUHidStartDevice")) };
        let destroy: Option<FnDestroy> = unsafe { proc(dll, s!("WinUHidDestroyDevice")) };
        let (Some(create), Some(submit), Some(start), Some(destroy)) =
            (create, submit, start, destroy)
        else {
            unsafe {
                let _ = FreeLibrary(dll);
            }
            last = format!("{} 缺少 WinUHid 导出函数", path.display());
            continue;
        };
        if let Some(get_ver) = get_ver {
            let ver = unsafe { get_ver() };
            if ver == 0 {
                let err = unsafe { GetLastError() };
                unsafe {
                    let _ = FreeLibrary(dll);
                }
                return Err(InputError::Windows(format!(
                    "WinUHid 驱动未安装或不可用（GetLastError={err:?}）。请管理员运行 scripts/install-winuhid.ps1 后重启应用。"
                )));
            }
            crate::log_line(&format!(
                "[vhid] WinUHid 驱动接口版本 {ver}，dll={}",
                path.display()
            ));
        }
        return Ok(Api {
            _dll: dll,
            create,
            submit,
            start,
            destroy,
        });
    }
    Err(InputError::Windows(format!(
        "{last}。请先运行 scripts/build-winuhid.ps1，再管理员运行 scripts/install-winuhid.ps1。"
    )))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyboard,
    Consumer,
}

impl Kind {
    fn instance(self) -> &'static str {
        match self {
            Kind::Keyboard => KEYBOARD_INSTANCE,
            Kind::Consumer => CONSUMER_INSTANCE,
        }
    }

    fn descriptor(self) -> &'static [u8] {
        match self {
            Kind::Keyboard => REPORT_DESCRIPTOR,
            Kind::Consumer => CONSUMER_REPORT_DESCRIPTOR,
        }
    }

    fn empty_report(self) -> Vec<u8> {
        match self {
            Kind::Keyboard => vec![0u8; REPORT_LEN],
            Kind::Consumer => vec![0u8; CONSUMER_REPORT_LEN],
        }
    }

    fn ready_log(self) -> &'static str {
        match self {
            Kind::Keyboard => "[vhid] 虚拟 HID 键盘已就绪",
            Kind::Consumer => "[vhid] 虚拟 HID 媒体键设备已就绪",
        }
    }
}

fn create_device(api: Api, kind: Kind) -> Result<Device> {
    let instance: Vec<u16> = kind
        .instance()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let descriptor = kind.descriptor();
    let config = WinUHidDeviceConfig {
        supported_events: 0,
        vendor_id: VID,
        product_id: PID,
        version_number: 1,
        report_descriptor_length: descriptor.len() as u16,
        report_descriptor: descriptor.as_ptr() as *const c_void,
        container_id: [0; 16],
        instance_id: instance.as_ptr(),
        hardware_ids: std::ptr::null(),
        read_report_period_us: 0,
    };
    let handle = unsafe { (api.create)(&config) };
    if handle.is_null() {
        let err = unsafe { GetLastError() };
        return Err(InputError::Windows(format!(
            "WinUHidCreateDevice 失败（{err:?}）。驱动可能未安装。"
        )));
    }
    let started = unsafe { (api.start)(handle, std::ptr::null_mut(), std::ptr::null_mut()) };
    if started == 0 {
        unsafe { (api.destroy)(handle) };
        let err = unsafe { GetLastError() };
        return Err(InputError::Windows(format!(
            "WinUHidStartDevice 失败（{err:?}）"
        )));
    }
    let device = Device {
        api,
        handle,
        keyboard: KeyboardState::default(),
        consumer: ConsumerState::default(),
    };
    submit_report(&device, &kind.empty_report())?;
    crate::log_line(kind.ready_log());
    Ok(device)
}

fn submit_report(device: &Device, report: &[u8]) -> Result<()> {
    let ok = unsafe {
        (device.api.submit)(
            device.handle,
            report.as_ptr() as *const c_void,
            report.len() as u32,
        )
    };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        return Err(InputError::Windows(format!(
            "WinUHidSubmitInputReport 失败（{err:?}）"
        )));
    }
    Ok(())
}

fn with_device<T>(
    slot: &'static Mutex<Option<Device>>,
    kind: Kind,
    f: impl FnOnce(&mut Device) -> Result<T>,
) -> Result<T> {
    let mut guard = slot
        .lock()
        .map_err(|_| InputError::Windows("虚拟 HID 锁损坏".into()))?;
    if guard.is_none() {
        let api = load_api()?;
        *guard = Some(create_device(api, kind)?);
    }
    f(guard.as_mut().unwrap())
}

/// 把 token 分成「键盘」与「媒体键」两组：媒体键走独立的 Consumer 设备。
fn split_tokens<'a>(tokens: &[&'a str]) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut keyboard = Vec::new();
    let mut media = Vec::new();
    for token in tokens {
        if consumer_bit(token).is_some() {
            media.push(*token);
        } else {
            keyboard.push(*token);
        }
    }
    (keyboard, media)
}

pub fn key_down(tokens: &[&str]) -> Result<()> {
    let (keyboard, media) = split_tokens(tokens);
    if !keyboard.is_empty() {
        with_device(&KEYBOARD, Kind::Keyboard, |dev| {
            dev.keyboard
                .press_tokens(&keyboard)
                .map_err(InputError::Windows)?;
            let report = dev.keyboard.report();
            crate::log_line(&format!(
                "[vhid] 按住 {}  report={:02X?}",
                keyboard.join("+"),
                report
            ));
            submit_report(dev, &report)
        })?;
    }
    if !media.is_empty() {
        with_device(&CONSUMER, Kind::Consumer, |dev| {
            for token in &media {
                dev.consumer.press_token(token);
            }
            let report = dev.consumer.report();
            crate::log_line(&format!(
                "[vhid] 按住 {}  report={:02X?}",
                media.join("+"),
                report
            ));
            submit_report(dev, &report)
        })?;
    }
    Ok(())
}

pub fn key_up(tokens: &[&str]) -> Result<()> {
    let (keyboard, media) = split_tokens(tokens);
    if !keyboard.is_empty() {
        with_device(&KEYBOARD, Kind::Keyboard, |dev| {
            dev.keyboard
                .release_tokens(&keyboard)
                .map_err(InputError::Windows)?;
            let report = dev.keyboard.report();
            crate::log_line(&format!(
                "[vhid] 松开 {}  report={:02X?}",
                keyboard.join("+"),
                report
            ));
            submit_report(dev, &report)
        })?;
    }
    if !media.is_empty() {
        with_device(&CONSUMER, Kind::Consumer, |dev| {
            for token in &media {
                dev.consumer.release_token(token);
            }
            let report = dev.consumer.report();
            crate::log_line(&format!(
                "[vhid] 松开 {}  report={:02X?}",
                media.join("+"),
                report
            ));
            submit_report(dev, &report)
        })?;
    }
    Ok(())
}

pub fn key_tap(tokens: &[&str]) -> Result<()> {
    key_down(tokens)?;
    std::thread::sleep(Duration::from_millis(20));
    key_up(tokens)
}

pub fn press_win_h() -> Result<()> {
    crate::log_line("[vhid] 按下 Win+H");
    key_tap(&["win", "h"])
}

pub fn press_escape() -> Result<()> {
    crate::log_line("[vhid] 按下 Escape");
    key_tap(&["esc"])
}

fn control_device_present() -> bool {
    let handle = unsafe {
        CreateFileW(
            w!("\\\\.\\WinUHid"),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    };
    match handle {
        Ok(h) => {
            unsafe {
                let _ = CloseHandle(h);
            }
            true
        }
        Err(_) => false,
    }
}

/// 探测虚拟 HID 是否可用（不创建键盘）。
pub fn probe() -> String {
    let device = control_device_present();
    match load_api() {
        Ok(_) if device => "WinUHid.dll 已加载，\\\\.\\WinUHid 可用".into(),
        Ok(_) => "WinUHid.dll 已加载，但控制设备 \\\\.\\WinUHid 不存在。请管理员运行 scripts/install-winuhid.ps1".into(),
        Err(e) => format!("虚拟 HID 未就绪：{e}"),
    }
}
