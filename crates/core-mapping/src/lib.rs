//! core-mapping — RC003 13 键按键映射与触发规则。

pub mod combo;
pub mod config;
pub mod trigger;
pub use trigger as gesture; // 向后兼容的别名

pub use combo::{
    combo_action_key, combo_display, parse_combo_action_key, parse_combo_spec, COMBO_ACTION_PREFIX,
};
pub use config::{default_mapping, KeyBinding, MappingConfig};

#[cfg(test)]
mod tests;

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
    /// 用 UI Automation 把键盘焦点送进前台窗口的输入框。
    ///
    /// 与 [`ActionKind::OpenApp`] 不是一回事：把窗口切到前台只抬升顶层窗口，
    /// 光标不会自己进输入框。适合绑在确定键上——选中应用、窗口到前台之后，
    /// 再按一次确定就能直接开始输入。
    FocusInput,
    /// 确定键的「对准输入框」版本：**焦点不在输入框就送进去，已经在里面就回车**。
    ///
    /// 这是「切应用 → 说话 → 发送」这条链路上最顺手的绑法：第一次按确定把光标
    /// 放进输入框，说完再按一次就是发送（回车）。找不到输入框时退化成普通回车。
    ///
    /// **它不是 [`ActionKind::Return`] 的超集**——差别不在「多做了什么」，而在
    /// 「少做了什么」：光标不在输入框时它**不发回车**，只把光标放进去，这正是
    /// 它的用处。两者会在「应用里有输入框、但光标不在里面」这个最常见的场景下
    /// 做出不同的结果：`Return` 无条件回车（列表里就是确认选中项），这个则改成
    /// 往输入框放光标。一个可预测、一个看上下文，绑哪个取决于你要哪种。
    ///
    /// 反过来说，**自适应会猜错**：既有关注列表又有搜索框的应用里，按确定想打开
    /// 选中项，它却可能把光标挪进搜索框。这种地方就该用 `Return`。
    FocusInputOrSubmit,
    OpenApp(String),
    /// 打开/关闭应用自带的右下角快捷菜单（由 Tauri 层执行）。
    ToggleQuickMenu,
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
