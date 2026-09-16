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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProcessSpec {
    One(String),
    Many(Vec<String>),
}

impl ProcessSpec {
    /// 规范化后的候选进程名（去空白 + 小写）。匹配一律大小写不敏感。
    pub fn names(&self) -> Vec<String> {
        match self {
            ProcessSpec::One(name) => vec![normalize(name)],
            ProcessSpec::Many(names) => names.iter().map(|n| normalize(n)).collect(),
        }
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

/// 一个应用专属配置：一个 JSON 文件对应一个应用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppProfile {
    /// 匹配用的进程名。
    pub process: ProcessSpec,
    /// 展示名，例如「Codex 桌面版」。
    #[serde(default)]
    pub name: String,
    /// 备注：写清适配的是哪个版本/形态，以及哪些快捷键还没实测。
    #[serde(default)]
    pub note: String,
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
}
