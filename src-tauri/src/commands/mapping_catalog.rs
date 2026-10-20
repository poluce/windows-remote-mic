//! 按键映射编辑器所需的目录数据（唯一事实来源）。
//!
//! 这些内容以前在前端 `src/pages/mapping/constants.ts` 里也复制了一份：
//! 按键清单、触发方式、动作目录、默认映射。后端新增/调整动作或按键时，
//! 必须记得同步前端，否则会出现「后端能用的动作在界面上选不到」。
//!
//! 现在统一由本模块的 [`get_mapping_catalog`] 命令提供给前端；前端只为
//! **浏览器预览**（没有 Tauri 后端时）保留本地兜底常量。

use serde::Serialize;

use core_mapping::{default_mapping, ActionKind, ButtonId, Trigger};

use crate::{action_key, action_label, button_key, trigger_key};

/// 一个物理按键及其允许的触发方式。
#[derive(Debug, Clone, Serialize)]
pub struct CatalogButton {
    /// 稳定小写标识（如 `power` / `mic`）。
    pub key: String,
    /// 中文显示名。
    pub name: String,
    /// 该按键允许的触发方式 key 列表（麦克风 PTT 只有按下/松开）。
    pub triggers: Vec<String>,
}

/// 一种触发方式。
#[derive(Debug, Clone, Serialize)]
pub struct CatalogTrigger {
    pub key: String,
    pub label: String,
    pub desc: String,
}

/// 动作选择器中的一项。
#[derive(Debug, Clone, Serialize)]
pub struct CatalogAction {
    pub key: String,
    /// 选择器里的短标签（与保存后列表里的 `action_label` 不同：
    /// 选择器要短，列表要能自解释）。
    pub label: String,
}

/// 动作分类。
#[derive(Debug, Clone, Serialize)]
pub struct CatalogCategory {
    pub key: String,
    pub title: String,
    pub actions: Vec<CatalogAction>,
}

/// `config.json` 里没有任何绑定时使用的默认绑定（与 `core_mapping::default_mapping` 一致）。
#[derive(Debug, Clone, Serialize)]
pub struct CatalogBinding {
    pub button: String,
    pub name: String,
    pub trigger: String,
    /// 保存后列表里显示的动作名。
    pub action: String,
    pub action_key: String,
}

/// 映射编辑器完整目录。
#[derive(Debug, Clone, Serialize)]
pub struct MappingCatalog {
    pub buttons: Vec<CatalogButton>,
    pub triggers: Vec<CatalogTrigger>,
    pub categories: Vec<CatalogCategory>,
    pub default_bindings: Vec<CatalogBinding>,
}

/// 动作选择器的分类与顺序（唯一事实来源，前端只负责渲染）。
///
/// 顺序有讲究：点一个分类会把动作默认选中该分类的第一项，所以顺序变动
/// 会影响「点分类后默认选中什么」，不要随意调整。
fn categories() -> Vec<CatalogCategory> {
    use ActionKind as A;
    let system = [
        (A::Return, "回车"),
        (A::DeleteBackward, "退格"),
        (A::Escape, "Esc"),
        (A::ShowDesktop, "显示桌面"),
        (A::FocusInput, "聚焦输入框"),
        (A::FocusInputOrSubmit, "聚焦输入框 / 回车"),
        (A::ContextMenu, "右键菜单"),
        (A::AppSwitcher, "切换应用"),
        (A::ToggleQuickMenu, "快捷菜单（开/关）"),
        (A::ArrowUp, "上"),
        (A::ArrowDown, "下"),
        (A::ArrowLeft, "左"),
        (A::ArrowRight, "右"),
        (A::SystemVolumeUp, "音量 +"),
        (A::SystemVolumeDown, "音量 −"),
        (A::SystemVolumeMute, "静音"),
        (A::PlayPause, "播放 / 暂停"),
        (A::Voice, "语音输入（Win+H）"),
    ]
    .into_iter()
    .map(|(action, label)| CatalogAction {
        key: action_key(&action),
        label: label.to_string(),
    })
    .collect();

    vec![
        CatalogCategory {
            key: "system".into(),
            title: "系统".into(),
            actions: system,
        },
        // 这一栏的按钮不来自后端动作，而是用户在界面上建的快捷键库（见
        // `mapping/shortcuts.ts`）。`custom_combo` 是个哨兵值：真正保存的
        // action_key 由前端用组合键现算。
        CatalogCategory {
            key: "combo".into(),
            title: "自定义".into(),
            actions: vec![CatalogAction {
                key: "custom_combo".into(),
                label: "自定义快捷键".into(),
            }],
        },
        CatalogCategory {
            key: "other".into(),
            title: "其他".into(),
            actions: vec![CatalogAction {
                key: action_key(&ActionKind::Disabled),
                label: "禁用".into(),
            }],
        },
    ]
}

fn button_triggers(button: ButtonId) -> Vec<String> {
    let triggers: &[Trigger] = match button {
        // 麦克风是 PTT 键：只有按下/松开。
        ButtonId::Mic => &[Trigger::Press, Trigger::Release],
        _ => &[
            Trigger::SingleClick,
            Trigger::DoubleClick,
            Trigger::LongPress,
        ],
    };
    triggers.iter().map(trigger_key).collect()
}

/// 构建完整目录（供命令与测试复用）。
pub fn catalog() -> MappingCatalog {
    // 顺序与前端映射矩阵的行顺序一致（电源、麦克风在最前，方向键按十字排布）。
    const BUTTON_ORDER: [ButtonId; 13] = [
        ButtonId::Power,
        ButtonId::Mic,
        ButtonId::Up,
        ButtonId::Left,
        ButtonId::Ok,
        ButtonId::Right,
        ButtonId::Down,
        ButtonId::Back,
        ButtonId::VolumeUp,
        ButtonId::Home,
        ButtonId::VolumeDown,
        ButtonId::Menu,
        ButtonId::Tv,
    ];

    let triggers = [
        (Trigger::SingleClick, "单击", "按一下立即执行"),
        (Trigger::DoubleClick, "双击", "0.3 秒内按两次"),
        (Trigger::LongPress, "长按", "按住约 0.55 秒"),
        (Trigger::Press, "按下", "识别为长按后触发（PTT）"),
        (Trigger::Release, "松开", "长按结束后触发（PTT）"),
    ]
    .into_iter()
    .map(|(trigger, label, desc)| CatalogTrigger {
        key: trigger_key(&trigger),
        label: label.to_string(),
        desc: desc.to_string(),
    })
    .collect();

    let buttons = BUTTON_ORDER
        .into_iter()
        .map(|button| CatalogButton {
            key: button_key(&button),
            name: button.display_name().to_string(),
            triggers: button_triggers(button),
        })
        .collect();

    let default_bindings = default_mapping()
        .iter()
        .map(|binding| CatalogBinding {
            button: button_key(&binding.button),
            name: binding.button.display_name().to_string(),
            trigger: trigger_key(&binding.trigger),
            action: action_label(&binding.action),
            action_key: action_key(&binding.action),
        })
        .collect();

    MappingCatalog {
        buttons,
        triggers,
        categories: categories(),
        default_bindings,
    }
}

/// 返回映射编辑器所需的全部目录数据（按键 / 触发方式 / 动作分类 / 默认绑定）。
#[tauri::command]
pub fn get_mapping_catalog() -> MappingCatalog {
    let catalog = catalog();
    core_log::log_info(&format!(
        "[commands/mapping] 下发映射目录：{} 按键 / {} 触发方式 / {} 动作分类 / {} 默认绑定",
        catalog.buttons.len(),
        catalog.triggers.len(),
        catalog.categories.len(),
        catalog.default_bindings.len()
    ));
    catalog
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_action;
    use std::collections::HashSet;

    #[test]
    fn buttons_cover_all_physical_keys_in_matrix_order() {
        let catalog = catalog();
        assert_eq!(catalog.buttons.len(), 13);
        let keys: HashSet<&str> = catalog.buttons.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys.len(), 13, "按键 key 必须唯一");
        // 矩阵行顺序：电源、麦克风在最前。
        assert_eq!(catalog.buttons[0].key, "power");
        assert_eq!(catalog.buttons[1].key, "mic");
        for button in &catalog.buttons {
            assert!(!button.name.is_empty());
            assert!(!button.triggers.is_empty());
        }
    }

    #[test]
    fn mic_only_accepts_press_and_release() {
        let catalog = catalog();
        let mic = catalog
            .buttons
            .iter()
            .find(|b| b.key == "mic")
            .expect("目录必须包含麦克风键");
        assert_eq!(
            mic.triggers,
            vec!["press".to_string(), "release".to_string()]
        );
        let ok = catalog.buttons.iter().find(|b| b.key == "ok").unwrap();
        assert_eq!(
            ok.triggers,
            vec![
                "single_click".to_string(),
                "double_click".to_string(),
                "long_press".to_string()
            ]
        );
    }

    #[test]
    fn triggers_are_unique_and_complete() {
        let catalog = catalog();
        let keys: Vec<&str> = catalog.triggers.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "single_click",
                "double_click",
                "long_press",
                "press",
                "release"
            ]
        );
        for trigger in &catalog.triggers {
            assert!(!trigger.label.is_empty());
            assert!(!trigger.desc.is_empty());
        }
    }

    #[test]
    fn every_catalog_action_roundtrips_to_action_key() {
        let catalog = catalog();
        for category in &catalog.categories {
            assert!(
                !category.actions.is_empty(),
                "分类 {} 不能为空",
                category.key
            );
            for item in &category.actions {
                // custom_combo 是前端哨兵，不是后端动作，跳过。
                if item.key == "custom_combo" {
                    continue;
                }
                let parsed = parse_action(&item.key)
                    .unwrap_or_else(|| panic!("动作 {} 无法被后端解析", item.key));
                assert_eq!(
                    action_key(&parsed),
                    item.key,
                    "动作 {} 的 key/label 往返不一致",
                    item.key
                );
                assert!(!item.label.is_empty());
            }
        }
        // 系统分类包含全部 18 个可绑定动作（新增动作时必须加进来）。
        let system = catalog
            .categories
            .iter()
            .find(|c| c.key == "system")
            .unwrap();
        assert_eq!(system.actions.len(), 18);
    }

    #[test]
    fn default_bindings_match_core_mapping_defaults() {
        let catalog = catalog();
        let defaults = default_mapping();
        assert_eq!(catalog.default_bindings.len(), defaults.len());
        assert_eq!(
            catalog.default_bindings.len(),
            14,
            "13 个单击 + 麦克风按下/松开"
        );
        for binding in &catalog.default_bindings {
            assert!(crate::parse_button(&binding.button).is_some());
            assert!(crate::parse_trigger(&binding.trigger).is_some());
            let parsed = parse_action(&binding.action_key).expect("默认绑定的动作必须可解析");
            assert_eq!(action_key(&parsed), binding.action_key);
            assert!(!binding.name.is_empty());
            assert!(!binding.action.is_empty());
        }
    }
}
