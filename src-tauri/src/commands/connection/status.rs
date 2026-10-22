//! 运行时状态快照：连接页启动时恢复「已连接 / 旁路 / 端点 / 语音桥」四类状态。

use serde::Serialize;

use core_voice::supervisor::{current_status, BridgeStatus};

/// 连接页需要恢复的运行时状态快照。
#[derive(Serialize)]
pub struct RuntimeStatus {
    pub connected: bool,
    pub tap_status: Option<core_hid::tap::TapStatusEvent>,
    pub endpoints_ready: bool,
    /// 最近一次语音桥状态（连接页恢复「重连中/失败」展示用）。
    pub bridge_status: Option<BridgeStatus>,
}

#[tauri::command]
pub fn get_runtime_status() -> RuntimeStatus {
    RuntimeStatus {
        connected: core_voice::connection_active(),
        tap_status: core_hid::tap::last_status(),
        endpoints_ready: core_voice::atvv_endpoints_ready(),
        bridge_status: current_status(),
    }
}
