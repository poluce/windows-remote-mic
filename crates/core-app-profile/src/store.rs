//! 磁盘读写与文件名规范：原子写入、安全 id 生成、目录扫描。

use std::path::Path;

use super::model::AppProfile;
use super::seeds::USER_PROFILE_DIR;

/// 读取一个目录下的所有 `*.json`；坏文件只记日志、不中断。
///
/// **按文件名排序后加载**：`read_dir` 的顺序在 Windows 上是不保证的，不排的话
/// 作用范围胶囊和矩阵的排列每次启动都可能不一样。
pub(crate) fn load_dir(dir: &Path) -> Vec<AppProfile> {
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
pub(crate) fn parse_profile(id: &str, text: &str) -> Result<AppProfile, serde_json::Error> {
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
pub(crate) fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 把进程名压成 id 用的 ASCII slug。
pub(crate) fn slug(process: &str) -> String {
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
