//! MCP 工具定义与 tools/call → 应用 RPC 的映射。

use serde_json::{json, Value};

use crate::http::{self, RpcCallError};

pub enum ToolCallError {
    UnknownTool,
    Execution {
        code: String,
        message: String,
        hint: Option<String>,
    },
}

fn rpc(method: &str, params: Value) -> Result<Value, ToolCallError> {
    http::call_rpc(method, params).map_err(|e: RpcCallError| ToolCallError::Execution {
        code: e.code,
        message: e.message,
        hint: e.hint,
    })
}

fn revision_of(value: &Value) -> Option<u64> {
    value.get("revision").and_then(Value::as_u64)
}

pub fn call_tool(
    name: &str,
    args: Value,
) -> Result<(String, Value, Option<u64>), ToolCallError> {
    match name {
        "remote_mic_get_status" => {
            let health = rpc("agent.health", json!({}))?;
            let runtime = rpc("runtime.status", json!({}))?;
            let revision = revision_of(&health);
            Ok((
                "已读取 Remote Mic 运行状态".into(),
                json!({ "health": health, "runtime": runtime }),
                revision,
            ))
        }
        "remote_mic_get_mapping_catalog" => {
            let data = rpc("mapping.catalog", json!({}))?;
            Ok(("已读取映射目录".into(), data, None))
        }
        "remote_mic_list_mappings" => {
            let data = rpc("mapping.list", args)?;
            let revision = revision_of(&data);
            Ok(("已读取当前映射".into(), data, revision))
        }
        "remote_mic_set_mapping" => {
            let data = rpc("mapping.set", args)?;
            let revision = revision_of(&data);
            let new_action = data
                .get("new")
                .and_then(|v| v.get("action"))
                .and_then(Value::as_str)
                .unwrap_or("新动作");
            Ok((
                format!("已保存映射：{new_action}"),
                data,
                revision,
            ))
        }
        "remote_mic_clear_mapping" => {
            let data = rpc("mapping.clear", args)?;
            let revision = revision_of(&data);
            Ok(("已取消关联".into(), data, revision))
        }
        "remote_mic_set_mapping_enabled" => {
            let enabled = args.get("enabled").and_then(Value::as_bool).unwrap_or(true);
            let data = rpc("mapping.set_enabled", args)?;
            let revision = revision_of(&data);
            Ok((
                if enabled {
                    "已启用绑定".into()
                } else {
                    "已禁用绑定".into()
                },
                data,
                revision,
            ))
        }
        "remote_mic_get_trigger_timing" => {
            let data = rpc("timing.get", json!({}))?;
            let revision = revision_of(&data);
            Ok(("已读取触发时间".into(), data, revision))
        }
        "remote_mic_set_trigger_timing" => {
            let data = rpc("timing.set", args)?;
            let revision = revision_of(&data);
            Ok(("已保存触发时间".into(), data, revision))
        }
        "remote_mic_list_profiles" => {
            let data = rpc("profiles.list", json!({}))?;
            Ok(("已读取应用档案".into(), data, None))
        }
        "remote_mic_list_shortcuts" => {
            let data = rpc("shortcuts.list", json!({}))?;
            let revision = revision_of(&data);
            Ok(("已读取自定义快捷键".into(), data, revision))
        }
        "remote_mic_get_voice_target" => {
            let data = rpc("voice.get", json!({}))?;
            Ok(("已读取语音目标".into(), data, None))
        }
        "remote_mic_get_eat_mode" => {
            let data = rpc("eat.get", json!({}))?;
            Ok(("已读取吃掉模式".into(), data, None))
        }
        _ => Err(ToolCallError::UnknownTool),
    }
}

fn output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "ok": { "type": "boolean" },
            "message": { "type": "string" },
            "data": { "type": "object" },
            "revision": { "type": "integer" },
            "warnings": { "type": "array", "items": { "type": "string" } },
            "error": {
                "type": "object",
                "properties": {
                    "code": { "type": "string" },
                    "message": { "type": "string" },
                    "field": { "type": "string" }
                },
                "required": ["code", "message"]
            },
            "hint": { "type": "string" }
        },
        "required": ["ok"]
    })
}

fn annotations(read_only: bool, destructive: bool, idempotent: bool) -> Value {
    json!({
        "readOnlyHint": read_only,
        "destructiveHint": destructive,
        "idempotentHint": idempotent,
        "openWorldHint": false
    })
}

fn tool(name: &str, title: &str, description: &str, input: Value, ann: Value) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input,
        "outputSchema": output_schema(),
        "annotations": ann,
    })
}

fn no_input() -> Value {
    json!({ "type": "object", "additionalProperties": false, "properties": {} })
}

fn binding_input(extra_enabled: bool) -> Value {
    let mut properties = json!({
        "button": { "type": "string", "description": "Button key from the catalog, e.g. volume_up, ok, mic" },
        "trigger": {
            "type": "string",
            "enum": ["single_click", "double_click", "long_press", "press", "release"],
            "description": "Trigger type. mic only supports press/release."
        },
        "action": { "type": "string", "description": "action_key from remote_mic_get_mapping_catalog, or combo:lctrl+k" },
        "scope": { "type": "string", "description": "global (default). App profile ids are not supported in this MVP." },
        "if_revision": { "type": "integer", "minimum": 0, "description": "Optional optimistic concurrency guard" }
    });
    let mut required = vec!["button", "trigger", "action"];
    if extra_enabled {
        properties["enabled"] = json!({ "type": "boolean", "description": "true = enabled, false = disabled" });
        required.push("enabled");
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
        "required": required
    })
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "remote_mic_get_status",
            "读取运行状态",
            "Read Remote Mic health: app version, API revision, connection/bridge/tap runtime status. Read-only.",
            no_input(),
            annotations(true, false, true),
        ),
        tool(
            "remote_mic_get_mapping_catalog",
            "读取映射目录",
            "List all supported buttons, triggers, action categories, and default bindings. Call this before setting a mapping to obtain valid action_key values. Read-only.",
            no_input(),
            annotations(true, false, true),
        ),
        tool(
            "remote_mic_list_mappings",
            "读取当前映射",
            "List persisted global key bindings with enabled state and the current config revision. Read-only.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "scope": { "type": "string", "description": "global (default)" }
                }
            }),
            annotations(true, false, true),
        ),
        tool(
            "remote_mic_set_mapping",
            "设置按键映射",
            "Create or update one key binding in the global mapping. The change is hot-reloaded immediately. Saving also enables the binding. Prefer action_key values from the catalog.",
            binding_input(false),
            annotations(false, false, true),
        ),
        tool(
            "remote_mic_clear_mapping",
            "取消关联",
            "Delete one key binding from the global mapping. This is destructive but reversible via the returned old binding. The change is hot-reloaded immediately.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "button": { "type": "string" },
                    "trigger": { "type": "string", "enum": ["single_click", "double_click", "long_press", "press", "release"] },
                    "scope": { "type": "string", "description": "global (default)" },
                    "if_revision": { "type": "integer", "minimum": 0 }
                },
                "required": ["button", "trigger"]
            }),
            annotations(false, true, true),
        ),
        tool(
            "remote_mic_set_mapping_enabled",
            "禁用或启用绑定",
            "Toggle one existing global binding between enabled and disabled. Disabled bindings keep their action and record but do not trigger and do not participate in gesture timing.",
            binding_input(true),
            annotations(false, false, true),
        ),
        tool(
            "remote_mic_get_trigger_timing",
            "读取触发时间",
            "Read the long-press threshold and double-click window in milliseconds. Read-only.",
            no_input(),
            annotations(true, false, true),
        ),
        tool(
            "remote_mic_set_trigger_timing",
            "设置触发时间",
            "Set long_press_ms (200-2000) and double_click_ms (150-1000). Values are clamped by the app and hot-reloaded immediately.",
            json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "long_press_ms": { "type": "integer", "minimum": 200, "maximum": 2000 },
                    "double_click_ms": { "type": "integer", "minimum": 150, "maximum": 1000 },
                    "if_revision": { "type": "integer", "minimum": 0 }
                },
                "required": ["long_press_ms", "double_click_ms"]
            }),
            annotations(false, false, true),
        ),
        tool(
            "remote_mic_list_profiles",
            "读取应用档案",
            "List app-specific profiles and their overridden bindings. Read-only. Profile writes are not available in this MVP.",
            no_input(),
            annotations(true, false, true),
        ),
        tool(
            "remote_mic_list_shortcuts",
            "读取自定义快捷键",
            "List user-defined named shortcuts. Read-only.",
            no_input(),
            annotations(true, false, true),
        ),
        tool(
            "remote_mic_get_voice_target",
            "读取语音目标",
            "Read the configured voice input target (windows_voice or a reserved IME target). Read-only.",
            no_input(),
            annotations(true, false, true),
        ),
        tool(
            "remote_mic_get_eat_mode",
            "读取吃掉模式",
            "Read whether HID tap eat mode is enabled. Read-only; changing it is intentionally not exposed to agents.",
            no_input(),
            annotations(true, false, true),
        ),
    ]
}
