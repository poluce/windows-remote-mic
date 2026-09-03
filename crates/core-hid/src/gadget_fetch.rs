//! 自动检查并下载官方 Frida Gadget（返回/音量 HOGP 旁路依赖）。
//!
//! 与 `scripts/fetch-frida-gadget.ps1` 使用同一版本与 SHA-256；
//! 应用在缺少 `frida-gadget.dll` 时会自行完成下载/校验/解压。

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 与 scripts/fetch-frida-gadget.ps1 保持一致。
pub const GADGET_VERSION: &str = "17.15.3";

/// GitHub Release 上该版本 windows-x86_64 `.dll.xz` 的官方 SHA-256。
pub const GADGET_ARCHIVE_SHA256: &str =
    "b566d70189b6d551ad8f4e0bea24de08a3d4c0f559bb35b2bdb67d45182240c2";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 优先使用 System32 工具，避免 PATH 里的 Git GNU tar/curl 行为不一致。
fn system32_bin(name: &str) -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    PathBuf::from(root).join("System32").join(name)
}

fn archive_name() -> String {
    format!("frida-gadget-{GADGET_VERSION}-windows-x86_64.dll.xz")
}

fn extracted_name() -> String {
    format!("frida-gadget-{GADGET_VERSION}-windows-x86_64.dll")
}

fn archive_url() -> String {
    format!(
        "https://github.com/frida/frida/releases/download/{GADGET_VERSION}/{}",
        archive_name()
    )
}

/// 若 `dir/frida-gadget.dll` 已存在则直接返回；否则下载、校验并解压到该目录。
pub fn ensure_frida_gadget(dir: &Path) -> Result<PathBuf, String> {
    let dll_path = dir.join("frida-gadget.dll");
    if dll_path.is_file() {
        return Ok(dll_path);
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("创建 Gadget 目录失败: {e}"))?;

    let archive_path = dir.join(archive_name());
    if !archive_path.is_file() || !archive_sha_ok(&archive_path) {
        download_archive(&archive_url(), &archive_path)?;
        if !archive_sha_ok(&archive_path) {
            let _ = std::fs::remove_file(&archive_path);
            return Err("Frida Gadget 压缩包 SHA-256 校验失败，已删除损坏文件".into());
        }
    }

    let extracted_path = dir.join(extracted_name());
    if extracted_path.is_file() {
        let _ = std::fs::remove_file(&extracted_path);
    }
    extract_archive(dir, &archive_path, &extracted_path)?;

    if !extracted_path.is_file() {
        return Err(format!("解压后未找到 DLL：{}", extracted_path.display()));
    }

    std::fs::copy(&extracted_path, &dll_path)
        .map_err(|e| format!("复制 frida-gadget.dll 失败: {e}"))?;

    // 解压产物可删，保留 xz 便于离线复用/校验。
    let _ = std::fs::remove_file(&extracted_path);

    if !dll_path.is_file() {
        return Err("自动安装后仍未找到 frida-gadget.dll".into());
    }

    core_log::log_info(&format!(
        "[hid-tap] Frida Gadget 已就绪: {}",
        dll_path.display()
    ));
    Ok(dll_path)
}

fn archive_sha_ok(path: &Path) -> bool {
    match file_sha256_hex(path) {
        Ok(got) => got.eq_ignore_ascii_case(GADGET_ARCHIVE_SHA256),
        Err(e) => {
            core_log::log_warn(&format!("[hid-tap] 计算 Gadget 压缩包哈希失败: {e}"));
            false
        }
    }
}

fn download_archive(url: &str, dest: &Path) -> Result<(), String> {
    core_log::log_info(&format!("[hid-tap] 正在下载 Frida Gadget: {url}"));
    let partial = dest.with_extension("xz.partial");
    if partial.is_file() {
        let _ = std::fs::remove_file(&partial);
    }

    let curl = system32_bin("curl.exe");
    let output = Command::new(&curl)
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-fsSL",
            "--connect-timeout",
            "30",
            "--max-time",
            "300",
            "-o",
        ])
        .arg(&partial)
        .arg(url)
        .output()
        .map_err(|e| format!("启动 curl 下载失败（{}）: {e}", curl.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(&partial);
        return Err(format!(
            "下载 Frida Gadget 失败（需能访问 GitHub）: {}",
            stderr.trim()
        ));
    }

    std::fs::rename(&partial, dest).map_err(|e| format!("保存压缩包失败: {e}"))?;
    Ok(())
}

fn extract_archive(_dir: &Path, archive_path: &Path, extracted_path: &Path) -> Result<(), String> {
    // Frida 发布的是裸 `.dll.xz`（不是 tar.xz）。部分 Windows 自带 tar 无法处理，
    // 因此用纯 Rust xz 解压，不依赖 tar/Python。
    let compressed = std::fs::read(archive_path).map_err(|e| format!("读取压缩包失败: {e}"))?;
    let mut decompressed = Vec::new();
    lzma_rs::xz_decompress(&mut compressed.as_slice(), &mut decompressed)
        .map_err(|e| format!("xz 解压 Frida Gadget 失败: {e}"))?;
    std::fs::write(extracted_path, decompressed).map_err(|e| format!("写入解压 DLL 失败: {e}"))?;
    Ok(())
}

fn file_sha256_hex(path: &Path) -> Result<String, String> {
    let certutil = system32_bin("certutil.exe");
    let output = Command::new(&certutil)
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-hashfile"])
        .arg(path)
        .arg("SHA256")
        .output()
        .map_err(|e| format!("计算 SHA-256 失败: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("certutil 哈希失败: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim().replace(' ', "");
        if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(trimmed.to_lowercase());
        }
    }
    Err("未能从 certutil 输出中解析 SHA-256".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_names_match_script_convention() {
        assert_eq!(archive_name(), "frida-gadget-17.15.3-windows-x86_64.dll.xz");
        assert_eq!(extracted_name(), "frida-gadget-17.15.3-windows-x86_64.dll");
        assert!(archive_url().contains("/17.15.3/"));
        assert_eq!(GADGET_ARCHIVE_SHA256.len(), 64);
    }

    /// 手动联网验证：`cargo test -p core-hid --lib downloads_gadget_to_temp -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn downloads_gadget_to_temp() {
        let dir = std::env::temp_dir().join("remote-mic-gadget-smoke");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = ensure_frida_gadget(&dir).expect("auto fetch gadget");
        assert!(path.is_file(), "dll missing at {}", path.display());
        // 第二次应直接命中缓存，不再报错。
        let again = ensure_frida_gadget(&dir).expect("cached gadget");
        assert_eq!(path, again);
    }
}
