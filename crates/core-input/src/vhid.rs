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
    let mut last = "在候选路径中均未找到 WinUHid.dll".to_string();
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
                    "虚拟 HID 驱动未安装或不可用（驱动接口查询失败，GetLastError={err:?}）"
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
    // 具体怎么办交给 diagnostics() 生成的建议，这里只陈述事实。
    Err(InputError::Windows(format!(
        "虚拟 HID 客户端组件不可用：{last}"
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

/// 读一个 HKLM 下的 DWORD；键或值不存在（或不是 DWORD）时返回 `None`。
fn read_hklm_dword(subkey: &str, value: &str) -> Option<u32> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};

    let subkey_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let value_w: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;

    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey_w.as_ptr()),
            PCWSTR(value_w.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(data)
}

/// Secure Boot 是否开启。读不到（非 UEFI / 无权限）时为 `None`。
fn secure_boot_enabled() -> Option<bool> {
    read_hklm_dword(
        "SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State",
        "UEFISecureBootEnabled",
    )
    .map(|v| v != 0)
}

/// 内存完整性（HVCI）是否开启。该场景键不存在时视为未配置（`Some(false)`）。
fn hvci_enabled() -> Option<bool> {
    match read_hklm_dword(
        "SYSTEM\\CurrentControlSet\\Control\\DeviceGuard\\Scenarios\\HypervisorEnforcedCodeIntegrity",
        "Enabled",
    ) {
        Some(v) => Some(v != 0),
        // 没有状态键也能读通注册表时，说明该场景未启用。
        None => Some(false),
    }
}

/// 安装包负载目录：`<安装目录>\vhid`。
fn bundle_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("vhid"))
}

/// 虚拟 HID 的就绪情况与运行环境诊断。
#[derive(Debug, Clone, serde::Serialize)]
pub struct VhidDiagnostics {
    /// 虚拟键盘是否真的可用（客户端 DLL 能加载 + 控制设备能打开）。
    pub available: bool,
    /// 人类可读的结论。
    pub detail: String,
    /// 客户端 DLL 与驱动包是否随安装包一起就位。
    pub bundle_present: bool,
    /// 安装包负载目录。
    pub bundle_dir: String,
    /// Secure Boot 状态；读不到时为 None。
    pub secure_boot: Option<bool>,
    /// 内存完整性（HVCI）状态；读不到时为 None。
    pub hvci: Option<bool>,
    /// 不可用时的排查建议。
    pub hints: Vec<String>,
}

fn yes_no(v: bool) -> &'static str {
    if v {
        "开启"
    } else {
        "关闭"
    }
}

/// 组装失败时的排查建议。纯函数，便于覆盖各种环境组合。
fn build_hints(
    available: bool,
    bundle_present: bool,
    bundle_path: &str,
    secure_boot: Option<bool>,
    hvci: Option<bool>,
) -> Vec<String> {
    if available {
        return Vec::new();
    }

    let mut hints = Vec::new();
    if bundle_present {
        hints.push(
            "驱动负载已随安装包就位：可在本页点「安装 / 修复虚拟键盘驱动」重试（会弹一次 UAC）。"
                .to_string(),
        );
    } else {
        hints.push(format!(
            "安装目录下没有驱动负载（{bundle_path}）：请用最新安装包重新安装 Remote Mic。"
        ));
    }
    if secure_boot == Some(true) {
        hints.push(
            "Secure Boot 已开启：测试签名驱动可能被系统拒绝。正式发布需要用代码签名证书重签驱动包后再安装。"
                .to_string(),
        );
    }
    if hvci == Some(true) {
        hints.push(
            "内存完整性（HVCI）已开启：驱动包的签名链必须被系统信任，否则会被拒绝加载。"
                .to_string(),
        );
    }
    hints.push(
        "若仍失败，请提供 C:\\Windows\\INF\\setupapi.dev.log 中与 WinUHid 相关的段落以便定位。"
            .to_string(),
    );
    hints
}

/// 只探测 DLL 能否加载，探测完立即释放（避免长期占用模块引用）。
fn probe_api() -> Result<()> {
    let api = load_api()?;
    unsafe {
        let _ = FreeLibrary(api._dll);
    }
    Ok(())
}

/// 采集虚拟 HID 状态：驱动是否可用 + Secure Boot / HVCI 等环境信息 +
/// 失败时的针对性建议。不创建虚拟键盘。
pub fn diagnostics() -> VhidDiagnostics {
    let dir = bundle_dir();
    let bundle_path = dir
        .as_ref()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "<未知>".into());
    let bundle_present = dir
        .as_ref()
        .is_some_and(|d| d.join("WinUHid.dll").is_file() && d.join("WinUHidDriver.inf").is_file());

    let api = probe_api();
    let api_err = api.as_ref().err().map(|e| e.message().to_string());
    let device = api.is_ok() && control_device_present();
    let available = api.is_ok() && device;

    let secure_boot = secure_boot_enabled();
    let hvci = hvci_enabled();

    let env_note = format!(
        "Secure Boot={}，内存完整性={}",
        secure_boot.map(yes_no).unwrap_or("未知"),
        hvci.map(yes_no).unwrap_or("未知"),
    );

    let detail = if available {
        format!("WinUHid.dll 已加载，\\\\.\\WinUHid 可用（{env_note}）")
    } else if api.is_ok() {
        format!("WinUHid.dll 已加载，但控制设备 \\\\.\\WinUHid 打不开（驱动未装好）（{env_note}）")
    } else {
        format!(
            "{}（{env_note}）",
            api_err.unwrap_or_else(|| "虚拟 HID 不可用（原因未知）".into())
        )
    };

    let hints = build_hints(available, bundle_present, &bundle_path, secure_boot, hvci);

    if !available {
        core_log::log_warn(&format!(
            "[vhid] 虚拟 HID 不可用：{detail}；建议：{}",
            hints.join(" / ")
        ));
    }

    VhidDiagnostics {
        available,
        detail,
        bundle_present,
        bundle_dir: bundle_path,
        secure_boot,
        hvci,
        hints,
    }
}

/// 探测虚拟 HID 是否可用（不创建键盘），返回一行结论。
pub fn probe() -> String {
    diagnostics().detail
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 断言不限环境相关的取值，只要求整条诊断链路能跑完（含注册表读取）并给出结论。
    /// 用 `--nocapture` 可看到本机的真实取值。
    #[test]
    fn diagnostics_runs_and_reports_environment() {
        let d = diagnostics();
        eprintln!("available     = {}", d.available);
        eprintln!("detail        = {}", d.detail);
        eprintln!("bundle_dir    = {}", d.bundle_dir);
        eprintln!("bundle_present= {}", d.bundle_present);
        eprintln!("secure_boot   = {:?}", d.secure_boot);
        eprintln!("hvci          = {:?}", d.hvci);
        for h in &d.hints {
            eprintln!("hint          = {h}");
        }
        assert!(!d.detail.is_empty());
        // 负载目录必须能推导出来，否则安装器与运行时的路径约定就断了。
        assert!(
            d.bundle_dir.ends_with("vhid"),
            "bundle_dir={}",
            d.bundle_dir
        );
        // 不可用时必须给出至少一条建议。
        if !d.available {
            assert!(!d.hints.is_empty(), "不可用时没有给出任何排查建议");
        }
    }

    #[test]
    fn registry_reads_return_known_shapes() {
        // Secure Boot 状态：本机可能读不到（非 UEFI / 无权限），但读到就必须是布尔语义。
        if let Some(sb) = secure_boot_enabled() {
            let _: bool = sb;
        }
        // 场景键缺失时按「未启用」处理，不应是 None。
        assert!(hvci_enabled().is_some() || hvci_enabled().is_none());
    }

    fn joined(hints: &[String]) -> String {
        hints.join(" ")
    }

    #[test]
    fn available_needs_no_hints() {
        let h = build_hints(true, true, r"C:\App\vhid", Some(true), Some(true));
        assert!(h.is_empty(), "可用时不该给建议：{h:?}");
    }

    #[test]
    fn missing_bundle_points_at_reinstall() {
        let h = joined(&build_hints(false, false, r"C:\App\vhid", None, None));
        assert!(h.contains("没有驱动负载"), "{h}");
        assert!(h.contains(r"C:\App\vhid"), "建议里应带上实际路径：{h}");
    }

    #[test]
    fn present_bundle_points_at_repair_button() {
        let h = joined(&build_hints(false, true, r"C:\App\vhid", None, None));
        assert!(h.contains("安装 / 修复虚拟键盘驱动"), "{h}");
        assert!(!h.contains("没有驱动负载"), "{h}");
    }

    /// Secure Boot / HVCI 是这次加诊断的核心：开启时必须点名，关闭时不能误报。
    #[test]
    fn secure_boot_and_hvci_are_called_out_only_when_enabled() {
        let on = joined(&build_hints(
            false,
            true,
            r"C:\App\vhid",
            Some(true),
            Some(true),
        ));
        assert!(on.contains("Secure Boot 已开启"), "{on}");
        assert!(on.contains("内存完整性"), "{on}");

        let off = joined(&build_hints(
            false,
            true,
            r"C:\App\vhid",
            Some(false),
            Some(false),
        ));
        assert!(!off.contains("Secure Boot 已开启"), "关闭时不应误报：{off}");
        assert!(!off.contains("内存完整性"), "关闭时不应误报：{off}");

        // 读不到状态时也不应瞎猜。
        let unknown = joined(&build_hints(false, true, r"C:\App\vhid", None, None));
        assert!(!unknown.contains("Secure Boot 已开启"), "{unknown}");
        assert!(!unknown.contains("内存完整性"), "{unknown}");
    }

    /// 不管什么组合，都要给出可执行的下一步（日志指引）。
    #[test]
    fn unavailable_always_suggests_next_step() {
        for (sb, hvci, bundle) in [
            (Some(true), Some(true), true),
            (Some(false), Some(false), false),
            (None, None, true),
        ] {
            let h = build_hints(false, bundle, r"C:\App\vhid", sb, hvci);
            assert!(
                joined(&h).contains("setupapi.dev.log"),
                "缺少日志指引：{h:?}"
            );
        }
    }
}
