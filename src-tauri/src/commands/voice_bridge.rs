//! 语音桥 Tauri 命令：**薄适配层**。
//!
//! 状态机与重试策略在 `core_voice::supervisor`；这里只负责：
//! - 把 Tauri 事件（`bridge-status` / `ble-connection-status`）接到 `SupervisorHost`；
//! - 启动/停止监督线程、等待退出；
//! - 「断开」时暂停、连接时恢复遥控器按键接管。
//!
//! 这样命令层不再包含业务策略，监督状态机也可以脱离 Tauri 单测。

use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, Manager};

use core_voice::supervisor as sup;

/// 停止结果：前端据此决定是否真的标记为「已断开」。
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStopResult {
    pub stopped: bool,
    pub message: String,
}

/// 把 `core_voice::supervisor::SupervisorHost` 端口接到 Tauri / AppState。
struct TauriSupervisorHost {
    app: tauri::AppHandle,
}

impl sup::SupervisorHost for TauriSupervisorHost {
    fn emit_status(&self, status: &sup::BridgeStatus) {
        let _ = self.app.emit("bridge-status", status);
    }

    fn emit_connection(&self, connected: bool) {
        let _ = self.app.emit("ble-connection-status", connected);
    }

    fn release_held_keys(&self, reason: &str) {
        crate::force_release_held_keys(&self.app, reason);
    }
}

/// 恢复遥控器按键接管（连接语音桥时调用）。
fn resume_key_passthrough(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        if state.dispatcher.set_enabled(true) {
            core_log::log_info("[dispatch] 遥控器按键接管已恢复（语音桥已连接）");
        }
    }
}

/// 暂停遥控器按键接管（断开语音桥时调用）。
///
/// 「断开」如果只停语音桥、不停按键接管，用户会疑惑：明明显示未连接，
/// 按遥控器麦克风键却还能唤起豆包等输入法（HID 旁路 + 映射注入仍在工作）。
fn pause_key_passthrough(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        if state.dispatcher.set_enabled(false) {
            core_log::log_info("[dispatch] 遥控器按键接管已暂停（语音桥已断开）");
        }
    }
}

/// 启动真实设备的语音桥（仅 Windows）。
///
/// 事件驱动：先起一个只监听 BLE 链路状态的监督线程，等系统把遥控器连上后
/// 才按需建立一次 ATVV 会话；不会定时盲连，也不会和别的电脑抢设备。
#[tauri::command]
pub fn start_voice_bridge(
    app: tauri::AppHandle,
    device_id: String,
    output_device: String,
) -> String {
    #[cfg(target_os = "windows")]
    {
        if !sup::try_start() {
            core_log::log_warn("[voice_bridge] 拒绝重复启动：语音桥监督线程已在运行");
            return "语音桥已在运行（请勿重复启动）".to_string();
        }
        core_voice::clear_bridge_stop_request();
        // 断开时暂停过按键接管，重新连接/重试时恢复，避免「连上了但按键没反应」。
        resume_key_passthrough(&app);
        let loop_id = sup::next_loop_id();
        core_log::log_info(&format!(
            "[voice_bridge] 收到启动语音桥请求：loop_id={loop_id}, 设备 ID='{device_id}'，输出='{output_device}'"
        ));

        let host = Arc::new(TauriSupervisorHost { app: app.clone() });
        sup::publish(
            host.as_ref(),
            sup::BridgeStatus::new("starting", 0, 0, 0, String::new()),
        );

        std::thread::spawn(move || {
            sup::run_supervisor(&device_id, &output_device, loop_id, host);
        });
        "语音桥已启动（系统连上遥控器后会自动接续）".to_string()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, device_id, output_device);
        "语音桥仅在 Windows 可用".to_string()
    }
}

/// 重试启动（失败/等待状态下用户点「重试」）：先停旧监督线程，再重新启动。
#[tauri::command]
pub fn retry_voice_bridge(
    app: tauri::AppHandle,
    device_id: String,
    output_device: String,
) -> String {
    #[cfg(target_os = "windows")]
    {
        if sup::is_running() {
            core_voice::request_bridge_stop();
            for _ in 0..50 {
                if !sup::is_running() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if sup::is_running() {
                return "语音桥仍在停止中，请稍后重试".to_string();
            }
        }
        start_voice_bridge(app, device_id, output_device)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, device_id, output_device);
        "语音桥仅在 Windows 可用".to_string()
    }
}

/// 停止语音桥重连循环（当前 run_bridge 会话结束后、或最迟约 10 秒内生效）。
///
/// 之前无论是否真的停下都返回「已断开连接」，用户会以为断了但实际仍在重连。
#[tauri::command]
pub fn stop_voice_bridge(app: tauri::AppHandle) -> BridgeStopResult {
    let host = TauriSupervisorHost { app: app.clone() };

    if !sup::is_running() {
        core_voice::set_connection_active(false);
        pause_key_passthrough(&app);
        sup::publish(
            &host,
            sup::BridgeStatus::new("stopped", 0, 0, 0, String::new()),
        );
        return BridgeStopResult {
            stopped: true,
            message: "语音桥未在运行；遥控器按键接管已暂停，点「连接」恢复".to_string(),
        };
    }

    core_voice::request_bridge_stop();
    core_voice::set_connection_active(false);
    // 最长等 10s：WinRT 操作已有超时，正常会在数秒内退出。
    for _ in 0..100 {
        if !sup::is_running() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let stopped = !sup::is_running();
    if stopped {
        // 断开 = 停止语音桥 + 暂停按键接管：
        // 否则遥控器麦克风键仍会经 HOGP 旁路 + 映射注入唤起豆包等输入法。
        pause_key_passthrough(&app);
        sup::publish(
            &host,
            sup::BridgeStatus::new("stopped", 0, 0, 0, String::new()),
        );
    }
    core_log::log_info(&format!("[voice_bridge] 停止语音桥：stopped={stopped}"));
    BridgeStopResult {
        stopped,
        message: if stopped {
            "已断开连接（遥控器按键接管已暂停，点「连接」恢复）".to_string()
        } else {
            "停止超时：语音桥仍在运行（可能卡在蓝牙操作，请稍后重试）".to_string()
        },
    }
}
