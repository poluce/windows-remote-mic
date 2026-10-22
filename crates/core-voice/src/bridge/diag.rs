//! 单次语音桥会话的断链诊断快照（活动时间戳 / 包计数）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// 会话期间的诊断状态：所有回调共享一份，主循环退出时输出 summary。
pub(crate) struct BridgeDiag {
    pub(crate) started_at: Instant,
    last_control_at: Mutex<Option<Instant>>,
    last_audio_at: Mutex<Option<Instant>>,
    last_mic_at: Mutex<Option<Instant>>,
    pub(crate) control_pkts: AtomicU64,
    pub(crate) audio_pkts: AtomicU64,
    pub(crate) voice_active: AtomicBool,
}

impl BridgeDiag {
    pub(crate) fn new() -> Self {
        Self {
            started_at: Instant::now(),
            last_control_at: Mutex::new(None),
            last_audio_at: Mutex::new(None),
            last_mic_at: Mutex::new(None),
            control_pkts: AtomicU64::new(0),
            audio_pkts: AtomicU64::new(0),
            voice_active: AtomicBool::new(false),
        }
    }

    pub(crate) fn touch_control(&self) {
        self.control_pkts.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut g) = self.last_control_at.lock() {
            *g = Some(Instant::now());
        }
    }

    pub(crate) fn touch_audio(&self) {
        self.audio_pkts.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut g) = self.last_audio_at.lock() {
            *g = Some(Instant::now());
        }
    }

    pub(crate) fn touch_mic(&self) {
        if let Ok(mut g) = self.last_mic_at.lock() {
            *g = Some(Instant::now());
        }
    }

    pub(crate) fn set_voice_active(&self, active: bool) {
        self.voice_active.store(active, Ordering::Relaxed);
    }

    pub(crate) fn last_control_at(&self) -> Option<Instant> {
        self.last_control_at.lock().ok().and_then(|g| *g)
    }

    pub(crate) fn last_audio_at(&self) -> Option<Instant> {
        self.last_audio_at.lock().ok().and_then(|g| *g)
    }

    pub(crate) fn age_label(since: Option<Instant>) -> String {
        match since {
            Some(t) => format!("{:.1}s", t.elapsed().as_secs_f32()),
            None => "从未".into(),
        }
    }

    /// 单行会话诊断（断链/停止时写 INFO）。
    pub(crate) fn summary(&self, reason: &str) -> String {
        let last_control = self.last_control_at();
        let last_audio = self.last_audio_at();
        let last_mic = self.last_mic_at.lock().ok().and_then(|g| *g);
        format!(
            "[bridge] 断链诊断: reason={reason}, 会话时长={:.1}s, 距上次控制通知={}, 距上次音频={}, 距上次麦克风事件={}, 控制包={}, 音频包={}, 语音输入中={}, session_active={}",
            self.started_at.elapsed().as_secs_f32(),
            Self::age_label(last_control),
            Self::age_label(last_audio),
            Self::age_label(last_mic),
            self.control_pkts.load(Ordering::Relaxed),
            self.audio_pkts.load(Ordering::Relaxed),
            self.voice_active.load(Ordering::Relaxed),
            crate::bridge_session_active(),
        )
    }
}
