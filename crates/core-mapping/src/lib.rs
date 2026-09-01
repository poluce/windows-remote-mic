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
        (B::Menu, A::ContextMenu),
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
}
