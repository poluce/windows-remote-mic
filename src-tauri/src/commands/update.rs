//! 自动更新的持久化状态。
//!
//! 更新时间戳放在 `config.json` 里而不是前端 localStorage：重启后仍要遵守
//! 「自动检查距上次成功检查 ≥1 小时才联网」的门槛，且配置持久化只有一个出口
//! （`core-config` 的原子写入 + .bak 回退）。

use crate::config_store;

/// 上次成功检查更新的 Unix 秒时间戳；0 表示还没成功检查过。
#[tauri::command]
pub fn get_last_update_check_at() -> u64 {
    config_store()
        .map(|store| store.load_or_default().last_update_check_at)
        .unwrap_or(0)
}

/// 记录一次成功的更新检查结果（成功 = 请求返回并解析出「有更新 / 无更新」）。
///
/// 用 `load()` 而不是 `load_or_default()`：配置真的损坏时宁可不写，也不要拿默认
/// 配置覆盖掉用户还能从 `.bak` 恢复的数据。
#[tauri::command]
pub fn set_last_update_check_at(timestamp: u64) -> Result<(), String> {
    let store = config_store().ok_or_else(|| "无法创建配置目录".to_string())?;
    let mut cfg = store.load().map_err(|e| e.to_string())?;
    cfg.last_update_check_at = timestamp;
    store.save(&cfg).map_err(|e| e.to_string())
}
