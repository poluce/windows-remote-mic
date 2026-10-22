//! 映射编辑的字符串 <-> 领域类型编解码（前后端协议层）。

use serde::Serialize;

use core_mapping::{ActionKind, ButtonId, Trigger};

/// 面向前端展示的映射条目。
#[derive(Serialize)]
pub(crate) struct MappingEntry {
    pub(crate) button: String,
    pub(crate) name: String,
    pub(crate) trigger: String,
    pub(crate) action: String,
    pub(crate) action_key: String,
}

/// 持久化设置的辅助函数。
pub(crate) fn config_store() -> Option<core_config::ConfigStore> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    core_config::ConfigStore::new(std::path::Path::new(&base).join("RemoteMic/RC003")).ok()
}

/// 加载应用专属按键配置：**先播种，再读目录**。
///
/// 配置只有一个来源——`<配置目录>/app-profiles/*.json`。仓库根 `profiles/*.json`
/// 是编译进来的种子，只在还没有同名文件时落地一次；落地之后磁盘上的文件就是
/// 唯一事实来源，程序不再覆盖它（所以用户删掉的配置不会在下次启动时复活）。
pub(crate) fn load_app_profiles() -> core_app_profile::ProfileRegistry {
    let Some(store) = config_store() else {
        core_log::log_warn("[app-profile] 拿不到配置目录，本次没有任何应用配置");
        return core_app_profile::ProfileRegistry::default();
    };
    let written = core_app_profile::seed_user_dir(&store.dir);
    if written > 0 {
        core_log::log_line(&format!("[app-profile] 首次落地了 {written} 份种子配置"));
    }
    core_app_profile::ProfileRegistry::load(Some(
        &store.dir.join(core_app_profile::USER_PROFILE_DIR),
    ))
}

/// 来自设置界面的映射编辑数据。
#[derive(serde::Deserialize)]
pub(crate) struct MappingEdit {
    pub(crate) button: String,
    pub(crate) trigger: String,
    pub(crate) action: String,
}

pub(crate) fn parse_button(s: &str) -> Option<ButtonId> {
    match s {
        "power" => Some(ButtonId::Power),
        "up" => Some(ButtonId::Up),
        "down" => Some(ButtonId::Down),
        "left" => Some(ButtonId::Left),
        "right" => Some(ButtonId::Right),
        "ok" => Some(ButtonId::Ok),
        "back" => Some(ButtonId::Back),
        "home" => Some(ButtonId::Home),
        "menu" => Some(ButtonId::Menu),
        "tv" => Some(ButtonId::Tv),
        "volume_up" => Some(ButtonId::VolumeUp),
        "volume_down" => Some(ButtonId::VolumeDown),
        "mic" => Some(ButtonId::Mic),
        _ => None,
    }
}

pub(crate) fn parse_trigger(s: &str) -> Option<Trigger> {
    match s {
        "single_click" => Some(Trigger::SingleClick),
        "double_click" => Some(Trigger::DoubleClick),
        "long_press" => Some(Trigger::LongPress),
        "press" => Some(Trigger::Press),
        "release" => Some(Trigger::Release),
        _ => None,
    }
}

pub(crate) fn parse_action(s: &str) -> Option<ActionKind> {
    use ActionKind as A;
    if let Some(tokens) = core_mapping::parse_combo_action_key(s) {
        return Some(A::KeyCombo(tokens));
    }
    Some(match s {
        "disabled" => A::Disabled,
        "escape" => A::Escape,
        "return" => A::Return,
        "arrow_up" => A::ArrowUp,
        "arrow_down" => A::ArrowDown,
        "arrow_left" => A::ArrowLeft,
        "arrow_right" => A::ArrowRight,
        "delete_backward" => A::DeleteBackward,
        "show_desktop" => A::ShowDesktop,
        "context_menu" => A::ContextMenu,
        "app_switcher" => A::AppSwitcher,
        "system_volume_up" => A::SystemVolumeUp,
        "system_volume_down" => A::SystemVolumeDown,
        "system_volume_mute" => A::SystemVolumeMute,
        "play_pause" => A::PlayPause,
        "voice" => A::Voice,
        "focus_input" => A::FocusInput,
        "focus_input_or_submit" => A::FocusInputOrSubmit,
        "toggle_quick_menu" => A::ToggleQuickMenu,
        _ => return None,
    })
}

/// 前端使用的稳定小写按键标识。
pub(crate) fn button_key(button: &ButtonId) -> String {
    button.key().to_string()
}

/// 前端使用的稳定小写触发方式标识。
pub(crate) fn trigger_key(trigger: &Trigger) -> String {
    match trigger {
        Trigger::SingleClick => "single_click",
        Trigger::DoubleClick => "double_click",
        Trigger::LongPress => "long_press",
        Trigger::Press => "press",
        Trigger::Release => "release",
    }
    .to_string()
}

/// 前端动作选择器使用的稳定动作标识。
pub(crate) fn action_key(action: &ActionKind) -> String {
    match action {
        ActionKind::KeyCombo(keys) => return core_mapping::combo_action_key(keys),
        ActionKind::Disabled => "disabled",
        ActionKind::Escape => "escape",
        ActionKind::Return => "return",
        ActionKind::ArrowUp => "arrow_up",
        ActionKind::ArrowDown => "arrow_down",
        ActionKind::ArrowLeft => "arrow_left",
        ActionKind::ArrowRight => "arrow_right",
        ActionKind::DeleteBackward => "delete_backward",
        ActionKind::ShowDesktop => "show_desktop",
        ActionKind::ContextMenu => "context_menu",
        ActionKind::AppSwitcher => "app_switcher",
        ActionKind::SystemVolumeUp => "system_volume_up",
        ActionKind::SystemVolumeDown => "system_volume_down",
        ActionKind::SystemVolumeMute => "system_volume_mute",
        ActionKind::PlayPause => "play_pause",
        ActionKind::Voice => "voice",
        ActionKind::FocusInput => "focus_input",
        ActionKind::FocusInputOrSubmit => "focus_input_or_submit",
        ActionKind::OpenApp(_) => "open_app",
        ActionKind::ToggleQuickMenu => "toggle_quick_menu",
    }
    .to_string()
}

pub(crate) fn action_label(action: &ActionKind) -> String {
    match action {
        ActionKind::Disabled => "禁用".into(),
        ActionKind::KeyCombo(keys) => format!("快捷键 {}", core_mapping::combo_display(keys)),
        ActionKind::Escape => "取消（Esc）".into(),
        ActionKind::Return => "回车（Enter）".into(),
        ActionKind::ArrowUp => "↑".into(),
        ActionKind::ArrowDown => "↓".into(),
        ActionKind::ArrowLeft => "←".into(),
        ActionKind::ArrowRight => "→".into(),
        ActionKind::DeleteBackward => "删除（退格）".into(),
        ActionKind::ShowDesktop => "显示桌面（Win+D）".into(),
        ActionKind::ContextMenu => "右键菜单（上下文菜单）".into(),
        ActionKind::AppSwitcher => "切换应用（Alt+Tab）".into(),
        ActionKind::SystemVolumeUp => "音量 +".into(),
        ActionKind::SystemVolumeDown => "音量 −".into(),
        ActionKind::SystemVolumeMute => "静音".into(),
        ActionKind::PlayPause => "播放/暂停".into(),
        ActionKind::Voice => "语音输入".into(),
        ActionKind::FocusInput => "聚焦输入框".into(),
        ActionKind::FocusInputOrSubmit => "聚焦输入框 / 回车".into(),
        ActionKind::OpenApp(name) => format!("打开应用：{name}"),
        ActionKind::ToggleQuickMenu => "快捷菜单（开/关）".into(),
    }
}
