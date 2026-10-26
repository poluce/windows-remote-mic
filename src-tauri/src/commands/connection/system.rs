//! 系统集成命令：打开 Windows 设置、重启应用、重装虚拟 HID 驱动。

#[tauri::command]
pub fn open_system_settings(setting: String) -> String {
    let uri = match setting.as_str() {
        "bluetooth" => "ms-settings:bluetooth",
        "microphone" => "ms-settings:privacy-microphone",
        "sound" => "ms-settings:sound",
        _ => "ms-settings:",
    };
    #[cfg(target_os = "windows")]
    {
        match std::process::Command::new("cmd")
            .args(["/C", "start", "", uri])
            .spawn()
        {
            Ok(_) => "已打开系统设置".to_string(),
            Err(e) => format!("打开失败：{e}"),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = uri;
        "仅限 Windows".to_string()
    }
}

/// 重启应用：退出当前进程并由 Tauri 内部 helper 重新拉起自身。
///
/// `AppHandle::restart` 不返回（`-> !`），因此本命令也不会有返回值；
/// 命令体内部直接触发重启，旧进程随后退出。
#[tauri::command]
pub fn restart_app(app: tauri::AppHandle) {
    core_log::log_info("[commands/connection] 收到重启请求，关闭 HOGP 吃掉模式并重启应用…");
    // 旧进程退出后注入在 WUDFHost 的脚本仍在，新实例启动时才会把开关
    // 写回配置值。这里先落盘 0，避免重启窗口期内遥控器按键被吃掉却无人处理。
    core_hid::tap::write_eat_mode_file(false);
    app.restart();
}

/// 安装 / 修复虚拟 HID 键盘驱动。
///
/// 复用安装包随附的 `vhid\install-winuhid.ps1`（安装器已把驱动包放在该目录）。
/// 脚本自身在非管理员时会以 UAC 提权并等待结束，因此这里会弹出一次 UAC。
/// 返回安装后的探测结果，便于前端直接判断是否可用。
#[tauri::command]
pub fn install_vhid_driver() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = exe
            .parent()
            .ok_or_else(|| "无法定位应用安装目录".to_string())?
            .join("vhid");
        let script = dir.join("install-winuhid.ps1");
        if !script.is_file() {
            return Err(format!(
                "找不到随应用安装的驱动脚本：{}。请用最新安装包重新安装 Remote Mic。",
                script.display()
            ));
        }

        core_log::log_info("[commands/connection] 开始安装/修复虚拟 HID 驱动");
        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                script.to_str().unwrap_or_default(),
                "-DriverDir",
                dir.to_str().unwrap_or_default(),
            ])
            .output()
            .map_err(|e| format!("无法启动驱动安装脚本：{e}"))?;

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() && !stderr.is_empty() {
            core_log::log_error(&format!("[commands/connection] 驱动安装脚本失败：{stderr}"));
            return Err(stderr);
        }

        // 用同一套诊断返回结论，失败时把环境信息与建议一并带给前端。
        let d = core_input::vhid_diagnostics();
        let mut msg = d.detail.clone();
        if !d.hints.is_empty() {
            msg.push('\n');
            msg.push_str(&d.hints.join("\n"));
        }
        core_log::log_info(&format!(
            "[commands/connection] 驱动安装结束：available={} {msg}",
            d.available
        ));
        Ok(msg)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("虚拟 HID 驱动仅在 Windows 上可用".to_string())
    }
}
