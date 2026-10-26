//! 映射 / 触发时间 / 快捷键的配置读写。
//!
//! 读操作直接读 `config.json`；写操作统一走 `ConfigStore::update`，返回新的
//! revision，供 MCP 做乐观并发、UI 做撤销校验。

use core_config::NamedShortcut;
use core_dispatch::KeyDispatcher;
use core_mapping::{ActionKind, ButtonId, KeyBinding, Trigger};

use crate::{
    action_key, action_label, button_key, config_store, parse_action, parse_button, parse_trigger,
    trigger_key, MappingEdit, MappingEntry,
};

/// 一条映射写入前后的变化（MCP 用来返回旧值/新值，UI 可忽略）。
#[derive(Debug, Clone)]
pub struct MappingChange {
    pub old: Option<MappingEntry>,
    pub new: MappingEntry,
    pub revision: u64,
}

/// 一条映射删除结果（`removed=None` 表示本来就没有）。
#[derive(Debug, Clone)]
pub struct MappingClear {
    pub removed: Option<MappingEntry>,
    pub revision: u64,
}

fn store() -> Result<core_config::ConfigStore, String> {
    config_store().ok_or_else(|| "无法创建配置目录".to_string())
}

fn entry_of(binding: &KeyBinding) -> MappingEntry {
    MappingEntry {
        button: button_key(&binding.button),
        name: binding.button.display_name().to_string(),
        trigger: trigger_key(&binding.trigger),
        action: action_label(&binding.action),
        action_key: action_key(&binding.action),
        enabled: binding.enabled,
    }
}

fn parse_key(button: &str, trigger: &str) -> Result<(ButtonId, Trigger), String> {
    let button = parse_button(button).ok_or("未知按键")?;
    let trigger = parse_trigger(trigger).ok_or("未知触发")?;
    Ok((button, trigger))
}

/// 全部持久化绑定（含 enabled）。
pub fn list_mappings() -> Vec<MappingEntry> {
    store()
        .and_then(|s| s.load().map_err(|e| e.to_string()))
        .map(|cfg| cfg.mapping.bindings.iter().map(entry_of).collect())
        .unwrap_or_default()
}

/// 当前配置 revision（旧配置读成 0）。
pub fn revision() -> u64 {
    store()
        .and_then(|s| s.load().map_err(|e| e.to_string()))
        .map(|cfg| cfg.revision)
        .unwrap_or(0)
}

/// 保存一条映射：存在则改动作并启用，不存在则新增；保存即启用。
pub fn save_mapping(
    edit: &MappingEdit,
    dispatcher: &KeyDispatcher,
) -> Result<MappingChange, String> {
    let (button, trigger) = parse_key(&edit.button, &edit.trigger)?;
    let action = parse_action(&edit.action).ok_or("未知动作")?;
    let enabled = edit.enabled.unwrap_or(true);
    let store = store()?;

    let ((mapping, old, new), revision) = store.update(|cfg| {
        let old = cfg.mapping.binding(button, trigger).map(entry_of);
        if let Some(binding) = cfg
            .mapping
            .bindings
            .iter_mut()
            .find(|b| b.button == button && b.trigger == trigger)
        {
            binding.action = action;
            binding.enabled = enabled;
        } else {
            cfg.mapping.bindings.push(KeyBinding {
                button,
                trigger,
                action,
                enabled,
            });
        }
        let new = entry_of(
            cfg.mapping
                .binding(button, trigger)
                .expect("刚写入的绑定必须存在"),
        );
        Ok((cfg.mapping.clone(), old, new))
    })?;

    dispatcher.update_mapping(mapping);
    Ok(MappingChange { old, new, revision })
}

/// 取消关联（删除一条绑定）。
pub fn clear_mapping(
    button: &str,
    trigger: &str,
    dispatcher: &KeyDispatcher,
) -> Result<MappingClear, String> {
    let (button, trigger) = parse_key(button, trigger)?;
    let store = store()?;

    let (change, revision) = store.update_opt(|cfg| {
        let removed = cfg.mapping.binding(button, trigger).map(entry_of);
        if removed.is_none() {
            // 旧命令的行为：本来就没有这条绑定就不写盘、不重载调度器。
            return Ok(None);
        }
        cfg.mapping
            .bindings
            .retain(|b| !(b.button == button && b.trigger == trigger));
        Ok(Some((cfg.mapping.clone(), removed)))
    })?;

    match change {
        Some((mapping, removed)) => {
            dispatcher.update_mapping(mapping);
            Ok(MappingClear { removed, revision })
        }
        None => Ok(MappingClear {
            removed: None,
            revision,
        }),
    }
}

/// 切换启用状态；绑定不存在时报错。
pub fn set_mapping_enabled(
    button: &str,
    trigger: &str,
    enabled: bool,
    dispatcher: &KeyDispatcher,
) -> Result<(MappingEntry, u64, bool), String> {
    let (button, trigger) = parse_key(button, trigger)?;
    let store = store()?;

    let (change, revision) = store.update_opt(|cfg| {
        let Some(binding) = cfg
            .mapping
            .bindings
            .iter_mut()
            .find(|b| b.button == button && b.trigger == trigger)
        else {
            return Err("这条绑定不存在".into());
        };
        if binding.enabled == enabled {
            // 旧命令的行为：已经是目标状态就不写盘、不重载调度器。
            return Ok(None);
        }
        binding.enabled = enabled;
        let new = entry_of(binding);
        Ok(Some((cfg.mapping.clone(), new)))
    })?;

    match change {
        Some((mapping, new)) => {
            dispatcher.update_mapping(mapping);
            Ok((new, revision, true))
        }
        None => {
            let entry = list_mappings()
                .into_iter()
                .find(|e| e.button == button_key(&button) && e.trigger == trigger_key(&trigger))
                .ok_or("这条绑定不存在")?;
            Ok((entry, revision, false))
        }
    }
}

/// 读长按阈值 / 双击窗口。
pub fn get_trigger_timing() -> (u64, u64) {
    let cfg = store()
        .and_then(|s| s.load().map_err(|e| e.to_string()))
        .unwrap_or_default();
    (cfg.long_press_ms, cfg.double_click_ms)
}

/// 写长按阈值 / 双击窗口（clamp 到安全范围），热更新调度器。
pub fn set_trigger_timing(
    long_press_ms: u64,
    double_click_ms: u64,
    dispatcher: &KeyDispatcher,
) -> Result<(u64, u64, u64), String> {
    let long_press_ms = long_press_ms.clamp(200, 2000);
    let double_click_ms = double_click_ms.clamp(150, 1000);
    let store = store()?;

    let revision = store
        .update(|cfg| {
            cfg.long_press_ms = long_press_ms;
            cfg.double_click_ms = double_click_ms;
            Ok(())
        })?
        .1;

    dispatcher.set_trigger_timing(long_press_ms, double_click_ms);
    Ok((long_press_ms, double_click_ms, revision))
}

/// 保存按键校准表并热更新调度器。
pub fn save_calibrations(
    calibrations: std::collections::HashMap<String, core_config::KeyCalibration>,
    dispatcher: &KeyDispatcher,
) -> Result<u64, String> {
    let store = store()?;
    let (calibrations, revision) = store.update(|cfg| {
        cfg.key_calibrations = calibrations;
        Ok(cfg.key_calibrations.clone())
    })?;
    dispatcher.update_calibrations(&calibrations);
    Ok(revision)
}

/// 快捷键库（只读）。
pub fn list_shortcuts() -> Vec<NamedShortcut> {
    store()
        .and_then(|s| s.load().map_err(|e| e.to_string()))
        .map(|cfg| cfg.shortcuts)
        .unwrap_or_default()
}

/// 新增 / 改名一条快捷键；返回改完后的整个库 + revision。
pub fn save_shortcut(name: String, keys: Vec<String>) -> Result<(Vec<NamedShortcut>, u64), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("请给这个快捷键起个名字".into());
    }
    let keys = normalize_keys(&keys);
    if keys.is_empty() {
        return Err("请先录制或输入快捷键".into());
    }
    let store = store()?;
    store.update(|cfg| {
        match cfg.shortcuts.iter_mut().find(|s| s.keys == keys) {
            Some(existing) => existing.name = name,
            None => cfg.shortcuts.push(NamedShortcut { name, keys }),
        }
        Ok(cfg.shortcuts.clone())
    })
}

/// 删除一条快捷键；不存在也返回成功（幂等）。
pub fn delete_shortcut(keys: Vec<String>) -> Result<(Vec<NamedShortcut>, u64), String> {
    let keys = normalize_keys(&keys);
    let store = store()?;
    let (change, revision) = store.update_opt(|cfg| {
        let before = cfg.shortcuts.len();
        cfg.shortcuts.retain(|s| s.keys != keys);
        if cfg.shortcuts.len() == before {
            return Ok(None);
        }
        Ok(Some(cfg.shortcuts.clone()))
    })?;
    match change {
        Some(shortcuts) => Ok((shortcuts, revision)),
        None => Ok((list_shortcuts(), revision)),
    }
}

fn normalize_keys(keys: &[String]) -> Vec<String> {
    keys.iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect()
}

/// 供 MCP 校验动作是否可用（目录里已移除 `disabled`，这里也拒绝）。
pub fn action_is_supported(action: &str) -> bool {
    matches!(parse_action(action), Some(a) if a != ActionKind::Disabled)
}
