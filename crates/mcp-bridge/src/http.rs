//! 读取发现文件 + 调用应用内 127.0.0.1 JSON-RPC。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discovery {
    pub enabled: bool,
    pub api_version: u32,
    pub app_version: String,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    #[serde(default)]
    pub started_at: u64,
}

#[derive(Debug, Clone)]
pub struct RpcCallError {
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
}

impl RpcCallError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            hint: None,
        }
    }

    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

fn discovery_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("REMOTE_MIC_AGENT_API") {
        if !path.trim().is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    if let Ok(base) = std::env::var("LOCALAPPDATA") {
        return Some(
            PathBuf::from(base)
                .join("RemoteMic")
                .join("RC003")
                .join("agent-api.json"),
        );
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        return Some(
            PathBuf::from(profile)
                .join("AppData")
                .join("Local")
                .join("RemoteMic")
                .join("RC003")
                .join("agent-api.json"),
        );
    }
    None
}

pub fn read_discovery() -> Result<Discovery, RpcCallError> {
    let path = discovery_path().ok_or_else(|| {
        RpcCallError::new(
            "app_not_running",
            "找不到 agent-api.json；请先启动 Remote Mic 并在 通用设置 → 智能体接入 中开启",
        )
    })?;
    let text = std::fs::read_to_string(&path).map_err(|e| {
        RpcCallError::new(
            "app_not_running",
            format!(
                "读取 {} 失败：{e}；请先启动 Remote Mic 并在 通用设置 → 智能体接入 中开启",
                path.display()
            ),
        )
    })?;
    let discovery: Discovery = serde_json::from_str(&text).map_err(|e| {
        RpcCallError::new("app_not_running", format!("agent-api.json 解析失败：{e}"))
    })?;
    if !discovery.enabled {
        return Err(RpcCallError::new(
            "agent_api_disabled",
            "智能体接口未启用；请在 通用设置 → 智能体接入 中打开开关",
        ));
    }
    if discovery.api_version != 1 {
        return Err(RpcCallError::new(
            "api_version_mismatch",
            format!(
                "MCP sidecar 与应用 API 版本不匹配（应用 api_version={}）",
                discovery.api_version
            ),
        )
        .with_hint("请升级 Remote Mic / 重新复制 MCP 配置"));
    }
    Ok(discovery)
}

/// 调应用内 RPC；返回 `result` 字段。
pub fn call_rpc(method: &str, params: Value) -> Result<Value, RpcCallError> {
    let discovery = read_discovery()?;
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    })
    .to_string();
    let addr = format!("127.0.0.1:{}", discovery.port);
    let mut stream = TcpStream::connect(&addr).map_err(|e| {
        RpcCallError::new(
            "app_not_running",
            format!("连接 Remote Mic 失败（{addr}）：{e}"),
        )
    })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let request = format!(
        "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        discovery.port,
        discovery.token,
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| RpcCallError::new("app_not_running", format!("写入请求失败：{e}")))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| RpcCallError::new("app_not_running", format!("读取响应失败：{e}")))?;
    let response = String::from_utf8_lossy(&response);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| RpcCallError::new("internal_error", "应用返回了非法 HTTP 响应"))?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    match status {
        "200" => {}
        "401" => {
            return Err(RpcCallError::new(
                "unauthorized",
                "token 无效（应用可能已重启并轮换 token）",
            )
            .with_hint("重启 MCP 客户端，或重新复制 MCP 配置"))
        }
        "429" => {
            return Err(RpcCallError::new(
                "rate_limited",
                "请求过于频繁，请稍后重试",
            ))
        }
        other => {
            return Err(RpcCallError::new(
                "internal_error",
                format!("应用返回 HTTP {other}"),
            ))
        }
    }

    let value: Value = serde_json::from_str(body)
        .map_err(|e| RpcCallError::new("internal_error", format!("响应 JSON 解析失败：{e}")))?;
    if let Some(error) = value.get("error") {
        let code = error.get("code").and_then(Value::as_i64).unwrap_or(-32007);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("应用返回错误");
        let mapped = match code {
            -32003 => "validation_error",
            -32004 => "not_found",
            -32005 => "conflict",
            -32006 => "state_error",
            -32601 => "unknown_method",
            -32029 => "rate_limited",
            _ => "internal_error",
        };
        let mut err = RpcCallError::new(mapped, message);
        if let Some(hint) = error
            .get("data")
            .and_then(|d| d.get("hint"))
            .and_then(Value::as_str)
        {
            err = err.with_hint(hint);
        }
        return Err(err);
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}
