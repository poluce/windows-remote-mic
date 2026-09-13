use serde::Serialize;
use tauri::State;

use crate::{config_store, AppState};

/// 连接页需要恢复的运行时状态快照。
#[derive(Serialize)]
pub struct RuntimeStatus {
    pub connected: bool,
    pub bridge_running: bool,
    pub tap_status: Option<core_hid::tap::TapStatusEvent>,
    pub endpoints_ready: bool,
}

#[tauri::command]
pub fn get_runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        connected: core_voice::connection_active(),
        bridge_running: core_voice::bridge_running(),
        tap_status: core_hid::tap::last_status(),
        endpoints_ready: core_voice::atvv_endpoints_ready(),
    }
}

/// 连接 RC003。
#[derive(Serialize)]
pub struct Rc003Connection {
    pub device: core_ble::BleDevice,
    pub endpoints: core_ble::gatt::AtvvEndpoints,
}

#[tauri::command]
pub async fn scan_for_rc003() -> Result<core_ble::BleDevice, String> {
    core_log::log_info("[commands/connection] 前端请求扫描 RC003");
    tauri::async_runtime::spawn_blocking(|| match core_ble::scan_for_rc003() {
        Ok(device) => {
            core_log::log_info(&format!(
                "[commands/connection] 扫描 RC003 成功：名称='{}'，ID='{}'",
                device.name, device.id
            ));
            Ok(device)
        }
        Err(e) => {
            core_log::log_error(&format!("[commands/connection] 扫描 RC003 失败: {e}"));
            Err(e.to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn connect_rc003() -> Result<Rc003Connection, String> {
    core_log::log_info("[commands/connection] 前端请求连接 RC003");
    tauri::async_runtime::spawn_blocking(|| {
        match core_ble::scan_and_connect() {
            Ok((device, endpoints)) => {
                core_log::log_info(&format!(
                    "[commands/connection] 连接 RC003 成功：'{}' ({}) -> ATVV: tx={:?}, audio={:?}, control={:?}",
                    device.name, device.id, endpoints.tx, endpoints.audio, endpoints.control
                ));
                Ok(Rc003Connection { device, endpoints })
            }
            Err(e) => {
                core_log::log_error(&format!("[commands/connection] 连接 RC003 失败: {e}"));
                Err(e.to_string())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_hid_tap_eat() -> bool {
    config_store()
        .map(|s| s.load_or_default().hid_tap_eat)
        .unwrap_or(true)
}

/// 切换「吃掉」模式：持久化到配置并写入热更新文件，Frida 脚本
/// 秒级轮询生效，无需重新注入 WUDFHost / 重新连接。
#[tauri::command]
pub fn set_hid_tap_eat(enabled: bool) -> Result<bool, String> {
    let store = config_store().ok_or_else(|| "无法创建配置目录".to_string())?;
    let mut cfg = store.load().unwrap_or_default();
    cfg.hid_tap_eat = enabled;
    store.save(&cfg).map_err(|e| e.to_string())?;
    core_hid::tap::write_eat_mode_file(enabled);
    core_log::log_info(&format!(
        "[commands/connection] 吃掉模式已{}（系统{}响应遥控器按键）",
        if enabled { "开启" } else { "关闭" },
        if enabled { "不" } else { "会" }
    ));
    Ok(enabled)
}

#[tauri::command]
pub fn save_selected_device(device_id: String) -> Result<(), String> {
    let mut cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    cfg.selected_device_id = Some(device_id);
    config_store()
        .ok_or_else(|| "无法创建配置目录".to_string())?
        .save(&cfg)
        .map_err(|e| e.to_string())
}

/// 读取当前语音识别目标（连接页「识别方案」）。
/// 返回 snake_case 字符串（如 `windows_voice` / `ime_wechat`）。
#[tauri::command]
pub fn get_voice_target() -> String {
    config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default()
        .voice_target
        .key()
        .to_string()
}

/// 设置语音识别目标：持久化到 `config.json` 并热更新调度器。
///
/// 第三方输入法（微信/豆包/搜狗）目前仅预留：连接页可选并保存，
/// 但麦克风键触发时会返回「尚未接入（需真机验证）」——见 core-input。
/// 未知字符串回落 Windows 语音，保证不产生无效配置。
#[tauri::command]
pub fn set_voice_target(target: String, state: State<AppState>) -> Result<String, String> {
    let parsed = core_mapping::VoiceTarget::parse(&target)
        .unwrap_or(core_mapping::VoiceTarget::WindowsVoice);
    let store = config_store().ok_or_else(|| "无法创建配置目录".to_string())?;
    let mut cfg = store.load().unwrap_or_default();
    cfg.voice_target = parsed;
    store.save(&cfg).map_err(|e| e.to_string())?;
    state.dispatcher.set_voice_target(parsed);
    core_log::log_info(&format!(
        "[commands/connection] 语音识别目标已设为 {:?}（'{}'）",
        parsed,
        parsed.key()
    ));
    Ok(parsed.key().to_string())
}

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
    core_log::log_info("[commands/connection] 收到重启请求，正在重启应用…");
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

        let probe = core_input::vhid_probe();
        core_log::log_info(&format!("[commands/connection] 驱动安装结束：{probe}"));
        Ok(probe)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("虚拟 HID 驱动仅在 Windows 上可用".to_string())
    }
}
