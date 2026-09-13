//! core-mapping — RC003 13 键按键映射与触发规则。

pub mod trigger;
pub use trigger as gesture; // 向后兼容的别名

use serde::{Deserialize, Serialize};

/// RC003 上的 13 个物理按键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ButtonId {
    Power,
    Up,
    Down,
    Left,
    Right,
    Ok,
    Back,
    Home,
    Menu,
    Tv,
    VolumeUp,
    VolumeDown,
    Mic,
}

impl ButtonId {
    pub const ALL: [ButtonId; 13] = [
        ButtonId::Power,
        ButtonId::Up,
        ButtonId::Down,
        ButtonId::Left,
        ButtonId::Right,
        ButtonId::Ok,
        ButtonId::Back,
        ButtonId::Home,
        ButtonId::Menu,
        ButtonId::Tv,
        ButtonId::VolumeUp,
        ButtonId::VolumeDown,
        ButtonId::Mic,
    ];

    /// 前端与统计使用的稳定小写按键标识。
    pub fn key(self) -> &'static str {
        match self {
            ButtonId::Power => "power",
            ButtonId::Up => "up",
            ButtonId::Down => "down",
            ButtonId::Left => "left",
            ButtonId::Right => "right",
            ButtonId::Ok => "ok",
            ButtonId::Back => "back",
            ButtonId::Home => "home",
            ButtonId::Menu => "menu",
            ButtonId::Tv => "tv",
            ButtonId::VolumeUp => "volume_up",
            ButtonId::VolumeDown => "volume_down",
            ButtonId::Mic => "mic",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ButtonId::Power => "电源",
            ButtonId::Up => "上",
            ButtonId::Down => "下",
            ButtonId::Left => "左",
            ButtonId::Right => "右",
            ButtonId::Ok => "确定",
            ButtonId::Back => "返回",
            ButtonId::Home => "主页",
            ButtonId::Menu => "菜单",
            ButtonId::Tv => "TV",
            ButtonId::VolumeUp => "音量 +",
            ButtonId::VolumeDown => "音量 −",
            ButtonId::Mic => "麦克风",
        }
    }
}

/// 普通按键支持的触发手势。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    SingleClick,
    DoubleClick,
    LongPress,
    /// 物理按下瞬间触发（麦克风 PTT 等场景）。
    Press,
    /// 物理松开瞬间触发。
    Release,
}

/// 一个动作在 Windows 上可以执行的操作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    Disabled,
    KeyCombo(Vec<String>),
    Escape,
    Return,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    DeleteBackward,
    ShowDesktop,
    ContextMenu,
    AppSwitcher,
    SystemVolumeUp,
    SystemVolumeDown,
    SystemVolumeMute,
    PlayPause,
    Voice,
    OpenApp(String),
    /// 打开/关闭应用自带的右下角快捷菜单（由 Tauri 层执行）。
    ToggleQuickMenu,
}

/// 自定义快捷键动作的稳定前缀，前端 `action_key` 形如 `combo:ctrl+c`。
pub const COMBO_ACTION_PREFIX: &str = "combo:";

const MODIFIER_ORDER: [&str; 8] = [
    "lwin", "rwin", "lctrl", "rctrl", "lalt", "ralt", "lshift", "rshift",
];

fn canonical_combo_token(tok: &str) -> Option<String> {
    let t = tok.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    Some(match t.as_str() {
        "lwin" => "lwin".into(),
        "rwin" => "rwin".into(),
        "win" | "meta" | "super" => "lwin".into(),
        "lctrl" => "lctrl".into(),
        "rctrl" => "rctrl".into(),
        "ctrl" | "control" => "lctrl".into(),
        "lalt" => "lalt".into(),
        "ralt" => "ralt".into(),
        "alt" | "option" => "lalt".into(),
        "lshift" => "lshift".into(),
        "rshift" => "rshift".into(),
        "shift" => "lshift".into(),
        "esc" | "escape" => "esc".into(),
        "enter" | "return" => "enter".into(),
        "pgup" | "pageup" | "page_up" => "pageup".into(),
        "pgdn" | "pagedown" | "page_down" => "pagedown".into(),
        "del" | "delete" => "delete".into(),
        "ins" | "insert" => "insert".into(),
        "bs" | "backspace" => "backspace".into(),
        "spc" | "space" => "space".into(),
        "apps" | "context_menu" => "apps".into(),
        "tab" | "up" | "down" | "left" | "right" | "home" | "end" => t,
        other if is_letter_token(other) || is_digit_token(other) || is_fn_token(other) => {
            other.to_string()
        }
        _ => return None,
    })
}

fn is_letter_token(tok: &str) -> bool {
    matches!(tok.chars().next(), Some(c) if tok.len() == 1 && c.is_ascii_lowercase())
}

fn is_digit_token(tok: &str) -> bool {
    matches!(tok.chars().next(), Some(c) if tok.len() == 1 && c.is_ascii_digit())
}

fn is_fn_token(tok: &str) -> bool {
    let rest = match tok.strip_prefix('f') {
        Some(r) => r,
        None => return false,
    };
    rest.parse::<u8>()
        .ok()
        .is_some_and(|n| (1..=12).contains(&n))
}

fn is_modifier_token(tok: &str) -> bool {
    matches!(
        tok,
        "lwin" | "rwin" | "lctrl" | "rctrl" | "lalt" | "ralt" | "lshift" | "rshift"
    )
}

/// 解析用户快捷键（`ctrl` / `ctrl+c` / `Win+H`），得到规范 token 序列。
///
/// 规则：至少一个键；主键最多一个；修饰键可单独使用（如只按 Ctrl）。
/// 修饰键顺序固定为 win → ctrl → alt → shift。未知键或空输入返回 None。
pub fn parse_combo_spec(spec: &str) -> Option<Vec<String>> {
    let raw: Vec<String> = spec
        .split('+')
        .map(canonical_combo_token)
        .collect::<Option<Vec<_>>>()?;
    if raw.is_empty() {
        return None;
    }

    let mut modifiers = Vec::new();
    let mut mains = Vec::new();
    for tok in raw {
        if is_modifier_token(&tok) {
            if modifiers.contains(&tok) {
                return None;
            }
            modifiers.push(tok);
        } else {
            mains.push(tok);
        }
    }
    if mains.len() > 1 {
        return None;
    }
    if mains.is_empty() && modifiers.is_empty() {
        return None;
    }
    modifiers.sort_by_key(|m| {
        MODIFIER_ORDER
            .iter()
            .position(|x| x == m)
            .unwrap_or(MODIFIER_ORDER.len())
    });
    if let Some(main) = mains.pop() {
        modifiers.push(main);
    }
    Some(modifiers)
}

/// `combo:ctrl+c` → token 列表；非该前缀返回 None。
pub fn parse_combo_action_key(action_key: &str) -> Option<Vec<String>> {
    parse_combo_spec(action_key.strip_prefix(COMBO_ACTION_PREFIX)?)
}

/// 前端 / 配置使用的稳定动作标识，如 `combo:ctrl+shift+s`。
pub fn combo_action_key(tokens: &[String]) -> String {
    format!("{COMBO_ACTION_PREFIX}{}", tokens.join("+"))
}

/// 展示用组合键文案，如 `Ctrl+Shift+S`。
pub fn combo_display(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|t| combo_token_display(t))
        .collect::<Vec<_>>()
        .join("+")
}

fn combo_token_display(tok: &str) -> String {
    match tok {
        "lwin" | "win" => "左Win".into(),
        "rwin" => "右Win".into(),
        "lctrl" | "ctrl" => "左Ctrl".into(),
        "rctrl" => "右Ctrl".into(),
        "lalt" | "alt" => "左Alt".into(),
        "ralt" => "右Alt".into(),
        "lshift" | "shift" => "左Shift".into(),
        "rshift" => "右Shift".into(),
        "esc" => "Esc".into(),
        "enter" => "Enter".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "backspace" => "Backspace".into(),
        "delete" => "Delete".into(),
        "insert" => "Insert".into(),
        "space" => "Space".into(),
        "apps" => "Menu".into(),
        "tab" => "Tab".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        other if is_fn_token(other) => other.to_ascii_uppercase(),
        other if other.len() == 1 => other.to_ascii_uppercase(),
        other => other.to_string(),
    }
}

/// 语音识别目标：麦克风键的 Voice 动作唤起哪一家的语音输入。
///
/// 这是**系统级偏好**（在连接页「识别方案」里选），与按键映射解耦：
/// 映射表只表达「麦克风键 → 语音动作」，具体唤起谁由全局 `voice_target`
/// 决定。默认 `WindowsVoice`（Windows 自带 Win+H 语音键入），向后兼容。
///
/// 遥控器是按住传声、松手断流的硬件（PTT），因此任何目标都必须适配
/// 「按住说话」语义；「点一下开、中间不按着、再点一下关」在硬件上不可行。
/// 第三方输入法的实际唤起热键 / 是否支持按住说话需真机验证后再接入，
/// 不在本版本臆测填入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTarget {
    /// Windows 自带语音键入（Win+H）。默认值。
    #[default]
    WindowsVoice,
    /// 微信输入法（预留：唤起方式待真机验证）。
    ImeWechat,
    /// 豆包输入法（预留：唤起方式待真机验证）。
    ImeDoubao,
    /// 搜狗输入法（预留：唤起方式待真机验证）。
    ImeSogou,
}

impl VoiceTarget {
    /// 所有已知目标（前端下拉顺序与其一致）。
    pub const ALL: [VoiceTarget; 4] = [
        VoiceTarget::WindowsVoice,
        VoiceTarget::ImeWechat,
        VoiceTarget::ImeDoubao,
        VoiceTarget::ImeSogou,
    ];

    /// 稳定的 snake_case 字符串标识（前端 / 配置 / 日志共用）。
    pub fn key(self) -> &'static str {
        match self {
            VoiceTarget::WindowsVoice => "windows_voice",
            VoiceTarget::ImeWechat => "ime_wechat",
            VoiceTarget::ImeDoubao => "ime_doubao",
            VoiceTarget::ImeSogou => "ime_sogou",
        }
    }

    /// 前端展示名（简体中文）。
    pub fn display_name(self) -> &'static str {
        match self {
            VoiceTarget::WindowsVoice => "Windows 语音键入",
            VoiceTarget::ImeWechat => "微信输入法",
            VoiceTarget::ImeDoubao => "豆包输入法",
            VoiceTarget::ImeSogou => "搜狗输入法",
        }
    }

    /// 按字符串解析（配置/前端传入）。未知值返回 None，由调用方回落默认。
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == s)
    }
}

/// 一个 (button, gesture) -> action 绑定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyBinding {
    pub button: ButtonId,
    pub trigger: Trigger,
    pub action: ActionKind,
}

/// 映射配置：绑定列表 + 语音热键。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingConfig {
    pub bindings: Vec<KeyBinding>,
}

impl Default for MappingConfig {
    fn default() -> Self {
        Self {
            bindings: default_mapping(),
        }
    }
}

impl MappingConfig {
    /// 解析某个按钮对应的触发动作（单击/双击/长按/按下/松开）。
    pub fn resolve(&self, button: ButtonId, trigger: Trigger) -> Option<&ActionKind> {
        self.bindings
            .iter()
            .find(|b| b.button == button && b.trigger == trigger)
            .map(|b| &b.action)
    }

    /// 旧版本把麦克风映射为 SingleClick；迁移为按下/松开 PTT 默认。
    /// 返回是否发生了迁移。
    pub fn migrate_mic_ptt(&mut self) -> bool {
        let has_press = self
            .bindings
            .iter()
            .any(|b| b.button == ButtonId::Mic && b.trigger == Trigger::Press);
        let has_release = self
            .bindings
            .iter()
            .any(|b| b.button == ButtonId::Mic && b.trigger == Trigger::Release);
        if has_press || has_release {
            return false;
        }
        let had_single = self
            .bindings
            .iter()
            .any(|b| b.button == ButtonId::Mic && b.trigger == Trigger::SingleClick);
        if !had_single {
            return false;
        }
        self.bindings
            .retain(|b| !(b.button == ButtonId::Mic && b.trigger == Trigger::SingleClick));
        self.bindings.push(KeyBinding {
            button: ButtonId::Mic,
            trigger: Trigger::Press,
            action: ActionKind::Voice,
        });
        self.bindings.push(KeyBinding {
            button: ButtonId::Mic,
            trigger: Trigger::Release,
            action: ActionKind::Voice,
        });
        true
    }

    /// 旧版本把菜单键映射为右键菜单（ContextMenu）；迁移为打开/关闭
    /// 应用自带的快捷菜单。返回是否发生了迁移。
    pub fn migrate_menu_quickmenu(&mut self) -> bool {
        let mut changed = false;
        for b in self.bindings.iter_mut() {
            if b.button == ButtonId::Menu
                && b.trigger == Trigger::SingleClick
                && b.action == ActionKind::ContextMenu
            {
                b.action = ActionKind::ToggleQuickMenu;
                changed = true;
            }
        }
        changed
    }
}

/// 构建默认映射：12 键单击 + 麦克风按下/松开（PTT，Voice 动作）。
pub fn default_mapping() -> Vec<KeyBinding> {
    let mut bindings = Vec::new();
    use ActionKind as A;
    use ButtonId as B;
    use Trigger as T;

    let singles = [
        (B::Power, A::Escape),
        (B::Up, A::ArrowUp),
        (B::Down, A::ArrowDown),
        (B::Left, A::ArrowLeft),
        (B::Right, A::ArrowRight),
        (B::Ok, A::Return),
        (B::Back, A::DeleteBackward),
        (B::Home, A::ShowDesktop),
        (B::Menu, A::ToggleQuickMenu),
        (B::Tv, A::AppSwitcher),
        (B::VolumeUp, A::SystemVolumeUp),
        (B::VolumeDown, A::SystemVolumeDown),
    ];

    for (button, action) in singles {
        bindings.push(KeyBinding {
            button,
            trigger: T::SingleClick,
            action,
        });
    }

    // 麦克风是 PTT：按下和松手各触发一次 Voice（Win+H）。
    bindings.push(KeyBinding {
        button: B::Mic,
        trigger: T::Press,
        action: A::Voice,
    });
    bindings.push(KeyBinding {
        button: B::Mic,
        trigger: T::Release,
        action: A::Voice,
    });
    bindings
}

#[cfg(test)]
mod tests {
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
}
