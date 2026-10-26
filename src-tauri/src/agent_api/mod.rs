//! 应用内智能体接口（MCP 的本地 RPC 桥）。
//!
//! 只绑 `127.0.0.1` 随机端口，token 写入 `%LOCALAPPDATA%\RemoteMic\RC003\agent-api.json`。
//! 对外只通过 MCP sidecar 暴露；这个 HTTP 端点不是公开 API。

mod methods;
mod server;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::{config_store, AppState};

pub(crate) const API_VERSION: u32 = 1;

/// 面向前端 / MCP 的接口状态。
#[derive(Debug, Clone, Serialize)]
pub struct AgentApiStatus {
    pub enabled: bool,
    pub running: bool,
    pub port: Option<u16>,
    pub discovery_path: String,
    pub sidecar_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiscoveryFile {
    enabled: bool,
    api_version: u32,
    app_version: String,
    port: u16,
    token: String,
    pid: u32,
    started_at: u64,
}

struct RunningAgent {
    port: u16,
    shutdown: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

static RUNNING: Mutex<Option<RunningAgent>> = Mutex::new(None);

/// 串行化 start/stop，避免快速开关时「start 刚写完发现文件、stop 又把它删掉」的竞态。
static LIFECYCLE: Mutex<()> = Mutex::new(());

fn discovery_path() -> Result<PathBuf, String> {
    config_store()
        .map(|s| s.dir.join("agent-api.json"))
        .ok_or_else(|| "无法定位配置目录".to_string())
}

fn sidecar_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("remote-mic-mcp.exe")))
        .unwrap_or_else(|| PathBuf::from("remote-mic-mcp.exe"))
}

fn generate_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("生成 token 失败：{e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn write_discovery(port: u16, token: &str) -> Result<(), String> {
    let path = discovery_path()?;
    let tmp = path.with_extension("json.tmp");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = DiscoveryFile {
        enabled: true,
        api_version: API_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        port,
        token: token.to_string(),
        pid: std::process::id(),
        started_at: now,
    };
    let json = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| format!("写 agent-api.json 失败：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("提交 agent-api.json 失败：{e}"))?;
    Ok(())
}

fn remove_discovery() {
    if let Ok(path) = discovery_path() {
        let _ = std::fs::remove_file(path);
    }
}

fn status_inner() -> AgentApiStatus {
    let enabled = config_store()
        .and_then(|s| s.load().ok())
        .map(|cfg| cfg.agent_api_enabled)
        .unwrap_or(false);
    let running = RUNNING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|r| (true, Some(r.port)))
        .unwrap_or((false, None));
    AgentApiStatus {
        enabled,
        running: running.0,
        port: running.1,
        discovery_path: discovery_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        sidecar_path: sidecar_path().to_string_lossy().to_string(),
    }
}

/// 启动本地 RPC（幂等；已在运行时直接返回状态）。
pub fn start(
    app: AppHandle,
    dispatcher: Arc<core_dispatch::KeyDispatcher>,
) -> Result<AgentApiStatus, String> {
    let _lifecycle = LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    {
        let running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        if running.is_some() {
            drop(running);
            return Ok(status_inner());
        }
    }

    let token = generate_token()?;
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("绑定智能体接口端口失败：{e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("读取智能体接口端口失败：{e}"))?
        .port();
    write_discovery(port, &token)?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown2 = shutdown.clone();
    let app2 = app.clone();
    let join = match std::thread::Builder::new()
        .name("agent-api".into())
        .spawn(move || server::run(listener, port, token, shutdown2, app2, dispatcher))
    {
        Ok(join) => join,
        Err(e) => {
            // 线程没起来时端口已绑定、发现文件已写入，必须清理，避免留下失效 token。
            remove_discovery();
            return Err(format!("启动智能体接口线程失败：{e}"));
        }
    };

    let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    *running = Some(RunningAgent {
        port,
        shutdown,
        join: Some(join),
    });
    drop(running);
    core_log::log_info(&format!(
        "[agent-api] 智能体接口已启动：127.0.0.1:{port}（仅本机）"
    ));
    Ok(status_inner())
}

/// 停止本地 RPC 并删除发现文件。
pub fn stop() {
    let _lifecycle = LIFECYCLE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut running) = RUNNING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        remove_discovery();
        return;
    };
    running.shutdown.store(true, Ordering::Relaxed);
    remove_discovery();
    if let Some(join) = running.join.take() {
        let _ = join.join();
    }
    core_log::log_info(&format!(
        "[agent-api] 智能体接口已停止（原端口 {}）",
        running.port
    ));
}

/// 应用启动时按配置恢复。
pub fn start_if_enabled(app: AppHandle, dispatcher: Arc<core_dispatch::KeyDispatcher>) {
    let enabled = config_store()
        .and_then(|s| s.load().ok())
        .map(|cfg| cfg.agent_api_enabled)
        .unwrap_or(false);
    if !enabled {
        remove_discovery();
        return;
    }
    if let Err(e) = start(app, dispatcher) {
        core_log::log_warn(&format!("[agent-api] 启动失败：{e}"));
    }
}

// ---------------------------------------------------------------------------
// Tauri commands（设置页开关）
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_agent_api_status() -> AgentApiStatus {
    status_inner()
}

/// 界面上的明确开关：开启即启动 RPC 并生成 token，关闭即停止并删除 token 文件。
#[tauri::command]
pub fn set_agent_api_enabled(
    enabled: bool,
    app: AppHandle,
    state: State<AppState>,
) -> Result<AgentApiStatus, String> {
    let store = config_store().ok_or_else(|| "无法创建配置目录".to_string())?;
    store.update(|cfg| {
        cfg.agent_api_enabled = enabled;
        Ok(())
    })?;

    if enabled {
        if let Err(e) = start(app, state.dispatcher.clone()) {
            let _ = store.update(|cfg| {
                cfg.agent_api_enabled = false;
                Ok(())
            });
            return Err(e);
        }
    } else {
        stop();
    }
    Ok(status_inner())
}

/// 重新生成 token（MCP 配置不需要改，sidecar 每次调用前重新读发现文件）。
#[tauri::command]
pub fn regenerate_agent_api_token(
    app: AppHandle,
    state: State<AppState>,
) -> Result<AgentApiStatus, String> {
    let enabled = config_store()
        .and_then(|s| s.load().ok())
        .map(|cfg| cfg.agent_api_enabled)
        .unwrap_or(false);
    if !enabled {
        return Err("智能体接口未启用".into());
    }
    stop();
    start(app, state.dispatcher.clone())
}

/// 给设置页「复制 MCP 配置」用的信息。
#[tauri::command]
pub fn get_agent_api_config() -> Result<serde_json::Value, String> {
    let status = status_inner();
    let sidecar = status.sidecar_path.clone();
    Ok(serde_json::json!({
        "enabled": status.enabled,
        "running": status.running,
        "port": status.port,
        "discovery_path": status.discovery_path,
        "sidecar_path": sidecar,
        "mcp_config": {
            "mcpServers": {
                "remote-mic": {
                    "command": sidecar,
                    "args": []
                }
            }
        }
    }))
}
