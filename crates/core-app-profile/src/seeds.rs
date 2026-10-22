//! 种子模板与首次落地管理。
//!
//! 种子来自仓库根目录的 `profiles/*.json`，在编译期生成并嵌入二进制；
//! 首次启动时写进 `<配置目录>/app-profiles/`，随后由用户完全掌控。

use std::collections::BTreeSet;
use std::path::Path;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/seed_profiles.rs"));
}

/// 应用配置所在的子目录名（相对配置目录）：**一个应用一个文件**。
pub const USER_PROFILE_DIR: &str = "app-profiles";

/// 种子清单的文件名（相对配置目录），记「哪些种子已经落地过了」。
///
/// 它必须放在 `app-profiles/` **外面**：那个目录的规矩是「一个应用一个文件」，
/// 而 [`load_dir`](crate::store::load_dir) 只认 `*.json`，清单混进去要么被当成配置解析、要么得给它加
/// 一条排除规则。
pub const SEED_MANIFEST: &str = "app-profiles.seeded.json";

/// 编译进来的种子：(id, JSON 文本)。id 就是落地后的文件名（不含 `.json`）。
pub fn seed_templates() -> Vec<(String, &'static str)> {
    embedded::SEED_PROFILES
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
