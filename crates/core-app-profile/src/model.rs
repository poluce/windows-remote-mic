//! 应用配置的数据模型。
//!
//! 刻意复用 `core-mapping` 的 `KeyBinding` / `ActionKind` 词表，不另造一套动作定义——
//! 这样 profile 里的写法和 `config.json` 的 `mapping` 完全一致，前端与调度器都不用改。

use core_mapping::KeyBinding;
use serde::{Deserialize, Serialize};

/// 进程名匹配规则。
///
/// JSON 里既可以写单个字符串，也可以写字符串数组（同一应用的不同可执行文件）：
///
/// ```json
/// { "process": "Codex.exe" }
/// { "process": ["Codex.exe", "ChatGPT.exe"] }
/// ```
///
/// 也可以**整条省略**——有些目标在 Windows 侧根本没有自己的进程
/// （跑在 WSL 里的服务、Chrome PWA），只能靠 [`AppProfile::window_title_contains`] 认。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProcessSpec {
    One(String),
    Many(Vec<String>),
}

impl Default for ProcessSpec {
    /// 空规则：任何进程名都不匹配（等价于「本配置不看进程名」）。
    fn default() -> Self {
        ProcessSpec::Many(Vec::new())
    }
}

impl ProcessSpec {
    /// 规范化后的候选进程名（去空白 + 小写）。匹配一律大小写不敏感。
    pub fn names(&self) -> Vec<String> {
        match self {
            ProcessSpec::One(name) => vec![normalize(name)],
            ProcessSpec::Many(names) => names.iter().map(|n| normalize(n)).collect(),
        }
    }

    /// 是否声明了任何进程名。
    pub fn is_empty(&self) -> bool {
        self.names().iter().all(|n| n.is_empty())
    }

    /// 是否匹配给定的可执行文件名（可传完整路径，只看文件名）。
    pub fn matches(&self, exe_name: &str) -> bool {
        let target = normalize(exe_name);
        !target.is_empty() && self.names().contains(&target)
    }

    /// 用于 UI 展示的写法。
    pub fn display(&self) -> String {
        match self {
            ProcessSpec::One(name) => name.clone(),
            ProcessSpec::Many(names) => names.join(" / "),
        }
    }
}

fn normalize(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

/// 快捷菜单里用的矢量简标（首字母 + 品牌色），不依赖图标资源。
///
/// 两个字段都允许留空，空值各自回退到自动值（见 [`AppProfile::menu_icon`]）：
/// 用户在「新建应用」里只挑了颜色时，落盘的就是 `{ "color": "#..." }`，
/// 简标文字继续跟着展示名自动变（改名字不用再手动改简标）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IconSpec {
    /// 圆圈里显示的字，1–2 个字符；留空表示按展示名自动取。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// 品牌色，形如 `#3B82F6`；留空表示按配置 id 自动取。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub color: String,
}

/// 自动简标与预设色卡共用的调色板。
///
/// 这组是按用户要求定下的默认色（暖色起手、冷色收尾）；快捷菜单绘制时会整体向
/// 中性灰混 22%，落在浅色底上仍然收得住。颜色只由稳定 id 决定，改展示名不会
/// 换色、重启也不会变。
///
/// 「新建应用」里给用户挑的也是这同一组颜色（命令 `app_icon_palette` 交给前端），
/// 避免出现调色板和内环观感对不上的第二套色卡。
static AUTO_ICON_COLORS: [&str; 8] = [
    "#FF4D4F", // 珊瑚红
    "#FF7A00", // 橙
    "#FFB800", // 琥珀
    "#FFE600", // 亮黄
    "#8CE000", // 黄绿
    "#4D6BFE", // 钢蓝
    "#FF2A85", // 玫红
    "#B84DFF", // 紫
];

/// 可选简标颜色（供「新建应用」/ 作用范围的颜色选择器使用）。
pub fn icon_palette() -> &'static [&'static str] {
    &AUTO_ICON_COLORS
}

impl IconSpec {
    /// 从展示名与稳定 id 生成简标：名字取前 1–2 个字母/数字/汉字，颜色按 id 定。
    ///
    /// 「新建应用」只填名称与进程名/标题，没有图标编辑器；内环又要能直接切到
    /// 用户自己建的配置，所以没写 `icon` 的配置在这里得到一个可用的兜底简标。
    /// 结果只依赖入参，因此每次打开菜单、每次启动都一致。
    pub fn auto(name: &str, seed: &str) -> Self {
        Self {
            label: auto_label(name),
            color: color_from_seed(seed).to_string(),
        }
    }

    /// 只指定颜色、简标文字留空（跟着展示名自动变）的简标。
    pub fn with_color(color: &str) -> Self {
        Self {
            label: String::new(),
            color: color.to_string(),
        }
    }

    /// 规范化用户输入的颜色：接受 `#rrggbb` / `rrggbb`，统一成小写 `#rrggbb`。
    ///
    /// 这是唯一入口——命令层与配置层都走它，避免把 `red`、`#FFF` 这类
    /// 前端猜不出 Canvas 会怎么画的写法落进配置文件。
    pub fn parse_color(raw: &str) -> Result<String, String> {
        let text = raw.trim();
        let hex = text.strip_prefix('#').unwrap_or(text);
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "颜色格式应为 #RRGGBB（例如 #4D6BFE），收到「{text}」"
            ));
        }
        Ok(format!("#{}", hex.to_ascii_lowercase()))
    }
}

/// 取展示名的前 1–2 个字母/数字/汉字；取不到时退回 `?`，避免画出空圆圈。
fn auto_label(name: &str) -> String {
    let mut label = String::new();
    for ch in name.chars() {
        if !ch.is_alphanumeric() {
            continue;
        }
        // ASCII 字母统一大写，和种子里的「CC」「DS」「Z」观感一致。
        label.push(if ch.is_ascii_alphabetic() {
            ch.to_ascii_uppercase()
        } else {
            ch
        });
        if label.chars().count() >= 2 {
            break;
        }
    }
    if label.is_empty() {
        "?".to_string()
    } else {
        label
    }
}

/// FNV-1a：把稳定 id 映射到调色板某一项。不用 `RandomState`——它每次进程启动
/// 都会换种子，菜单颜色会跟着闪。
fn color_from_seed(seed: &str) -> &'static str {
    let mut hash: u32 = 2_166_136_261;
    for byte in seed.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    AUTO_ICON_COLORS[hash as usize % AUTO_ICON_COLORS.len()]
}

/// 启动一个应用的方式。
///
/// 三种形态对应实际安装情况：
/// - 普通安装包（ZCode）→ `path`
/// - MSIX / 打包应用（Claude）→ `appid`，经 `shell:AppsFolder\` 启动
/// - 网页应用 / 本地服务（Chrome PWA 的 DSH）→ `url`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchSpec {
    /// 可执行文件路径。
    Path { value: String },
    /// 打包应用的 AppUserModelID，例如 `Claude_pzs8sxrjxfjjc!Claude`。
    Appid { value: String },
    /// 用默认浏览器打开的 URL。
    Url { value: String },
}

/// 一个应用专属配置：一个 JSON 文件对应一个应用。
///
/// 实现了 `Default`：新增可选字段时，手写结构体的测试用
/// `..Default::default()` 补齐即可，不必每加一个字段就全仓库改一遍。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppProfile {
    /// 稳定标识：`app-profiles/` 里那个文件的名字（不含 `.json`）。
    ///
    /// 只用来定位「保存 / 删除哪一份配置」，**不参与匹配**，也**不写进 JSON**
    /// （`serde(skip)`）。展示名 `name` 是给人看的、随时可以改，不能拿来当身份——
    /// 否则用户改个名字，快捷菜单的「点图标打开」和编辑器的「保存到这份配置」
    /// 就会同时找不到目标。
    #[serde(skip)]
    pub id: String,
    /// 匹配用的进程名；省略则只看窗口标题。
    #[serde(default)]
    pub process: ProcessSpec,
    /// 展示名，例如「Codex 桌面版」。
    #[serde(default)]
    pub name: String,
    /// 备注：写清适配的是哪个版本/形态，以及哪些快捷键还没实测。
    #[serde(default)]
    pub note: String,
    /// 窗口标题匹配（任一命中即算匹配）。
    ///
    /// 用于**进程名区分不出**的目标：Chrome PWA（如 DSH）的前台进程是
    /// `chrome.exe`，只能靠窗口标题认出它是哪个网页应用。
    #[serde(default)]
    pub window_title_contains: Vec<String>,
    /// 快捷菜单内圈的简标；没有则不进菜单。
    #[serde(default)]
    pub icon: Option<IconSpec>,
    /// 启动方式；没有时只能「聚焦已打开的窗口」，不能启动。
    #[serde(default)]
    pub launch: Option<LaunchSpec>,
    /// 覆盖的基础映射。未列出的 (按键, 触发) 继续沿用全局映射。
    #[serde(default)]
    pub bindings: Vec<KeyBinding>,
}

impl AppProfile {
    /// UI 展示名；没写 `name` 时退回进程名，避免出现空标题。
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            self.process.display()
        } else {
            self.name.clone()
        }
    }

    /// 快捷菜单内环实际要画的简标：**每份配置都有**。
    ///
    /// 显式写了 `icon` 就用它（空 label / 空 color 各自回退到自动值），没写就
    /// 自动生成——内环不再以「有没有 icon 字段」当入菜单开关，用户在这里新建
    /// 的每一份应用配置都能直接在菜单里切换。
    pub fn menu_icon(&self) -> IconSpec {
        let name = self.display_name();
        // id 是文件名（稳定）；测试或临时构造的配置没有 id 时退回展示名，
        // 至少同一进程内颜色是稳定的。
        let seed = if self.id.is_empty() {
            name.clone()
        } else {
            self.id.clone()
        };
        let auto = IconSpec::auto(&name, &seed);

        match &self.icon {
            Some(icon) => IconSpec {
                label: if icon.label.trim().is_empty() {
                    auto.label
                } else {
                    icon.label.clone()
                },
                color: if icon.color.trim().is_empty() {
                    auto.color
                } else {
                    icon.color.clone()
                },
            },
            None => auto,
        }
    }

    /// 是否匹配给定的前台上下文。
    ///
    /// 进程名命中即算命中；否则再看窗口标题——后者是 Chrome PWA 唯一可行的识别方式。
    pub fn matches_context(&self, exe_name: &str, window_title: &str) -> bool {
        self.process.matches(exe_name) || self.matches_title(window_title)
    }

    /// 窗口标题是否命中本配置。
    pub fn matches_title(&self, window_title: &str) -> bool {
        let title = window_title.trim().to_lowercase();
        !title.is_empty()
            && self
                .title_keys()
                .iter()
                .any(|needle| title.contains(needle))
    }

    /// 会不会和另一份配置抢同一个前台上下文。
    ///
    /// 进程名有交集，或窗口标题关键字有交集，就算冲突。**标题这一半不能省**：
    /// 纯标题配置（Chrome PWA、WSL 里的服务）的 `process` 是空的，只按进程名
    /// 判重的话两份同标题的配置会同时留在注册表里，而匹配用的是 `.find()`——
    /// 永远只有先加载的那份生效，另一份变成「改了没反应」。
    pub fn conflicts_with(&self, other: &Self) -> bool {
        let theirs = other.process.names();
        if self
            .process
            .names()
            .iter()
            .any(|name| !name.is_empty() && theirs.contains(name))
        {
            return true;
        }
        let theirs = other.title_keys();
        self.title_keys().iter().any(|key| theirs.contains(key))
    }

    /// 规范化后的标题关键字（去空白 + 小写 + 丢掉空串）。空串不能变成「匹配一切」。
    fn title_keys(&self) -> Vec<String> {
        self.window_title_contains
            .iter()
            .map(|needle| needle.trim().to_lowercase())
            .filter(|needle| !needle.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_spec_accepts_single_and_list() {
        let one: ProcessSpec = serde_json::from_str("\"Codex.exe\"").unwrap();
        assert_eq!(one.names(), vec!["codex.exe"]);

        let many: ProcessSpec = serde_json::from_str("[\"Codex.exe\", \"ChatGPT.exe\"]").unwrap();
        assert_eq!(many.names(), vec!["codex.exe", "chatgpt.exe"]);
        assert_eq!(many.display(), "Codex.exe / ChatGPT.exe");
    }

    #[test]
    fn matching_is_case_insensitive_and_ignores_blank() {
        let spec = ProcessSpec::One("Codex.exe".into());
        assert!(spec.matches("codex.exe"));
        assert!(spec.matches("CODEX.EXE"));
        assert!(spec.matches("  Codex.exe  "));
        assert!(!spec.matches("codex-cli.exe"));
        assert!(!spec.matches(""));
        assert!(!ProcessSpec::One("  ".into()).matches(""));
    }

    #[test]
    fn display_name_falls_back_to_process() {
        let json = r#"{ "process": "Codex.exe" }"#;
        let profile: AppProfile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.display_name(), "Codex.exe");
        assert!(profile.bindings.is_empty());
        assert!(profile.note.is_empty());
    }

    #[test]
    fn bindings_deserialize_in_config_json_shape() {
        // 与 config.json 的 mapping 完全同构，便于人工互相拷贝。
        let json = r#"{
            "process": "Codex.exe",
            "name": "Codex 桌面版",
            "bindings": [
                { "button": "Ok", "trigger": "SingleClick", "action": "Return" },
                { "button": "Menu", "trigger": "SingleClick",
                  "action": { "KeyCombo": ["lctrl", "k"] } }
            ]
        }"#;
        let profile: AppProfile = serde_json::from_str(json).unwrap();
        assert_eq!(profile.bindings.len(), 2);
        assert_eq!(profile.display_name(), "Codex 桌面版");
    }

    #[test]
    fn process_can_be_omitted_for_title_only_profiles() {
        // 跑在 WSL 里的服务只有浏览器窗口是 Windows 进程，没有进程名可填。
        let json = r#"{
            "name": "DeepSeek Harness",
            "window_title_contains": ["DeepSeek Harness"]
        }"#;
        let profile: AppProfile = serde_json::from_str(json).unwrap();
        assert!(profile.process.is_empty());
        assert!(!profile.process.matches("chrome.exe"));
        assert!(profile.matches_context("chrome.exe", "聊天 · DeepSeek Harness"));
        assert!(profile.launch.is_none());
        assert!(profile.icon.is_none());
    }

    #[test]
    fn title_matching_is_case_insensitive_and_any_of() {
        let json = r#"{
            "process": "Codex.exe",
            "window_title_contains": ["ChatGPT", "codex"]
        }"#;
        let profile: AppProfile = serde_json::from_str(json).unwrap();
        assert!(profile.matches_title("ChatGPT — 新对话"));
        assert!(profile.matches_title("CODEX 工作区"));
        assert!(!profile.matches_title("Claude"));
        assert!(!profile.matches_title("   "));
        // 空的匹配串不能变成「匹配一切」。
        let blank: AppProfile =
            serde_json::from_str(r#"{ "process": "A.exe", "window_title_contains": ["  "] }"#)
                .unwrap();
        assert!(!blank.matches_title("任何标题"));
    }

    #[test]
    fn icon_and_launch_deserialize() {
        // 注意用 r##"…"##：JSON 里有 `"#F59E0B"`，`"#` 会提前结束 r#"…"#。
        let json = r##"{
            "name": "ZCode",
            "process": "ZCode.exe",
            "window_title_contains": ["ZCode"],
            "icon": { "label": "Z", "color": "#F59E0B" },
            "launch": { "kind": "appid", "value": "dev.zcode.app" }
        }"##;
        let profile: AppProfile = serde_json::from_str(json).unwrap();
        assert_eq!(
            profile.icon,
            Some(IconSpec {
                label: "Z".into(),
                color: "#F59E0B".into()
            })
        );
        assert_eq!(
            profile.launch,
            Some(LaunchSpec::Appid {
                value: "dev.zcode.app".into()
            })
        );
    }

    #[test]
    fn launch_kinds_are_strict() {
        // kind 拼错必须报错，而不是静默退化成某个默认值。
        let bad = r#"{ "name": "X", "launch": { "kind": "exe", "value": "x" } }"#;
        assert!(serde_json::from_str::<AppProfile>(bad).is_err());
    }
}
