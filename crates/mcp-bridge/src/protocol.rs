//! MCP 协议常量与结果包装（2026-07-28 modern + 2025-11-25 legacy）。

use serde_json::{json, Value};

pub const MODERN_VERSION: &str = "2026-07-28";
pub const SUPPORTED_VERSIONS: &[&str] = &[MODERN_VERSION];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Era {
    Modern,
    Legacy,
}

pub fn is_modern(params: &Value) -> bool {
    params
        .get("_meta")
        .and_then(|m| m.get("io.modelcontextprotocol/protocolVersion"))
        .is_some()
}

/// modern 请求必须声明受支持的协议版本；不支持时返回 -32022。
pub fn check_protocol_version(params: &Value) -> Result<(), Value> {
    let requested = params
        .get("_meta")
        .and_then(|m| m.get("io.modelcontextprotocol/protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if SUPPORTED_VERSIONS.contains(&requested) {
        return Ok(());
    }
    Err(json!({
        "code": -32022,
        "message": "Unsupported protocol version",
        "data": {
            "supported": SUPPORTED_VERSIONS,
            "requested": requested,
        }
    }))
}

pub fn server_info() -> Value {
    json!({
        "name": "remote-mic",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

pub fn discover_result(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "resultType": "complete",
            "supportedVersions": SUPPORTED_VERSIONS,
            "capabilities": { "tools": {} },
            "_meta": {
                "io.modelcontextprotocol/serverInfo": server_info(),
            },
            "instructions": "Remote Mic configuration server. Read the mapping catalog first, then set/clear/enable bindings. Changes apply immediately to the running app.",
            "ttlMs": 3_600_000,
            "cacheScope": "private",
        }
    })
}

pub fn modern_error(code: i32, message: impl Into<String>) -> Value {
    json!({ "code": code, "message": message.into() })
}

pub fn tool_ok_result(era: Era, message: &str, data: Value, revision: Option<u64>) -> Value {
    let mut structured = json!({
        "ok": true,
        "message": message,
        "data": data,
        "warnings": [],
    });
    if let Some(revision) = revision {
        structured["revision"] = json!(revision);
    }
    let mut result = json!({
        "content": [{ "type": "text", "text": structured.to_string() }],
        "structuredContent": structured,
        "isError": false,
    });
    if era == Era::Modern {
        result["resultType"] = json!("complete");
    }
    result
}

pub fn tool_error_result(era: Era, code: &str, message: &str, hint: Option<&str>) -> Value {
    let mut structured = json!({
        "ok": false,
        "error": { "code": code, "message": message },
    });
    if let Some(hint) = hint {
        structured["hint"] = json!(hint);
    }
    let mut result = json!({
        "content": [{ "type": "text", "text": structured.to_string() }],
        "structuredContent": structured,
        "isError": true,
    });
    if era == Era::Modern {
        result["resultType"] = json!("complete");
    }
    result
}
