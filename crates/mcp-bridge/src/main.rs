//! Remote Mic MCP stdio bridge.
//!
//! 这是外部智能体唯一看到的接口：MCP over stdio。
//! 它不直接改配置，而是读 `agent-api.json` 后把 tools/call 转发给
//! Remote Mic 主进程的本地 JSON-RPC（127.0.0.1 + token）。
//!
//! 规范基线：MCP 2026-07-28（modern，无 initialize、每请求 `_meta`）
//! + 2025-11-25（legacy，initialize 握手）双 era。

mod http;
mod protocol;
mod tools;

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use protocol::{is_modern, modern_error, tool_error_result, tool_ok_result};
use tools::{call_tool, tool_definitions, ToolCallError};

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut legacy_mode = false;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("[remote-mic-mcp] 读取 stdin 失败：{e}");
                break;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                write_message(
                    &mut stdout,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": { "code": -32700, "message": format!("JSON 解析失败：{e}") }
                    }),
                );
                continue;
            }
        };

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        // legacy 握手
        if method == "initialize" {
            legacy_mode = true;
            write_message(
                &mut stdout,
                &json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "remote-mic", "version": env!("CARGO_PKG_VERSION") }
                    }
                }),
            );
            continue;
        }
        if method == "notifications/initialized" || method == "notifications/cancelled" {
            continue;
        }

        // 没有 _meta 且不是 legacy 握手后的请求：按 legacy 兼容处理，尽量兼容旧客户端。
        let era = if is_modern(&params) {
            match protocol::check_protocol_version(&params) {
                Ok(()) => protocol::Era::Modern,
                Err(err) => {
                    write_message(&mut stdout, &json!({ "jsonrpc": "2.0", "id": id, "error": err }));
                    continue;
                }
            }
        } else {
            protocol::Era::Legacy
        };
        let _ = legacy_mode;

        match method {
            "server/discover" => {
                write_message(&mut stdout, &protocol::discover_result(id.clone()));
            }
            "tools/list" => {
                let tools = tool_definitions();
                let mut result = json!({ "tools": tools });
                if era == protocol::Era::Modern {
                    result["resultType"] = json!("complete");
                    result["ttlMs"] = json!(300_000);
                    result["cacheScope"] = json!("private");
                }
                write_message(&mut stdout, &json!({ "jsonrpc": "2.0", "id": id, "result": result }));
            }
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                match call_tool(name, arguments) {
                    Ok((message, data, revision)) => {
                        let result = tool_ok_result(era, &message, data, revision);
                        write_message(&mut stdout, &json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                    }
                    Err(ToolCallError::UnknownTool) => {
                        write_message(
                            &mut stdout,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": { "code": -32602, "message": format!("未知工具：{name}") }
                            }),
                        );
                    }
                    Err(ToolCallError::Execution { code, message, hint }) => {
                        let result = tool_error_result(era, &code, &message, hint.as_deref());
                        write_message(&mut stdout, &json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                    }
                }
            }
            "ping" => {
                // legacy 兼容；modern 2026-07-28 已移除 ping。
                if era == protocol::Era::Modern {
                    write_message(&mut stdout, &json!({ "jsonrpc": "2.0", "id": id, "error": modern_error(-32601, "modern 协议已移除 ping") }));
                } else {
                    write_message(&mut stdout, &json!({ "jsonrpc": "2.0", "id": id, "result": {} }));
                }
            }
            _ => {
                write_message(
                    &mut stdout,
                    &json!({ "jsonrpc": "2.0", "id": id, "error": modern_error(-32601, format!("未知方法：{method}")) }),
                );
            }
        }
    }
}

fn write_message(stdout: &mut std::io::Stdout, value: &Value) {
    // stdout 只允许 MCP 消息；日志一律 stderr。
    if let Ok(text) = serde_json::to_string(value) {
        let _ = writeln!(stdout, "{text}");
        let _ = stdout.flush();
    }
}
