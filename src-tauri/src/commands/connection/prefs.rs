//! 用户偏好命令：吃掉模式（hid_tap_eat）与语音识别目标（voice_target）。

use tauri::State;

use crate::{config_store, AppState};

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
    store.update(|cfg| {
        cfg.hid_tap_eat = enabled;
        Ok(())
    })?;
    core_hid::tap::write_eat_mode_file(enabled);
    core_log::log_info(&format!(
        "[commands/connection] 吃掉模式已{}（系统{}响应遥控器按键）",
        if enabled { "开启" } else { "关闭" },
        if enabled { "不" } else { "会" }
    ));
    Ok(enabled)
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
    store.update(|cfg| {
        cfg.voice_target = parsed;
        Ok(())
    })?;
    state.dispatcher.set_voice_target(parsed);
    core_log::log_info(&format!(
        "[commands/connection] 语音识别目标已设为 {:?}（'{}'）",
        parsed,
        parsed.key()
    ));
    Ok(parsed.key().to_string())
}
