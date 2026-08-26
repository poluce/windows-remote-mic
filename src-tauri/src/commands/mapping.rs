use crate::{
    action_key, action_label, button_key, config_store, parse_action, parse_button, parse_trigger,
    trigger_key, MappingEdit, MappingEntry,
};

/// Save one button mapping to `config.json`.
#[tauri::command]
pub fn save_mapping(edit: MappingEdit) -> Result<(), String> {
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
    store.save(&cfg).map_err(|e| e.to_string())
}

/// Return all persisted bindings (single/double/long) for the mapping editor.
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

/// Save key calibrations map to `config.json`.
#[tauri::command]
pub fn save_key_calibrations(
    calibrations: std::collections::HashMap<String, core_config::KeyCalibration>,
) -> Result<(), String> {
    let store = config_store().ok_or("无法创建配置目录")?;
    let mut cfg = store.load().map_err(|e| e.to_string())?;
    cfg.key_calibrations = calibrations;
    store.save(&cfg).map_err(|e| e.to_string())
}

/// Get key calibrations map from `config.json`.
#[tauri::command]
pub fn get_key_calibrations() -> std::collections::HashMap<String, core_config::KeyCalibration> {
    let cfg = config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default();
    cfg.key_calibrations
}