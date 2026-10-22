//! Tauri 外壳共享状态（调度器）与配置目录/应用档案加载。

use std::sync::Arc;

use core_dispatch::KeyDispatcher;

/// 共享给 Tauri 命令的运行时状态：按键调度器。
pub struct AppState {
    pub dispatcher: Arc<KeyDispatcher>,
}
