//! 把仓库根目录 `profiles/*.json` 编进二进制。
//!
//! 一个应用一个文件：新增应用只需往 `profiles/` 放一个 JSON，
//! 不需要改任何 Rust 代码（build.rs 会自动发现并 include）。
//!
//! 用户目录下的同名进程配置在运行时会覆盖内置的（见 `ProfileRegistry::load`）。

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
                "cargo:warning=core-app-profile: 读不到 {}（{e}），本次不会内置任何应用配置",
                profiles_dir.display()
            );
            Vec::new()
        }
    };
    files.sort();

    let mut generated = String::from(
        "// 由 build.rs 生成，请勿手改。\n\
         /// 内置应用配置：(文件名, 文件内容)。\n\
         pub const BUILTIN_PROFILES: &[(&str, &str)] = &[\n",
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

    let dest = Path::new(&out_dir).join("builtin_profiles.rs");
    fs::write(&dest, generated).expect("write builtin_profiles.rs");
}
