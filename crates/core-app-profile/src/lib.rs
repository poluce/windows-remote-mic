//! core-app-profile — 按前台应用切换按键配置（**一个应用一个文件**）。
//!
//! # 这个 crate 负责什么
//!
//! 三件事：
//! 1. 把编译进来的**种子**在首次运行时落地成文件（[`seed_user_dir`]）；
//! 2. 加载并校验「一个应用一个文件」的配置；
//! 3. 按前台窗口的进程名匹配到某份配置。
//!
//! 它**不**发按键、**不**依赖 `core-dispatch` / `core-input` / Tauri，
//! 也不决定「怎么覆盖映射」——那是上层的事。上层拿
//! [`ProfileRegistry::match_process`] 的结果去覆盖即可。
//!
//! # 配置来源：只有文件
//!
//! 唯一的来源是 `<配置目录>/app-profiles/*.json`（见 [`USER_PROFILE_DIR`]）。
//! 仓库根 `profiles/*.json` 由 `build.rs` 编进二进制，但角色只是**种子**：
//! [`seed_user_dir`] 在首次运行时把还没有同名文件的种子写进去，之后**磁盘上的
//! 文件就是唯一事实来源，程序不再覆盖它**。
//!
//! 为什么不继续做「内置 + 用户」两层合并：那样一来「删除一份应用配置」是删不掉
//! 的——种子每次启动都从二进制里重新展开，用户删掉的那份下一轮自己就回来了。
//! 界面上「除了全局都能删」这条规则，只有在配置真的躺在磁盘上时才成立。
//!
//! 代价是种子内容在编译期冻结：改了 `profiles/*.json`，只对还没落地过该 id 的
//! 机器生效。想恢复出厂，删掉 `app-profiles/` 目录与种子清单即可。
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

use std::collections::BTreeSet;
use std::path::Path;

mod seeds {
    include!(concat!(env!("OUT_DIR"), "/seed_profiles.rs"));
}

/// 应用配置所在的子目录名（相对配置目录）：**一个应用一个文件**。
pub const USER_PROFILE_DIR: &str = "app-profiles";

/// 种子清单的文件名（相对配置目录），记「哪些种子已经落地过了」。
///
/// 它必须放在 `app-profiles/` **外面**：那个目录的规矩是「一个应用一个文件」，
/// 而 [`load_dir`] 只认 `*.json`，清单混进去要么被当成配置解析、要么得给它加
/// 一条排除规则。
pub const SEED_MANIFEST: &str = "app-profiles.seeded.json";

/// 编译进来的种子：(id, JSON 文本)。id 就是落地后的文件名（不含 `.json`）。
pub fn seed_templates() -> Vec<(String, &'static str)> {
    seeds::SEED_PROFILES
        .iter()
        .map(|(file, content)| {
            let id = file.strip_suffix(".json").unwrap_or(file).to_string();
            (id, *content)
        })
        .collect()
}

/// 把还没落地过的种子写进 `<配置目录>/app-profiles/`，返回本次新写了几份。
///
/// 每条种子按顺序判断：
/// 1. 已记在 [`SEED_MANIFEST`] 里 → 跳过（**用户删掉的那份不会复活**）；
/// 2. `app-profiles/<id>.json` 已存在 → 只补记清单，**不覆盖**（保护手改过的文件）；
/// 3. 否则写文件并记进清单。
///
/// 于是：以后版本新增的种子不在清单里，会自动落地；已经被删掉的因为还在清单里，
/// 永远不会回来。清单本身丢了也不要紧——只要文件还在，第 2 条会把它重新记上。
pub fn seed_user_dir(config_dir: &Path) -> usize {
    let templates = seed_templates();
    if templates.is_empty() {
        return 0;
    }

    let dir = config_dir.join(USER_PROFILE_DIR);
    let manifest_path = config_dir.join(SEED_MANIFEST);
    let mut settled = read_seed_manifest(&manifest_path);
    let before = settled.len();
    let mut written = 0usize;

    for (id, content) in templates {
        if settled.contains(&id) {
            continue;
        }
        let dest = dir.join(format!("{id}.json"));
        if dest.exists() {
            settled.insert(id);
            continue;
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            core_log::log_warn(&format!(
                "[app-profile] 建不出配置目录 {}：{e}",
                dir.display()
            ));
            break;
        }
        match std::fs::write(&dest, content) {
            Ok(()) => {
                core_log::log_info(&format!("[app-profile] 已落地种子配置 {id}.json"));
                settled.insert(id);
                written += 1;
            }
            // 写失败就不记进清单，下次启动再试一次。
            Err(e) => core_log::log_warn(&format!("[app-profile] 种子 {id} 写入失败：{e}")),
        }
    }

    if settled.len() != before {
        write_seed_manifest(&manifest_path, &settled);
    }
    written
}

/// 读种子清单。文件不存在或内容坏掉时当成空清单——已有文件会被第 2 条重新记上。
fn read_seed_manifest(path: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_seed_manifest(path: &Path, ids: &BTreeSet<String>) {
    let text = match serde_json::to_string_pretty(ids) {
        Ok(text) => text,
        Err(e) => {
            core_log::log_warn(&format!("[app-profile] 种子清单序列化失败：{e}"));
            return;
        }
    };
    if let Err(e) = std::fs::write(path, text) {
        core_log::log_warn(&format!("[app-profile] 种子清单写入失败：{e}"));
    }
}

/// 已加载的应用配置集合。
#[derive(Debug, Clone, Default)]
pub struct ProfileRegistry {
    profiles: Vec<AppProfile>,
}

impl ProfileRegistry {
    /// 解析编译进来的种子，**不落盘**。
    ///
    /// 正常的加载路径不经过这里：种子先由 [`seed_user_dir`] 落地成文件，再走
    /// [`load`](Self::load) 读回来。这个入口留给播种逻辑的自检与测试——需要在
    /// 没有配置目录的环境里拿一份真实的配置集合时用它。
    pub fn from_seeds() -> Self {
        let mut registry = Self::default();
        for (id, content) in seed_templates() {
            match parse_profile(&id, content) {
                Ok(profile) => registry.upsert(profile),
                Err(e) => core_log::log_warn(&format!(
                    "[app-profile] 种子 {id}.json 解析失败，已跳过：{e}"
                )),
            }
        }
        registry
    }

    /// 加载用户目录下的全部配置。
    ///
    /// 目录不存在（还没播种过，或用户把配置删光了）就是空注册表，不是错误。
    pub fn load(user_dir: Option<&Path>) -> Self {
        let mut registry = Self::default();
        if let Some(dir) = user_dir {
            for profile in load_dir(dir) {
                registry.upsert(profile);
            }
        }
        registry
    }

    /// 插入配置；与已有配置抢同一批窗口时，后者整份替换前者。
    ///
    /// 判重用 [`AppProfile::conflicts_with`]，进程名和标题关键字都算。这不是
    /// 「用户覆盖内置」那条老路了（内置已经不存在），而是防止两份用户文件互相
    /// 遮蔽：匹配用的是 `.find()`，真撞上时只有先加载的那份生效，另一份会表现成
    /// 「改了没反应」，所以至少要留一条日志。
    pub fn upsert(&mut self, profile: AppProfile) {
        match self
            .profiles
            .iter()
            .position(|p| p.conflicts_with(&profile))
        {
            Some(idx) => {
                if self.profiles[idx].id != profile.id {
                    core_log::log_warn(&format!(
                        "[app-profile] 「{}」与「{}」会匹配同一批窗口，后者生效",
                        self.profiles[idx].display_name(),
                        profile.display_name()
                    ));
                }
                self.profiles[idx] = profile;
            }
            None => self.profiles.push(profile),
        }
    }

    /// 全部配置，顺序即目录里的加载顺序。
    pub fn profiles(&self) -> &[AppProfile] {
        &self.profiles
    }

    /// 按 id（文件名）取配置。
    ///
    /// 保存 / 删除 / 快捷菜单「点图标」都用它定位目标。**不要改用展示名**：
    /// `name` 在界面上可改，改了之后所有按名字找的地方都会落空。
    pub fn get_by_id(&self, id: &str) -> Option<&AppProfile> {
        self.profiles.iter().find(|p| p.id == id)
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
///
/// **按文件名排序后加载**：`read_dir` 的顺序在 Windows 上是不保证的，不排的话
/// 作用范围胶囊和矩阵的排列每次启动都可能不一样。
fn load_dir(dir: &Path) -> Vec<AppProfile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        // id 就是文件名去掉 `.json`；展示名另存在 JSON 的 `name` 里。
        let id = file.strip_suffix(".json").unwrap_or(&file).to_string();
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_profile(&id, &text) {
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

/// 解析一份配置并挂上 id。
///
/// JSON 里没有 `id` 字段（`serde(skip)`），身份完全由文件名决定，所以只能在这里补。
fn parse_profile(id: &str, text: &str) -> Result<AppProfile, serde_json::Error> {
    let mut profile: AppProfile = serde_json::from_str(text)?;
    profile.id = id.to_string();
    Ok(profile)
}

/// 把一份配置写回 `<配置目录>/app-profiles/<id>.json`。
///
/// **原子写入**：先写同目录的临时文件再改名。编辑器是「读出来 → 改一个按键 →
/// 整份写回去」，直接往目标文件写的话，中途失败会留下一份残缺 JSON，而坏文件在
/// [`load_dir`] 里是**整份跳过**的——用户会一次性丢掉这个应用的所有配置。
/// 改名是原子的，要么是旧的完整内容，要么是新的完整内容。
///
/// 临时文件用 `.json.tmp` 后缀：`load_dir` 只认扩展名正好是 `json` 的文件，
/// 不会把它当成一份配置读进来。
pub fn save_profile(config_dir: &Path, profile: &AppProfile) -> std::io::Result<()> {
    if !is_valid_id(&profile.id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("配置 id 非法：{:?}", profile.id),
        ));
    }
    let dir = config_dir.join(USER_PROFILE_DIR);
    std::fs::create_dir_all(&dir)?;

    let text = serde_json::to_string_pretty(profile)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let dest = dir.join(format!("{}.json", profile.id));
    let tmp = dir.join(format!("{}.json.tmp", profile.id));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &dest)
}

/// 删掉一份配置。文件本来就不在也算成功——用户要的结果是「它没了」。
pub fn remove_profile(config_dir: &Path, id: &str) -> std::io::Result<()> {
    if !is_valid_id(id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("配置 id 非法：{id:?}"),
        ));
    }
    let dest = config_dir.join(USER_PROFILE_DIR).join(format!("{id}.json"));
    match std::fs::remove_file(&dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// 给一份新配置取 id（即文件名）。撞了就加 `-1`、`-2` 后缀。
///
/// id 一律 ASCII：从进程名 slug 化（`ZCode.exe` → `zcode`），进程名为空的纯标题
/// 配置退化成 `app`、`app-1`……**中文展示名只存在 JSON 的 `name` 字段里**——
/// 文件名要进日志、进路径拼接、以后还可能进命令行，带中文迟早出事。
pub fn new_profile_id(config_dir: &Path, process: &str) -> String {
    let dir = config_dir.join(USER_PROFILE_DIR);
    let base = slug(process);
    for n in 1..1000 {
        let id = if n == 1 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        if !dir.join(format!("{id}.json")).exists() {
            return id;
        }
    }
    // 一千份同进程名的配置…… 不至于，但不能返回一个会互相覆盖的 id。
    format!("{base}-{}", std::process::id())
}

/// id 必须是安全的文件名：只允许 ASCII 字母数字与 `-` `_`。
///
/// 这条同时挡住了路径穿越（`..`、`/`、`\`）——id 会参与拼路径，不能是任意字符串。
fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 把进程名压成 id 用的 ASCII slug。
fn slug(process: &str) -> String {
    // 可能传来完整路径，也可能是一串候选（`"A.exe, B.exe"`）：
    // 先取**最后一个**路径段（路径里最后一段才是可执行文件名），再取**第一个**候选。
    let last_segment = process
        .split(['/', '\\'])
        .map(str::trim)
        .rfind(|s| !s.is_empty())
        .unwrap_or("");
    let first = last_segment
        .split(',')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("");
    let lowered = first.to_ascii_lowercase();
    let stem = lowered.strip_suffix(".exe").unwrap_or(&lowered);

    let mut out = String::new();
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "app".to_string()
    } else {
        out
    }
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
    fn seed_profiles_are_embedded_and_parse() {
        let registry = ProfileRegistry::from_seeds();
        // build.rs 会把仓库根 profiles/*.json 编进来；仓库里至少要有 Codex 这份。
        assert!(
            !registry.profiles().is_empty(),
            "没有编译进任何应用配置种子，检查 profiles/ 与 build.rs"
        );
        for p in registry.profiles() {
            assert!(
                !p.process.is_empty() || !p.window_title_contains.is_empty(),
                "种子既没有 process 也没有 window_title_contains，永远匹配不上：{}",
                p.display_name()
            );
            assert!(
                !p.display_name().is_empty(),
                "种子缺少展示名：{:?}",
                p.process
            );
            assert!(!p.id.is_empty(), "种子没有 id：{}", p.display_name());
        }
    }

    #[test]
    fn match_process_is_case_insensitive_and_accepts_paths() {
        let registry = ProfileRegistry::from_seeds();
        // 用种子里第一个**带进程名**的配置反查，避免绑死在某个具体应用上，
        // 也避免碰上纯标题配置（它的 process 是空的）。
        let Some(first) = registry.profiles().iter().find(|p| !p.process.is_empty()) else {
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
    fn upsert_replaces_profile_matching_the_same_process() {
        let mut registry = ProfileRegistry::default();
        registry.upsert(profile("MyApp.exe", "第一版"));
        registry.upsert(profile("MyApp.exe", "第二版"));

        assert_eq!(registry.profiles().len(), 1, "同进程名应整份替换而不是新增");
        assert_eq!(
            registry.match_process("MyApp.exe").unwrap().display_name(),
            "第二版"
        );
    }

    #[test]
    fn upsert_replaces_title_only_profile_matching_the_same_title() {
        // 修复前这里会留下两份：判重只看进程名，而纯标题配置的 process 是空的，
        // 谁都撞不上。匹配用的是 .find()，于是后加的那份永远不生效——
        // 表现出来就是「界面里改了，实际没反应」。
        let mut first = profile("", "DeepSeek Harness");
        first.window_title_contains = vec!["DeepSeek Harness".into()];
        let mut second = profile("", "DSH 改过的");
        second.window_title_contains = vec!["deepseek harness".into()]; // 大小写不同也算同一批窗口

        let mut registry = ProfileRegistry::default();
        registry.upsert(first);
        registry.upsert(second);

        assert_eq!(registry.profiles().len(), 1, "同标题应整份替换");
        assert_eq!(
            registry
                .match_context("chrome.exe", "打招呼 · DeepSeek Harness")
                .unwrap()
                .display_name(),
            "DSH 改过的"
        );
    }

    #[test]
    fn id_tracks_the_file_name_and_stays_out_of_json() {
        let dir = tempfile::tempdir().unwrap();
        let user_dir = dir.path().join(USER_PROFILE_DIR);
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(
            user_dir.join("my-app.json"),
            r#"{ "process": "MyApp.exe", "name": "我的应用" }"#,
        )
        .unwrap();

        let registry = ProfileRegistry::load(Some(&user_dir));
        let hit = registry.get_by_id("my-app").expect("id 应取自文件名");
        assert_eq!(hit.display_name(), "我的应用");
        assert!(registry.get_by_id("我的应用").is_none(), "展示名不是身份");

        // 身份不能跟着配置写进文件——它是文件名给的。
        let text = serde_json::to_string(hit).unwrap();
        assert!(!text.contains("\"id\""), "id 不该被序列化：{text}");
    }

    #[test]
    fn user_dir_loads_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let user_dir = dir.path().join(USER_PROFILE_DIR);
        std::fs::create_dir_all(&user_dir).unwrap();

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
        assert_eq!(registry.profiles().len(), 1, "坏文件和 txt 都不该进来");
        let hit = registry.match_process("MyApp.exe").expect("用户配置应命中");
        assert_eq!(hit.display_name(), "我的应用");
        assert_eq!(hit.bindings.len(), 1);
        assert_eq!(hit.id, "my-app");
    }

    #[test]
    fn missing_user_dir_is_not_an_error() {
        let registry = ProfileRegistry::load(Some(Path::new(r"Z:\nope\nothing\here")));
        assert!(
            registry.profiles().is_empty(),
            "目录不存在（还没播种，或用户把配置删光了）就是空注册表"
        );
    }

    #[test]
    fn seed_user_dir_materializes_seeds_once() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let written = seed_user_dir(config_dir);
        assert_eq!(written, seed_templates().len(), "首次运行应把所有种子落地");
        assert!(config_dir.join(USER_PROFILE_DIR).is_dir());
        assert!(config_dir.join(SEED_MANIFEST).is_file());

        // 落地的文件能被正常加载，且 id 就是文件名。
        let user_dir = config_dir.join(USER_PROFILE_DIR);
        let registry = ProfileRegistry::load(Some(&user_dir));
        assert_eq!(registry.profiles().len(), seed_templates().len());
        for (id, _) in seed_templates() {
            assert!(registry.get_by_id(&id).is_some(), "缺少 {id}");
        }

        // 第二次运行不该重复写。
        assert_eq!(seed_user_dir(config_dir), 0, "已落地的种子不该再写一遍");
    }

    #[test]
    fn seed_user_dir_never_resurrects_a_deleted_profile() {
        // 这条是整套种子机制存在的理由：用户删掉的配置不能在下次启动时自己回来。
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();
        seed_user_dir(config_dir);

        let Some((id, _)) = seed_templates().first().cloned() else {
            return;
        };
        let file = config_dir.join(USER_PROFILE_DIR).join(format!("{id}.json"));
        assert!(file.exists());
        std::fs::remove_file(&file).unwrap();

        // 模拟下次启动：清单里还记着这个 id，所以不会把它写回来。
        assert_eq!(seed_user_dir(config_dir), 0);
        assert!(!file.exists(), "删掉的配置不该在下次启动时复活");
        assert!(
            ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
                .get_by_id(&id)
                .is_none()
        );
    }

    #[test]
    fn seed_user_dir_does_not_overwrite_an_edited_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();
        let Some((id, _)) = seed_templates().first().cloned() else {
            return;
        };
        let user_dir = config_dir.join(USER_PROFILE_DIR);
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(
            user_dir.join(format!("{id}.json")),
            r#"{ "process": "HandEdited.exe", "name": "手改过的" }"#,
        )
        .unwrap();

        seed_user_dir(config_dir);

        let registry = ProfileRegistry::load(Some(&user_dir));
        assert_eq!(
            registry.get_by_id(&id).unwrap().display_name(),
            "手改过的",
            "播种不能盖掉用户手改过的文件"
        );
    }

    #[test]
    fn seed_user_dir_picks_up_seeds_added_later() {
        // 模拟「以后版本新增了一个种子」：清单里只记着旧的，新 id 不在其中。
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();
        let templates = seed_templates();
        if templates.len() < 2 {
            return;
        }
        let (first, second) = (templates[0].0.clone(), templates[1].0.clone());

        let user_dir = config_dir.join(USER_PROFILE_DIR);
        std::fs::create_dir_all(&user_dir).unwrap();
        std::fs::write(config_dir.join(SEED_MANIFEST), format!(r#"["{first}"]"#)).unwrap();
        std::fs::write(user_dir.join(format!("{first}.json")), "{}").unwrap();

        seed_user_dir(config_dir);

        assert!(
            user_dir.join(format!("{second}.json")).exists(),
            "清单里没有的新种子应该自动落地"
        );
        let manifest = std::fs::read_to_string(config_dir.join(SEED_MANIFEST)).unwrap();
        assert!(
            manifest.contains(&first) && manifest.contains(&second),
            "两个 id 都该记进清单：{manifest}"
        );
    }

    #[test]
    fn seed_user_dir_survives_a_lost_manifest() {
        // 清单丢了不该导致数据丢失：文件还在就只补记，不重写。
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();
        seed_user_dir(config_dir);

        let Some((id, _)) = seed_templates().first().cloned() else {
            return;
        };
        let file = config_dir.join(USER_PROFILE_DIR).join(format!("{id}.json"));
        std::fs::write(&file, r#"{ "process": "Mine.exe", "name": "我的" }"#).unwrap();
        std::fs::remove_file(config_dir.join(SEED_MANIFEST)).unwrap();

        seed_user_dir(config_dir);

        assert_eq!(
            ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
                .get_by_id(&id)
                .unwrap()
                .display_name(),
            "我的",
            "清单丢了也不能覆盖已存在的文件"
        );
        assert!(config_dir.join(SEED_MANIFEST).is_file(), "清单应被补回来");
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

    #[test]
    fn slug_is_ascii_and_handles_paths_and_lists() {
        assert_eq!(slug("ZCode.exe"), "zcode");
        assert_eq!(slug("  zcode.EXE  "), "zcode");
        assert_eq!(slug(r"C:\Program Files\ZCode\ZCode.exe"), "zcode");
        assert_eq!(slug("My App.exe"), "my-app");
        assert_eq!(slug("A.exe, B.exe"), "a");
        // 中文进程名不能原样当文件名：字符全被丢掉，退化成安全兜底值。
        assert_eq!(slug("我的助手.exe"), "app");
        assert_eq!(slug(""), "app");
        assert_eq!(slug("   "), "app");
    }

    #[test]
    fn new_profile_id_avoids_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let first = new_profile_id(config_dir, "ZCode.exe");
        assert_eq!(first, "zcode");

        let mut p = profile("ZCode.exe", "ZCode");
        p.id = first.clone();
        save_profile(config_dir, &p).unwrap();

        let second = new_profile_id(config_dir, "ZCode.exe");
        assert_eq!(second, "zcode-2", "撞名要加后缀，不能覆盖已有的那份");

        // 没有进程名的纯标题配置退化成 app / app-1……
        assert_eq!(new_profile_id(config_dir, ""), "app");
    }

    #[test]
    fn save_then_load_round_trips_and_keeps_id_out_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let mut p = profile("MyApp.exe", "我的应用");
        p.id = "my-app".into();
        p.note = "备注".into();
        save_profile(config_dir, &p).unwrap();

        let raw =
            std::fs::read_to_string(config_dir.join(USER_PROFILE_DIR).join("my-app.json")).unwrap();
        assert!(!raw.contains("\"id\""), "id 不该出现在文件里：{raw}");

        let registry = ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)));
        let back = registry.get_by_id("my-app").expect("应能读回来");
        assert_eq!(back.display_name(), "我的应用");
        assert_eq!(back.bindings.len(), 1);

        // 原子写入不该留下临时文件被当成配置读进来。
        assert_eq!(registry.profiles().len(), 1);
        assert!(!config_dir
            .join(USER_PROFILE_DIR)
            .join("my-app.json.tmp")
            .exists());
    }

    #[test]
    fn save_profile_rejects_unsafe_ids() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["../escape", r"..\escape", "a/b", "", "有中文"] {
            let mut p = profile("X.exe", "X");
            p.id = bad.into();
            assert!(
                save_profile(dir.path(), &p).is_err(),
                "{bad:?} 不该被当成合法 id"
            );
        }
        assert!(remove_profile(dir.path(), "../../config").is_err());
    }

    #[test]
    fn remove_profile_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();

        let mut p = profile("MyApp.exe", "我的应用");
        p.id = "my-app".into();
        save_profile(config_dir, &p).unwrap();
        assert!(
            ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
                .get_by_id("my-app")
                .is_some()
        );

        remove_profile(config_dir, "my-app").unwrap();
        assert!(
            ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
                .get_by_id("my-app")
                .is_none()
        );

        // 再删一次不该报错——用户要的结果是「它没了」，而它确实没了。
        remove_profile(config_dir, "my-app").unwrap();
    }

    #[test]
    fn removing_a_seeded_id_does_not_bring_it_back() {
        // 编辑器和种子机制的交界处：删掉一份落地过的种子配置，重启不能复活它。
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path();
        seed_user_dir(config_dir);

        let Some((id, _)) = seed_templates().first().cloned() else {
            return;
        };
        remove_profile(config_dir, &id).unwrap();

        assert_eq!(seed_user_dir(config_dir), 0, "删掉的种子不该被重新落地");
        assert!(
            ProfileRegistry::load(Some(&config_dir.join(USER_PROFILE_DIR)))
                .get_by_id(&id)
                .is_none()
        );
    }
}
