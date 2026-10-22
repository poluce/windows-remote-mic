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
mod registry;
mod seeds;
mod store;
mod window;

#[cfg(test)]
mod tests;

pub use foreground::{file_name, foreground_process_name, foreground_window_title};
pub use model::{AppProfile, IconSpec, LaunchSpec, ProcessSpec};
pub use registry::ProfileRegistry;
pub use seeds::{seed_templates, seed_user_dir, SEED_MANIFEST, USER_PROFILE_DIR};
pub use store::{new_profile_id, remove_profile, save_profile};
pub use window::{
    find_window, focus, launch, list_windows, open_profile, AppWindow, OpenOutcome, WindowError,
};
