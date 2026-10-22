//! 旁路状态：机器可读状态机 + 推送前端的状态事件 + 注入后宽限期。
//!
//! 与注入流程（`inject.rs`）和看门狗（tap.rs）解耦：状态只在这里写入与查询。

use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;

use super::INPUT_GRACE;

/// 旁路子系统的机器可读状态，与前端 `TapStatus` 一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TapState {
    /// 未启动或尚未产生任何状态。
    Idle,
    /// 启动中 / 等待注入或旁路连接。
    Pending,
    /// 已附着 HOGP 宿主，旁路可用。
    Attached,
    /// 确定不可用（缺 Gadget、UAC 被拒、注入失败等）。
    Unavailable,
}

/// 推送给前端的旁路状态事件：状态枚举 + 展示消息。
#[derive(Debug, Clone, Serialize)]
pub struct TapStatusEvent {
    pub state: TapState,
    pub message: String,
}

type StatusCallback = Box<dyn Fn(TapStatusEvent) + Send>;

static STATUS_CB: Mutex<Option<StatusCallback>> = Mutex::new(None);
static LAST_STATUS: Mutex<Option<TapStatusEvent>> = Mutex::new(None);
static GRACE_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

pub fn set_status_callback(cb: impl Fn(TapStatusEvent) + Send + 'static) {
    *STATUS_CB.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(cb));
}

/// 返回最近一次旁路状态，供页面挂载时恢复显示。
pub fn last_status() -> Option<TapStatusEvent> {
    LAST_STATUS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub(super) fn status(state: TapState, msg: &str) {
    core_log::log_info(&format!("[hid-tap] {msg}"));
    let event = TapStatusEvent {
        state,
        message: msg.to_string(),
    };
    if let Ok(mut last) = LAST_STATUS.lock() {
        *last = Some(event.clone());
    }
    if let Ok(cb) = STATUS_CB.lock() {
        if let Some(f) = cb.as_ref() {
            f(event);
        }
    }
}

pub(super) fn arm_grace() {
    *GRACE_UNTIL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now() + INPUT_GRACE);
}

pub(super) fn in_grace() -> bool {
    let mut guard = GRACE_UNTIL.lock().unwrap_or_else(|e| e.into_inner());
    match *guard {
        Some(until) if Instant::now() < until => true,
        Some(_) => {
            *guard = None;
            false
        }
        None => false,
    }
}
