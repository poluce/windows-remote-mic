use tauri::State;

use crate::{
    action_key, action_label, button_key, config_store, parse_action, parse_button, parse_trigger,
    trigger_key, AppState, MappingEdit, MappingEntry,
};

/// 将一条按键映射保存到 `config.json`，并热更新运行时调度器。
#[tauri::command]
pub fn save_mapping(edit: MappingEdit, state: State<AppState>) -> Result<(), String> {
    let button = parse_button(&edit.button).ok_or("未知按键")?;
    let trigger = parse_trigger(&edit.trigger).ok_or("未知触发")?;
    let action = parse_action(&edit.action).ok_or("未知动作")?;

    let store = config_store().ok_or("无法创建配置目录")?;
    let mut cfg = store.load().map_err(|e| e.to_string())?;
    if let Some(binding) = cfg
        .mapping
        .bindings
        .iter_mut()
        .find(|b| b.button == button && b.trigger == trigger)
    {
        binding.action = action;
    } else {
        cfg.mapping.bindings.push(core_mapping::KeyBinding {
            button,
            trigger,
            action,
        });
    }
    store.save(&cfg).map_err(|e| e.to_string())?;
    state.dispatcher.update_mapping(cfg.mapping);
    Ok(())
}

/// 返回映射编辑器所需的全部持久化绑定（单击/双击/长按）。
#[tauri::command]
pub fn get_mappings() -> Vec<MappingEntry> {
    let cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    cfg.mapping
        .bindings
        .iter()
        .map(|b| MappingEntry {
            button: button_key(&b.button),
            name: b.button.display_name().to_string(),
            trigger: trigger_key(&b.trigger),
            action: action_label(&b.action),
            action_key: action_key(&b.action),
        })
        .collect()
}

/// 将按键校准表保存到 `config.json`，并热更新调度器的虚拟键反查表。
#[tauri::command]
pub fn save_key_calibrations(
    calibrations: std::collections::HashMap<String, core_config::KeyCalibration>,
    state: State<AppState>,
) -> Result<(), String> {
    let store = config_store().ok_or("无法创建配置目录")?;
    let mut cfg = store.load().map_err(|e| e.to_string())?;
    cfg.key_calibrations = calibrations;
    store.save(&cfg).map_err(|e| e.to_string())?;
    state.dispatcher.update_calibrations(&cfg.key_calibrations);
    Ok(())
}

/// 暂停 / 恢复按键调度。按键测试与校准界面打开时应暂停，
/// 避免测试按键触发真实动作。
#[tauri::command]
pub fn set_dispatch_enabled(enabled: bool, state: State<AppState>) {
    state.dispatcher.set_enabled(enabled);
    core_log::log_info(&format!(
        "[dispatch] 调度器已{}",
        if enabled {
            "启用"
        } else {
            "暂停（按键测试）"
        }
    ));
}

/// 从 `config.json` 读取按键校准表。
#[tauri::command]
pub fn get_key_calibrations() -> std::collections::HashMap<String, core_config::KeyCalibration> {
    let cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    cfg.key_calibrations
}
