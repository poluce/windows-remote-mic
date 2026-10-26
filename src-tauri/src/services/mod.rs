//! 应用配置的 service 层：UI（Tauri command）与 MCP（agent_api）共用。
//!
//! 所有写操作都走 `ConfigStore::update`（进程内全局写锁 + revision 自增），
//! 写盘成功后再热更新调度器，避免 UI 与智能体并发时互相覆盖。

pub mod mapping;
