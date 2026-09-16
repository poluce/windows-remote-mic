//! core-app-profile — 按前台应用切换按键配置（**一个应用一个文件**）。
//!
//! # 这个 crate 负责什么
//!
//! 只做两件事：
//! 1. 加载并校验「一个应用一个文件」的配置；
//! 2. 按前台窗口的进程名匹配到某份配置。
//!
//! 它**不**发按键、**不**依赖 `core-dispatch` / `core-input` / Tauri，
//! 也不决定「怎么覆盖映射」——那是上层的事。上层拿
//! [`ProfileRegistry::match_process`] 的结果去覆盖即可。
//!
//! # 配置来源
//!
//! 后者按进程名覆盖前者：
//! - **内置**：仓库根 `profiles/*.json`，由 `build.rs` 编进二进制，新增应用不用改 Rust 代码；
//! - **用户**：`<配置目录>/app-profiles/*.json`（见 [`USER_PROFILE_DIR`]）。
//!
//! # 用法
//!
//! ```no_run
//! use core_app_profile::{foreground_process_name, ProfileRegistry, USER_PROFILE_DIR};
//! use std::path::PathBuf;
//!
//! // 配置目录由调用方决定（src-tauri 用 core-config::ConfigStore::dir），
//! // 这样本 crate 不需要知道路径约定。
//! let config_dir = PathBuf::from(r"C:\Users\me\AppData\Local\RemoteMic\RC003");
//! let registry = ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)));
//!
//! if let Some(exe) = foreground_process_name() {
//!     if let Some(profile) = registry.match_process(&exe) {
//!         println!("命中应用配置：{}", profile.display_name());
//!     }
//! }
//! ```

mod foreground;
mod model;
mod window;

pub use foreground::{file_name, foreground_process_name, foreground_window_title};
pub use model::{AppProfile, IconSpec, LaunchSpec, ProcessSpec};
pub use window::{
    find_window, focus, launch, list_windows, open_profile, AppWindow, OpenOutcome, WindowError,
};

use std::path::Path;

mod builtin {
    include!(concat!(env!("OUT_DIR"), "/builtin_profiles.rs"));
}

/// 用户自定义应用配置所在的子目录名（相对配置目录）。
pub const USER_PROFILE_DIR: &str = "app-profiles";

/// 已加载的应用配置集合。
#[derive(Debug, Clone, Default)]
pub struct ProfileRegistry {
    profiles: Vec<AppProfile>,
}

impl ProfileRegistry {
    /// 只加载内置配置（`profiles/*.json`）。
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        for (file, content) in builtin::BUILTIN_PROFILES {
            match serde_json::from_str::<AppProfile>(content) {
                Ok(profile) => registry.profiles.push(profile),
                Err(e) => core_log::log_warn(&format!(
                    "[app-profile] 内置配置 {file} 解析失败，已跳过：{e}"
                )),
            }
        }
        registry
    }

    /// 内置配置 + 用户目录配置（同进程名时用户配置整份覆盖内置）。
    pub fn load(user_dir: Option<&Path>) -> Self {
        let mut registry = Self::builtin();
        if let Some(dir) = user_dir {
            for profile in load_dir(dir) {
                registry.upsert(profile);
            }
        }
        registry
    }

    /// 插入配置；与已有配置的进程名有交集时整份替换（用户覆盖内置）。
    pub fn upsert(&mut self, profile: AppProfile) {
        let incoming = profile.process.names();
        let existing = self
            .profiles
            .iter()
            .position(|p| p.process.names().iter().any(|n| incoming.contains(n)));
        match existing {
            Some(idx) => self.profiles[idx] = profile,
            None => self.profiles.push(profile),
        }
    }

    /// 全部配置（内置在前，被覆盖的已替换）。
    pub fn profiles(&self) -> &[AppProfile] {
        &self.profiles
    }

    /// 按前台进程名匹配配置；大小写不敏感。传完整路径也可以，只看文件名。
    pub fn match_process(&self, exe_name: &str) -> Option<&AppProfile> {
        let name = exe_name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(exe_name)
            .trim();
        self.profiles.iter().find(|p| p.process.matches(name))
    }

    /// 按「进程名 + 窗口标题」匹配配置，进程名优先。
    ///
    /// 标题兜底是为进程名认不出来的目标准备的：Chrome PWA、以及跑在 WSL 里
    /// 的服务（DSH 只有浏览器窗口是 Windows 进程）。
    pub fn match_context(&self, exe_name: &str, window_title: &str) -> Option<&AppProfile> {
        let name = exe_name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(exe_name)
            .trim();
        self.profiles
            .iter()
            .find(|p| p.process.matches(name))
            .or_else(|| self.profiles.iter().find(|p| p.matches_title(window_title)))
    }

    /// 配了图标的配置，用于快捷菜单内圈；顺序即文件里的声明顺序。
    pub fn menu_entries(&self) -> Vec<&AppProfile> {
        self.profiles
            .iter()
            .filter(|p| p.icon.as_ref().is_some_and(|i| !i.label.trim().is_empty()))
            .collect()
    }
}

/// 读取一个目录下的所有 `*.json`；坏文件只记日志、不中断。
fn load_dir(dir: &Path) -> Vec<AppProfile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<AppProfile>(&text) {
                Ok(profile) => out.push(profile),
                Err(e) => core_log::log_warn(&format!(
                    "[app-profile] 用户配置 {file} 解析失败，已跳过：{e}"
                )),
            },
            Err(e) => core_log::log_warn(&format!(
                "[app-profile] 用户配置 {file} 读取失败，已跳过：{e}"
            )),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_mapping::{ActionKind, ButtonId, KeyBinding, Trigger};

    fn profile(process: &str, name: &str) -> AppProfile {
        AppProfile {
            process: ProcessSpec::One(process.into()),
            name: name.into(),
            bindings: vec![KeyBinding {
                button: ButtonId::Ok,
                trigger: Trigger::SingleClick,
                action: ActionKind::Return,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn builtin_profiles_are_embedded_and_parse() {
        let registry = ProfileRegistry::builtin();
        // build.rs 会把仓库根 profiles/*.json 编进来；仓库里至少要有 Codex 这份。
        assert!(
            !registry.profiles().is_empty(),
            "没有加载到任何内置应用配置，检查 profiles/ 与 build.rs"
        );
        for p in registry.profiles() {
            assert!(
                !p.process.is_empty() || !p.window_title_contains.is_empty(),
                "内置配置既没有 process 也没有 window_title_contains，永远匹配不上：{}",
                p.display_name()
            );
            assert!(
                !p.display_name().is_empty(),
                "内置配置缺少展示名：{:?}",
                p.process
            );
        }
    }

    #[test]
    fn match_process_is_case_insensitive_and_accepts_paths() {
        let registry = ProfileRegistry::builtin();
        // 用内置配置里的第一个进程名反查，避免把测试绑死在某个具体应用上。
        let Some(first) = registry.profiles().first() else {
            return;
        };
        let exe = first.process.names()[0].clone();

        assert!(registry.match_process(&exe).is_some());
        assert!(registry.match_process(&exe.to_uppercase()).is_some());
        assert!(registry
            .match_process(&format!(r"C:\Apps\Whatever\{exe}"))
            .is_some());
        assert!(registry
            .match_process("definitely-not-an-app.exe")
            .is_none());
    }

    #[test]
    fn user_profile_overrides_builtin_by_process_name() {
        let mut registry = ProfileRegistry::builtin();
        let Some(existing) = registry.profiles().first().cloned() else {
            return;
        };
        let exe = existing.process.names()[0].clone();
        let before = registry.profiles().len();

        registry.upsert(profile(&exe, "用户覆盖版"));

        assert_eq!(
            registry.profiles().len(),
            before,
            "同进程名应整份替换而不是新增"
        );
        assert_eq!(
            registry.match_process(&exe).unwrap().display_name(),
            "用户覆盖版"
        );
    }

    #[test]
    fn user_dir_loads_and_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let user_dir = dir.path().join(USER_PROFILE_DIR);
        std::fs::create_dir_all(&user_dir).unwrap();

        // 新增一个内置里没有的应用
        std::fs::write(
            user_dir.join("my-app.json"),
            r#"{ "process": "MyApp.exe", "name": "我的应用",
                 "bindings": [ { "button": "Ok", "trigger": "SingleClick", "action": "Return" } ] }"#,
        )
        .unwrap();
        // 坏文件不应影响其它配置加载
        std::fs::write(user_dir.join("broken.json"), "{ not json").unwrap();
        // 非 json 文件应被忽略
        std::fs::write(user_dir.join("readme.txt"), "ignore me").unwrap();

        let registry = ProfileRegistry::load(Some(&user_dir));
        let hit = registry.match_process("MyApp.exe").expect("用户配置应命中");
        assert_eq!(hit.display_name(), "我的应用");
        assert_eq!(hit.bindings.len(), 1);
        assert!(registry.match_process("broken.json").is_none());
    }

    #[test]
    fn missing_user_dir_is_not_an_error() {
        let registry = ProfileRegistry::load(Some(Path::new(r"Z:\nope\nothing\here")));
        assert_eq!(
            registry.profiles().len(),
            ProfileRegistry::builtin().profiles().len()
        );
    }

    #[test]
    fn match_context_falls_back_to_window_title() {
        let mut registry = ProfileRegistry::default();
        let mut dsh = profile("__not_a_real_process__.exe", "DeepSeek Harness");
        dsh.window_title_contains = vec!["DeepSeek Harness".into()];
        registry.upsert(dsh);

        // 进程名认不出来，靠标题命中（WSL 里的服务就是这个情形）。
        assert_eq!(
            registry
                .match_context("chrome.exe", "打招呼 · DeepSeek Harness")
                .map(|p| p.display_name()),
            Some("DeepSeek Harness".to_string())
        );
        // 标题不匹配就不该命中。
        assert!(registry.match_context("chrome.exe", "别的标签页").is_none());
        // 空标题不能命中任何配置。
        assert!(registry.match_context("chrome.exe", "  ").is_none());
    }

    #[test]
    fn match_context_prefers_process_over_title() {
        let mut registry = ProfileRegistry::default();
        let mut by_process = profile("Editor.exe", "编辑器");
        by_process.window_title_contains = vec!["不该赢".into()];
        registry.upsert(by_process);
        let mut by_title = profile("__not_a_real_process__.exe", "标题命中");
        by_title.window_title_contains = vec!["编辑器窗口".into()];
        registry.upsert(by_title);

        // 进程名和标题同时命中不同配置时，进程名优先。
        assert_eq!(
            registry
                .match_context("Editor.exe", "编辑器窗口")
                .map(|p| p.display_name()),
            Some("编辑器".to_string())
        );
    }

    #[test]
    fn menu_entries_skip_profiles_without_icon() {
        let mut registry = ProfileRegistry::default();
        registry.upsert(profile("A.exe", "无图标"));
        let mut with_icon = profile("B.exe", "有图标");
        with_icon.icon = Some(IconSpec {
            label: "B".into(),
            color: "#3B82F6".into(),
        });
        registry.upsert(with_icon);
        let mut blank = profile("C.exe", "空图标");
        blank.icon = Some(IconSpec {
            label: "   ".into(),
            color: "#000".into(),
        });
        registry.upsert(blank);

        let entries = registry.menu_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].display_name(), "有图标");
    }
}
