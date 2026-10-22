//! 单次语音桥会话的统计快照（供外层重连循环做健康判断）。

use std::sync::Mutex;

/// 单次语音桥会话结束时留存的统计。
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionReport {
    /// 本会话存活时长（秒）。
    pub duration_secs: f32,
    /// 收到的控制通知包数（正常会话应 >0：至少 CAPS 响应）。
    pub control_pkts: u64,
    /// 收到的音频包数（只有按住说话时才 >0）。
    pub audio_pkts: u64,
    /// 本次会话退出原因。
    pub exit_reason: &'static str,
}

static LAST_SESSION_REPORT: Mutex<Option<SessionReport>> = Mutex::new(None);

/// 最近一次语音桥会话的统计快照。
pub fn last_session_report() -> Option<SessionReport> {
    match LAST_SESSION_REPORT.lock() {
        Ok(g) => *g,
        Err(e) => *e.into_inner(),
    }
}

/// 会话结束时写入快照（供监督线程判断「链路通但零数据」）。
pub(crate) fn store_report(report: SessionReport) {
    match LAST_SESSION_REPORT.lock() {
        Ok(mut guard) => *guard = Some(report),
        Err(e) => *e.into_inner() = Some(report),
    }
}
