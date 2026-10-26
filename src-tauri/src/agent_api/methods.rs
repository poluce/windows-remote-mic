//! 内部 JSON-RPC 方法 → service 层。
//!
//! 错误码只在本地 RPC 内部使用；MCP sidecar 会把它映射为 tool result 的
//! `isError: true` + 字符串 error.code。

use core_dispatch::KeyDispatcher;
use core_mapping::KeyBinding;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::{
    action_key, action_label, button_key, config_store, services, trigger_key, MappingEdit,
};

pub struct RpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn validation(message: impl Into<String>) -> Self {
        Self::new(-32003, message)
    }

    fn state(message: impl Into<String>) -> Self {
        Self::new(-32006, message)
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self::new(-32005, message)
    }
}

pub fn dispatch(
    method: &str,
    params: Value,
    app: &AppHandle,
    dispatcher: &KeyDispatcher,
) -> Result<Value, RpcError> {
    match method {
        "agent.health" => Ok(json!({
            "ok": true,
            "api_version": crate::agent_api::API_VERSION,
            "app_version": env!("CARGO_PKG_VERSION"),
            "revision": services::mapping::revision(),
            "pid": std::process::id(),
        })),
        "agent.capabilities" => Ok(json!({
            "api_version": crate::agent_api::API_VERSION,
            "methods": [
                "agent.health", "agent.capabilities",
                "mapping.catalog", "mapping.list", "mapping.set",
                "mapping.clear", "mapping.set_enabled",
                "timing.get", "timing.set",
                "shortcuts.list", "profiles.list",
                "voice.get", "eat.get", "runtime.status"
            ],
        })),
        "mapping.catalog" => {
            let catalog = crate::commands::mapping_catalog::catalog_for_agent();
            serde_json::to_value(catalog).map_err(|e| RpcError::new(-32007, e.to_string()))
        }
        "mapping.list" => {
            check_scope(&params)?;
            Ok(json!({
                "bindings": services::mapping::list_mappings(),
                "revision": services::mapping::revision(),
            }))
        }
        "mapping.set" => {
            check_scope(&params)?;
            check_revision(&params)?;
            let button = req_str(&params, "button")?;
            let trigger = req_str(&params, "trigger")?;
            let action = req_str(&params, "action")?;
            if !services::mapping::action_is_supported(&action) {
                return Err(RpcError::validation(
                    "动作不存在或不支持；请先调用 mapping.catalog",
                ));
            }
            let edit = MappingEdit {
                button,
                trigger,
                action,
                enabled: Some(true),
            };
            let change = services::mapping::save_mapping(&edit, dispatcher)
                .map_err(|e| RpcError::validation(e))?;
            emit_changed(app, change.revision);
            Ok(json!({
                "old": change.old,
                "new": change.new,
                "revision": change.revision,
            }))
        }
        "mapping.clear" => {
            check_scope(&params)?;
            check_revision(&params)?;
            let button = req_str(&params, "button")?;
            let trigger = req_str(&params, "trigger")?;
            let change = services::mapping::clear_mapping(&button, &trigger, dispatcher)
                .map_err(|e| RpcError::validation(e))?;
            if change.removed.is_some() {
                emit_changed(app, change.revision);
            }
            Ok(json!({
                "removed": change.removed,
                "revision": change.revision,
            }))
        }
        "mapping.set_enabled" => {
            check_scope(&params)?;
            check_revision(&params)?;
            let button = req_str(&params, "button")?;
            let trigger = req_str(&params, "trigger")?;
            let enabled = req_bool(&params, "enabled")?;
            let (entry, revision, changed) =
                services::mapping::set_mapping_enabled(&button, &trigger, enabled, dispatcher)
                    .map_err(|e| RpcError::validation(e))?;
            if changed {
                emit_changed(app, revision);
            }
            Ok(json!({ "binding": entry, "revision": revision }))
        }
        "timing.get" => {
            let (long_press_ms, double_click_ms) = services::mapping::get_trigger_timing();
            Ok(json!({
                "long_press_ms": long_press_ms,
                "double_click_ms": double_click_ms,
                "revision": services::mapping::revision(),
            }))
        }
        "timing.set" => {
            check_revision(&params)?;
            let long_press_ms = req_u64(&params, "long_press_ms")?;
            let double_click_ms = req_u64(&params, "double_click_ms")?;
            let (long_press_ms, double_click_ms, revision) =
                services::mapping::set_trigger_timing(long_press_ms, double_click_ms, dispatcher)
                    .map_err(|e| RpcError::validation(e))?;
            emit_changed(app, revision);
            Ok(json!({
                "long_press_ms": long_press_ms,
                "double_click_ms": double_click_ms,
                "revision": revision,
            }))
        }
        "shortcuts.list" => Ok(json!({
            "shortcuts": services::mapping::list_shortcuts(),
            "revision": services::mapping::revision(),
        })),
        "profiles.list" => Ok(json!({
            "profiles": dispatcher
                .profiles_snapshot()
                .iter()
                .map(profile_json)
                .collect::<Vec<_>>(),
        })),
        "voice.get" => Ok(json!({
            "voice_target": config_store()
                .and_then(|s| s.load().ok())
                .map(|cfg| cfg.voice_target.key().to_string())
                .unwrap_or_else(|| "windows_voice".into()),
        })),
        "eat.get" => Ok(json!({
            "eat_mode": config_store()
                .and_then(|s| s.load().ok())
                .map(|cfg| cfg.hid_tap_eat)
                .unwrap_or(true),
        })),
        "runtime.status" => {
            let status = crate::commands::connection::status::get_runtime_status();
            serde_json::to_value(status).map_err(|e| RpcError::new(-32007, e.to_string()))
        }
        _ => Err(RpcError::new(-32601, format!("未知方法：{method}"))),
    }
}

fn emit_changed(app: &AppHandle, revision: u64) {
    let _ = app.emit(
        "config-changed",
        json!({ "source": "agent", "revision": revision }),
    );
}

fn check_scope(params: &Value) -> Result<(), RpcError> {
    if let Some(scope) = params.get("scope").and_then(Value::as_str) {
        if scope != "global" {
            return Err(RpcError::state(
                "当前 MVP 只支持全局作用域；应用档案写入在下一阶段",
            ));
        }
    }
    Ok(())
}

fn check_revision(params: &Value) -> Result<(), RpcError> {
    if let Some(expected) = params.get("if_revision").and_then(Value::as_u64) {
        if expected != services::mapping::revision() {
            return Err(RpcError::conflict("revision 冲突，请重新读取后再试"));
        }
    }
    Ok(())
}

fn req_str(params: &Value, key: &str) -> Result<String, RpcError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| RpcError::validation(format!("缺少参数 {key}")))
}

fn req_bool(params: &Value, key: &str) -> Result<bool, RpcError> {
    params
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| RpcError::validation(format!("缺少布尔参数 {key}")))
}

fn req_u64(params: &Value, key: &str) -> Result<u64, RpcError> {
    params
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| RpcError::validation(format!("缺少整数参数 {key}")))
}

fn profile_json(profile: &core_app_profile::AppProfile) -> Value {
    json!({
        "id": profile.id,
        "name": profile.display_name(),
        "process": profile.process.display(),
        "title_contains": profile.window_title_contains,
        "icon_color": profile.icon.as_ref().map(|i| i.color.clone()),
        "note": profile.note,
        "bindings": profile.bindings.iter().map(binding_json).collect::<Vec<_>>(),
    })
}

fn binding_json(binding: &KeyBinding) -> Value {
    json!({
        "button": button_key(&binding.button),
        "name": binding.button.display_name(),
        "trigger": trigger_key(&binding.trigger),
        "action": action_label(&binding.action),
        "action_key": action_key(&binding.action),
        "enabled": binding.enabled,
    })
}
