//! core-mapping 单元测试。

use super::*;

#[test]
fn default_has_12_single_click_and_mic_ptt() {
    let cfg = MappingConfig::default();
    assert_eq!(
        cfg.bindings
            .iter()
            .filter(|b| b.trigger == Trigger::SingleClick)
            .count(),
        12
    );
    assert_eq!(cfg.bindings.len(), 14);
}

#[test]
fn mic_is_voice_on_press_and_release() {
    let cfg = MappingConfig::default();
    assert_eq!(
        cfg.resolve(ButtonId::Mic, Trigger::Press),
        Some(&ActionKind::Voice)
    );
    assert_eq!(
        cfg.resolve(ButtonId::Mic, Trigger::Release),
        Some(&ActionKind::Voice)
    );
    assert_eq!(cfg.resolve(ButtonId::Mic, Trigger::SingleClick), None);
}

#[test]
fn migrate_mic_ptt_replaces_legacy_single_click() {
    let mut cfg = MappingConfig {
        bindings: vec![KeyBinding {
            button: ButtonId::Mic,
            trigger: Trigger::SingleClick,
            action: ActionKind::Voice,
            enabled: true,
        }],
    };
    assert!(cfg.migrate_mic_ptt());
    assert_eq!(
        cfg.resolve(ButtonId::Mic, Trigger::Press),
        Some(&ActionKind::Voice)
    );
    assert_eq!(
        cfg.resolve(ButtonId::Mic, Trigger::Release),
        Some(&ActionKind::Voice)
    );
    assert_eq!(cfg.resolve(ButtonId::Mic, Trigger::SingleClick), None);
    assert!(!cfg.migrate_mic_ptt(), "已迁移后不应重复迁移");
}

#[test]
fn resolve_missing_trigger_returns_none() {
    let cfg = MappingConfig::default();
    assert_eq!(cfg.resolve(ButtonId::Ok, Trigger::LongPress), None);
}

#[test]
fn menu_defaults_to_toggle_quick_menu() {
    let cfg = MappingConfig::default();
    assert_eq!(
        cfg.resolve(ButtonId::Menu, Trigger::SingleClick),
        Some(&ActionKind::ToggleQuickMenu)
    );
}

#[test]
fn migrate_menu_quickmenu_replaces_legacy_context_menu() {
    let mut cfg = MappingConfig {
        bindings: vec![KeyBinding {
            button: ButtonId::Menu,
            trigger: Trigger::SingleClick,
            action: ActionKind::ContextMenu,
            enabled: true,
        }],
    };
    assert!(cfg.migrate_menu_quickmenu());
    assert_eq!(
        cfg.resolve(ButtonId::Menu, Trigger::SingleClick),
        Some(&ActionKind::ToggleQuickMenu)
    );
    assert!(!cfg.migrate_menu_quickmenu(), "已迁移后不应重复迁移");
}

#[test]
fn migrate_menu_quickmenu_keeps_custom_menu_binding() {
    let mut cfg = MappingConfig {
        bindings: vec![KeyBinding {
            button: ButtonId::Menu,
            trigger: Trigger::SingleClick,
            action: ActionKind::OpenApp("notepad".into()),
            enabled: true,
        }],
    };
    assert!(!cfg.migrate_menu_quickmenu(), "自定义映射不应被迁移覆盖");
    assert_eq!(
        cfg.resolve(ButtonId::Menu, Trigger::SingleClick),
        Some(&ActionKind::OpenApp("notepad".into()))
    );
}

#[test]
fn voice_target_default_is_windows_voice() {
    assert_eq!(VoiceTarget::default(), VoiceTarget::WindowsVoice);
}

#[test]
fn voice_target_key_roundtrip() {
    for t in VoiceTarget::ALL {
        assert_eq!(VoiceTarget::parse(t.key()), Some(t));
    }
    assert_eq!(VoiceTarget::parse("unknown_target"), None);
}

#[test]
fn voice_target_serde_snake_case() {
    let json = serde_json::to_string(&VoiceTarget::ImeWechat).unwrap();
    assert_eq!(json, "\"ime_wechat\"");
    let back: VoiceTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(back, VoiceTarget::ImeWechat);
}

#[test]
fn parse_combo_spec_canonicalizes_order_and_aliases() {
    assert_eq!(
        parse_combo_spec("Shift+Ctrl+s"),
        Some(vec!["lctrl".into(), "lshift".into(), "s".into()])
    );
    assert_eq!(
        parse_combo_spec("win+h"),
        Some(vec!["lwin".into(), "h".into()])
    );
    assert_eq!(parse_combo_spec("F5"), Some(vec!["f5".into()]));
    assert_eq!(parse_combo_spec("ctrl"), Some(vec!["lctrl".into()]));
    assert_eq!(parse_combo_spec("rctrl"), Some(vec!["rctrl".into()]));
    assert_eq!(
        parse_combo_spec("rshift+lctrl"),
        Some(vec!["lctrl".into(), "rshift".into()])
    );
    assert_eq!(
        parse_combo_spec("alt+ctrl"),
        Some(vec!["lctrl".into(), "lalt".into()])
    );
    assert_eq!(combo_display(&["rctrl".into()]), "右Ctrl");
    assert_eq!(parse_combo_spec("escape"), Some(vec!["esc".into()]));
    assert_eq!(
        parse_combo_action_key("combo:ctrl+c"),
        Some(vec!["lctrl".into(), "c".into()])
    );
    assert_eq!(
        combo_action_key(&["lctrl".into(), "c".into()]),
        "combo:lctrl+c"
    );
    assert_eq!(
        combo_display(&["lctrl".into(), "lshift".into(), "s".into()]),
        "左Ctrl+左Shift+S"
    );
}

#[test]
fn parse_combo_spec_rejects_invalid() {
    assert_eq!(parse_combo_spec(""), None);
    assert_eq!(parse_combo_spec("ctrl+c+v"), None, "不能一次发两个主键");
    assert_eq!(parse_combo_spec("ctrl+ctrl+c"), None);
    assert_eq!(parse_combo_spec("ctrl+foo"), None);
    assert_eq!(parse_combo_action_key("return"), None);
}

#[test]
fn default_bindings_are_enabled() {
    assert!(MappingConfig::default().bindings.iter().all(|b| b.enabled));
}

#[test]
fn legacy_json_without_enabled_defaults_to_enabled() {
    let json = r#"{"button":"Ok","trigger":"SingleClick","action":"Return"}"#;
    let binding: KeyBinding = serde_json::from_str(json).unwrap();
    assert!(binding.enabled, "旧配置没有 enabled 字段时应默认启用");
}

#[test]
fn disabled_binding_keeps_action_in_config() {
    let cfg = MappingConfig {
        bindings: vec![KeyBinding {
            button: ButtonId::VolumeUp,
            trigger: Trigger::SingleClick,
            action: ActionKind::SystemVolumeUp,
            enabled: false,
        }],
    };
    // 配置层仍能读到动作（编辑器要显示它）；执行侧由调度器过滤。
    assert_eq!(
        cfg.resolve(ButtonId::VolumeUp, Trigger::SingleClick),
        Some(&ActionKind::SystemVolumeUp)
    );
    assert!(
        !cfg.binding(ButtonId::VolumeUp, Trigger::SingleClick)
            .unwrap()
            .enabled
    );
}

#[test]
fn migrate_disabled_bindings_removes_legacy_disabled_action() {
    let mut cfg = MappingConfig {
        bindings: vec![
            KeyBinding {
                button: ButtonId::Ok,
                trigger: Trigger::SingleClick,
                action: ActionKind::Return,
                enabled: true,
            },
            KeyBinding {
                button: ButtonId::Ok,
                trigger: Trigger::DoubleClick,
                action: ActionKind::Disabled,
                enabled: true,
            },
        ],
    };
    assert!(cfg.migrate_disabled_bindings());
    assert_eq!(cfg.bindings.len(), 1);
    assert_eq!(cfg.bindings[0].action, ActionKind::Return);
    assert!(!cfg.migrate_disabled_bindings(), "已迁移后不应重复迁移");
}
