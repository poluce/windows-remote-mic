//! 把仓库根目录 `profiles/*.json` 编进二进制，作为**首次运行的种子**。
//!
//! 一个应用一个文件：新增应用只需往 `profiles/` 放一个 JSON，
//! 不需要改任何 Rust 代码（build.rs 会自动发现并 include）。
//!
//! 种子只在 `app-profiles/` 里还没有同名文件时落地一次，之后磁盘上的文件就是
//! 唯一事实来源，程序不再覆盖它（见 `core_app_profile::seed_user_dir`）。
//!
//! **代价**：种子内容在这个阶段被冻结。以后改了这里某个 JSON，只对还没落地过
//! 该 id 的机器生效；已落地过的用户要自己删掉文件才会换新的。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let profiles_dir = Path::new(&manifest_dir)
        .join("..")
        .join("..")
        .join("profiles");
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");

    println!("cargo:rerun-if-changed={}", profiles_dir.display());

    let mut files: Vec<PathBuf> = match fs::read_dir(&profiles_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect(),
        Err(e) => {
            println!(
                "cargo:warning=core-app-profile: 读不到 {}（{e}），本次不会编译进任何应用配置种子",
                profiles_dir.display()
            );
            Vec::new()
        }
    };
    files.sort();

    let mut generated = String::from(
        "// 由 build.rs 生成，请勿手改。\n\
         /// 应用配置种子：(文件名, 文件内容)。\n\
         pub const SEED_PROFILES: &[(&str, &str)] = &[\n",
    );
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let absolute = path.canonicalize().unwrap_or_else(|_| path.clone());
        let literal = absolute.display().to_string().replace('\\', "\\\\");
        generated.push_str(&format!(
            "    (\"{file_name}\", include_str!(\"{literal}\")),\n"
        ));
    }
    generated.push_str("];\n");

    let dest = Path::new(&out_dir).join("seed_profiles.rs");
    fs::write(&dest, generated).expect("write seed_profiles.rs");
}
