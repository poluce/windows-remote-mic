//! Tauri 外壳的通用支撑模块：路径 / 状态 / 按键兜底 / panic / 映射编解码。
//!
//! `lib.rs` 只保留应用装配（命令、窗口、调度器、生命周期），
//! 这些可复用、可单独阅读的职责放到 `support/` 下。

pub(crate) mod codec;
pub(crate) mod keys;
pub(crate) mod panic;
pub(crate) mod paths;
pub(crate) mod state;
