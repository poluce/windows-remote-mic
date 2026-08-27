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
    pub fn by_button(&self, button: ButtonId) -> Vec<&KeyBinding> {
        self.bindings
            .iter()
            .filter(|b| b.button == button)
            .collect()
    }

    /// 解析某个按钮对应的单击/双击/长按动作。
    pub fn resolve(&self, button: ButtonId, trigger: Trigger) -> Option<&ActionKind> {
        self.bindings
            .iter()
            .find(|b| b.button == button && b.trigger == trigger)
            .map(|b| &b.action)
    }
}

/// 判断一个动作在物理按键按住期间是否保持可重复触发。
pub fn action_allows_repeat(action: &ActionKind) -> bool {
    !matches!(
        action,
        ActionKind::OpenApp(_) | ActionKind::AppSwitcher | ActionKind::Voice
    )
}

/// 构建默认的 13 键单击映射。
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
        (B::Menu, A::ContextMenu),
        (B::Tv, A::AppSwitcher),
        (B::VolumeUp, A::SystemVolumeUp),
        (B::VolumeDown, A::SystemVolumeDown),
        (B::Mic, A::Voice),
    ];

    for (button, action) in singles {
        bindings.push(KeyBinding {
            button,
            trigger: T::SingleClick,
            action,
        });
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_13_single_click_bindings() {
        let cfg = MappingConfig::default();
        assert_eq!(
            cfg.bindings
                .iter()
                .filter(|b| b.trigger == Trigger::SingleClick)
                .count(),
            13
        );
    }

    #[test]
    fn mic_is_voice() {
        let cfg = MappingConfig::default();
        assert_eq!(
            cfg.resolve(ButtonId::Mic, Trigger::SingleClick),
            Some(&ActionKind::Voice)
        );
    }

    #[test]
    fn arrows_repeat_but_open_app_does_not() {
        assert!(action_allows_repeat(&ActionKind::ArrowUp));
        assert!(!action_allows_repeat(&ActionKind::OpenApp("codex".into())));
        assert!(!action_allows_repeat(&ActionKind::AppSwitcher));
    }

    #[test]
    fn resolve_missing_trigger_returns_none() {
        let cfg = MappingConfig::default();
        assert_eq!(cfg.resolve(ButtonId::Ok, Trigger::LongPress), None);
    }
}
