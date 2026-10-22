//! 映射配置结构体与默认配置。

use serde::{Deserialize, Serialize};

use super::{ActionKind, ButtonId, Trigger};

/// 单条按键映射：物理按钮 + 触发方式 → 动作。
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
