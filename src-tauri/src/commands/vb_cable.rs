//! VB-CABLE 虚拟声卡：状态检测与一键安装。
//!
//! 安装逻辑只分发**助手脚本**（下载官方包、校验固定 SHA-256、拉起官方安装程序）；
//! VB-CABLE 本体不随包分发（其 EULA 禁止集成进其他产品的安装流程），
//! 详见 `scripts/install-vb-cable.ps1` 与 `scripts/prepare-installer-extras.ps1`。

use serde::Serialize;

use crate::find_install_script;

#[derive(Serialize)]
pub struct VbCableStatus {
    pub input: bool,
    pub output: bool,
    pub ready: bool,
}

#[tauri::command]
pub fn vb_cable_status() -> VbCableStatus {
    let d = core_audio::diagnostics::run();
    VbCableStatus {
        input: d.cable_input_present,
        output: d.cable_output_present,
        ready: d.has_vb_cable,
    }
}

/// 一键安装 VB-CABLE：运行官方安装辅助脚本。
#[tauri::command]
pub fn install_vb_cable() -> String {
    let d = core_audio::diagnostics::run();
    if d.has_vb_cable {
        return "已安装 VB-CABLE，无需重复安装".to_string();
    }

    #[cfg(target_os = "windows")]
    {
        let script = match find_install_script() {
            Some(s) => s,
            None => {
                return "找不到 VB-CABLE 安装助手脚本：安装包可能过旧或安装不完整。\n请安装最新版 Remote Mic 后重试，或从 VB-Audio 官网手动下载安装 VB-CABLE。".to_string();
            }
        };

        core_log::log_info(&format!(
            "[vb_cable] 启动 VB-CABLE 下载/安装助手：{}",
            script.display()
        ));
        match std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap_or_default(),
            ])
            .output()
        {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                core_log::log_info(&format!(
                    "[vb_cable] VB-CABLE 助手脚本输出：{}",
                    stdout.trim()
                ));
                "已启动 VB-CABLE 下载与官方安装程序（请留意 UAC 弹窗）。\n\
                 1) 在官方安装窗口点击安装；\n\
                 2) 安装完成后必须重启电脑；\n\
                 3) 重启后回到本页点「重新检测」。"
                    .to_string()
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                core_log::log_error(&format!(
                    "[vb_cable] VB-CABLE 助手脚本失败：{}",
                    stderr.trim()
                ));
                format!(
                    "安装失败：{}\n如仍失败，请从 VB-Audio 官网手动下载安装 VB-CABLE。",
                    stderr.trim()
                )
            }
            Err(e) => format!("无法启动安装助手：{e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        "VB-CABLE 安装仅在 Windows 上可用".to_string()
    }
}
