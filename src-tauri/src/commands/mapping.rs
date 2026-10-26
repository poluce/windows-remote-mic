use serde::Serialize;
use tauri::State;

use crate::{config_store, services, AppState, MappingEdit, MappingEntry};

/// 触发判定时间设置。
#[derive(Serialize)]
pub struct TriggerTiming {
    pub long_press_ms: u64,
    pub double_click_ms: u64,
}

/// 读取长按阈值与双击窗口（毫秒）。
#[tauri::command]
pub fn get_trigger_timing() -> TriggerTiming {
    let (long_press_ms, double_click_ms) = services::mapping::get_trigger_timing();
    TriggerTiming {
        long_press_ms,
        double_click_ms,
    }
}

/// 保存长按阈值与双击窗口，并热更新调度器。
#[tauri::command]
pub fn set_trigger_timing(
    long_press_ms: u64,
    double_click_ms: u64,
    state: State<AppState>,
) -> Result<TriggerTiming, String> {
    let (long_press_ms, double_click_ms, _revision) =
        services::mapping::set_trigger_timing(long_press_ms, double_click_ms, &state.dispatcher)?;
    core_log::log_info(&format!(
        "[commands/mapping] 触发时间已更新：长按={long_press_ms}ms，双击窗口={double_click_ms}ms"
    ));
    Ok(TriggerTiming {
        long_press_ms,
        double_click_ms,
    })
}

/// 将一条按键映射保存到 `config.json`，并热更新运行时调度器。
#[tauri::command]
pub fn save_mapping(edit: MappingEdit, state: State<AppState>) -> Result<(), String> {
    services::mapping::save_mapping(&edit, &state.dispatcher).map(|_| ())
}

/// 返回映射编辑器所需的全部持久化绑定（单击/双击/长按）。
#[tauri::command]
pub fn get_mappings() -> Vec<MappingEntry> {
    services::mapping::list_mappings()
}

/// 清除一条全局绑定（映射表右上角 ×）。
#[tauri::command]
pub fn clear_mapping_binding(
    button: String,
    trigger: String,
    state: State<AppState>,
) -> Result<(), String> {
    let change = services::mapping::clear_mapping(&button, &trigger, &state.dispatcher)?;
    if let Some(removed) = change.removed {
        core_log::log_info(&format!(
            "[commands/mapping] 已取消关联：{} · {}",
            removed.name, removed.trigger
        ));
    }
    Ok(())
}

/// 切换一条全局绑定的启用状态（映射表点击格子本体）。
#[tauri::command]
pub fn set_mapping_binding_enabled(
    button: String,
    trigger: String,
    enabled: bool,
    state: State<AppState>,
) -> Result<(), String> {
    let (entry, _revision, changed) =
        services::mapping::set_mapping_enabled(&button, &trigger, enabled, &state.dispatcher)?;
    if changed {
        core_log::log_info(&format!(
            "[commands/mapping] 绑定已{}：{} · {}",
            if enabled { "启用" } else { "禁用" },
            entry.name,
            entry.trigger
        ));
    }
    Ok(())
}

/// 将按键校准表保存到 `config.json`，并热更新调度器的虚拟键反查表。
#[tauri::command]
pub fn save_key_calibrations(
    calibrations: std::collections::HashMap<String, core_config::KeyCalibration>,
    state: State<AppState>,
) -> Result<(), String> {
    services::mapping::save_calibrations(calibrations, &state.dispatcher).map(|_| ())
}

/// 暂停 / 恢复按键调度。按键测试与校准界面打开时应暂停，
/// 避免测试按键触发真实动作。
#[tauri::command]
pub fn set_dispatch_enabled(enabled: bool, state: State<AppState>) {
    if state.dispatcher.set_enabled(enabled) {
        core_log::log_info(&format!(
            "[dispatch] 调度器已{}",
            if enabled {
                "启用"
            } else {
                "暂停（按键测试）"
            }
        ));
    }
}

/// 从 `config.json` 读取按键校准表。
#[tauri::command]
pub fn get_key_calibrations() -> std::collections::HashMap<String, core_config::KeyCalibration> {
    config_store()
        .and_then(|s| s.load().ok())
        .unwrap_or_default()
        .key_calibrations
}

// ---------------------------------------------------------------------------
// 用户命名的自定义快捷键
//
// 这三个命令**不碰调度器**：名字只是显示层，映射绑定里存的仍是
// `combo:lctrl+k`，改名或删除都不会影响任何已经保存的绑定。
// ---------------------------------------------------------------------------

/// 读自定义快捷键库。
#[tauri::command]
pub fn get_shortcuts() -> Vec<core_config::NamedShortcut> {
    services::mapping::list_shortcuts()
}

/// 新增或改名，返回改完之后的整个库。
#[tauri::command]
pub fn save_shortcut(
    name: String,
    keys: Vec<String>,
) -> Result<Vec<core_config::NamedShortcut>, String> {
    let (shortcuts, _revision) = services::mapping::save_shortcut(name, keys)?;
    core_log::log_info(&format!(
        "[commands/mapping] 自定义快捷键库已更新：{} 条",
        shortcuts.len()
    ));
    Ok(shortcuts)
}

/// 删掉一条。删不存在的 keys 也算成功——调用方要的结果是「它没了」。
#[tauri::command]
pub fn delete_shortcut(keys: Vec<String>) -> Result<Vec<core_config::NamedShortcut>, String> {
    let (shortcuts, _revision) = services::mapping::delete_shortcut(keys)?;
    core_log::log_info(&format!(
        "[commands/mapping] 自定义快捷键库已更新：{} 条",
        shortcuts.len()
    ));
    Ok(shortcuts)
}
