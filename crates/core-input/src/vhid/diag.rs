//! 虚拟 HID 运行环境与驱动可用性诊断。
//!
//! 检查项包括：
//! - WinUHid.dll 客户端加载状态；
//! - \\\\.\\WinUHid 控制设备打开状态；
//! - 安装包负载目录与文件是否存在；
//! - 是否为本地开发构建（target/debug 或 target/release）；
//! - Windows Secure Boot 与内存完整性（HVCI）状态（用于排查驱动加载被拒）；
//! - 针对性排查提示。

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, FreeLibrary, GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};

use crate::error::Result;

pub(super) fn control_device_present() -> bool {
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
pub(super) fn read_hklm_dword(subkey: &str, value: &str) -> Option<u32> {
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
pub(super) fn secure_boot_enabled() -> Option<bool> {
    read_hklm_dword(
        "SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State",
        "UEFISecureBootEnabled",
    )
    .map(|v| v != 0)
}

/// 内存完整性（HVCI）是否开启。该场景键不存在时视为未配置（`Some(false)`）。
pub(super) fn hvci_enabled() -> Option<bool> {
    match read_hklm_dword(
        "SYSTEM\\CurrentControlSet\\Control\\DeviceGuard\\Scenarios\\HypervisorEnforcedCodeIntegrity",
        "Enabled",
    ) {
        Some(v) => Some(v != 0),
        // 没有状态键也能读通注册表时，说明该场景未启用。
        None => Some(false),
    }
}

/// 可执行文件所在目录（安装目录或 `target\...`）。
fn exe_dir() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
}

/// 安装包负载目录：`<安装目录>\vhid`。
fn bundle_dir() -> Option<PathBuf> {
    Some(exe_dir()?.join("vhid"))
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
    /// 是否是 cargo / tauri 开发构建（没有随包负载，也不会触发安装器的自动安装）。
    pub dev_build: bool,
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

/// 是否是 cargo / tauri 的开发构建（`<仓库>\target\{debug,release}\`）。
///
/// 开发构建旁边没有安装包释放的负载，也不会触发 NSIS 安装钩子，
/// 因此不能用「请重新安装 Remote Mic」这类面向安装包的提示。
pub(super) fn is_dev_build(exe_dir: &Path) -> bool {
    let leaf = exe_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if leaf != "debug" && leaf != "release" {
        return false;
    }
    exe_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        == Some("target")
}

/// 组装失败时的排查建议。纯函数，便于覆盖各种环境组合。
pub(super) fn build_hints(
    available: bool,
    bundle_present: bool,
    dev_build: bool,
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
    } else if dev_build {
        hints.push(
            "这是开发构建（target\\debug 或 target\\release），既没有随包负载、也不会触发安装器的自动安装。\
             请先运行 scripts/prepare-vhid-bundle.ps1 生成负载，或直接安装正式安装包。"
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
    let api = super::load_api()?;
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

    let dev_build = exe_dir().as_ref().is_some_and(|d| is_dev_build(d));
    let hints = build_hints(
        available,
        bundle_present,
        dev_build,
        &bundle_path,
        secure_boot,
        hvci,
    );

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
        dev_build,
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
