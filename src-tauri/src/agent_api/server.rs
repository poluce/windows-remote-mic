//! 极简本地 HTTP JSON-RPC 服务（仅 127.0.0.1，供 MCP sidecar 调用）。

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::agent_api::methods;

const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;
const RATE_LIMIT_PER_SEC: usize = 20;

struct RateLimiter {
    hits: Mutex<VecDeque<Instant>>,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            hits: Mutex::new(VecDeque::new()),
        }
    }

    fn allow(&self) -> bool {
        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        while hits
            .front()
            .map(|t| now.duration_since(*t) > Duration::from_secs(1))
            .unwrap_or(false)
        {
            hits.pop_front();
        }
        if hits.len() >= RATE_LIMIT_PER_SEC {
            return false;
        }
        hits.push_back(now);
        true
    }
}

pub fn run(
    listener: TcpListener,
    port: u16,
    token: String,
    shutdown: Arc<AtomicBool>,
    app: AppHandle,
    dispatcher: Arc<core_dispatch::KeyDispatcher>,
) {
    let _ = listener.set_nonblocking(true);
    let limiter = RateLimiter::new();
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(e) = handle(stream, port, &token, &app, &dispatcher, &limiter) {
                    core_log::log_debug(&format!("[agent-api] 请求处理失败：{e}"));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                core_log::log_warn(&format!("[agent-api] accept 失败：{e}"));
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn handle(
    mut stream: TcpStream,
    port: u16,
    token: &str,
    app: &AppHandle,
    dispatcher: &core_dispatch::KeyDispatcher,
    limiter: &RateLimiter,
) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let (headers, body_start) = read_headers(&mut stream)?;
    let request_line = headers.lines().next().ok_or_else(|| "空请求".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if method != "POST" || path != "/rpc" {
        return respond_error(&mut stream, 404, "not_found", "只支持 POST /rpc");
    }

    let mut header_map = std::collections::HashMap::new();
    for line in headers.lines().skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            header_map.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    // 浏览器请求带 Origin，直接拒绝（MCP 客户端不会带）。
    if header_map.contains_key("origin") {
        return respond_error(&mut stream, 403, "forbidden", "不允许浏览器来源");
    }

    let auth = header_map
        .get("authorization")
        .map(String::as_str)
        .unwrap_or("");
    let expected = format!("Bearer {token}");
    if !secure_eq(auth.as_bytes(), expected.as_bytes()) {
        return respond_error(&mut stream, 401, "unauthorized", "token 无效");
    }

    let host = header_map.get("host").map(String::as_str).unwrap_or("");
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return respond_error(&mut stream, 400, "bad_host", "Host 头不匹配");
    }

    if !limiter.allow() {
        return respond_error(&mut stream, 429, "rate_limited", "请求过于频繁");
    }

    let content_length: usize = header_map
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| "缺少 Content-Length".to_string())?;
    if content_length > MAX_BODY {
        return respond_error(&mut stream, 413, "too_large", "请求体过大");
    }

    let mut body = Vec::new();
    if body_start.len() >= content_length {
        body.extend_from_slice(&body_start[..content_length]);
    } else {
        body.extend_from_slice(&body_start);
        let mut remaining = content_length - body.len();
        let mut buf = [0u8; 4096];
        while remaining > 0 {
            let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            let take = n.min(remaining);
            body.extend_from_slice(&buf[..take]);
            remaining -= take;
        }
    }

    let request: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    if method.is_empty() {
        let resp = json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32600, "message": "无效请求：缺少 method" }
        });
        return respond_json(&mut stream, 200, &resp);
    }
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    let response = match methods::dispatch(method, params, app, dispatcher) {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(err) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": err.code,
                "message": err.message,
                "data": err.data
            }
        }),
    };
    respond_json(&mut stream, 200, &response)
}

fn read_headers(stream: &mut TcpStream) -> Result<(String, Vec<u8>), String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("连接提前关闭".into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_header_end(&buf) {
            let headers = String::from_utf8_lossy(&buf[..pos]).to_string();
            let rest = buf[pos + 4..].to_vec();
            return Ok((headers, rest));
        }
        if buf.len() > MAX_HEADER {
            return Err("请求头过大".into());
        }
    }
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn secure_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn respond_error(
    stream: &mut TcpStream,
    status: u16,
    code: &str,
    message: &str,
) -> Result<(), String> {
    let body = json!({ "ok": false, "error": { "code": code, "message": message } });
    respond_json(stream, status, &body)
}

fn respond_json(stream: &mut TcpStream, status: u16, body: &Value) -> Result<(), String> {
    let payload = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream
        .write_all(head.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(&payload).map_err(|e| e.to_string())?;
    let _ = stream.flush();
    Ok(())
}
