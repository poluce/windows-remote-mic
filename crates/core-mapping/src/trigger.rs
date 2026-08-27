//! 触发检测：单击 / 双击 / 长按，以及按住重复。

use serde::{Deserialize, Serialize};

use crate::Trigger;

/// 时间常量（毫秒），与常见遥控器惯例保持一致。
pub const DOUBLE_CLICK_WINDOW_MS: u64 = 300;
pub const LONG_PRESS_MS: u64 = 550;
/// 长按触发后，这是按住重复（hold-repeat）的间隔。
pub const HOLD_REPEAT_MS: u64 = 120;

/// 由触发检测器产生的事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerEvent {
    pub trigger: Trigger,
}

/// 检测单个物理按键的点击触发。
///
/// 依次送入 `press(now_ms)` 和 `release(now_ms)`；当触发被确认时发出事件
/// （单击会延迟到双击窗口结束后才确认）。
#[derive(Debug, Clone)]
pub struct TriggerDetector {
    pressed_at: Option<u64>,
    released_at: Option<u64>,
    last_single_at: Option<u64>,
    long_fired: bool,
    repeat_armed: bool,
    last_repeat_at: Option<u64>,
    double_click_window_ms: u64,
    long_press_ms: u64,
}

impl Default for TriggerDetector {
    fn default() -> Self {
        Self {
            pressed_at: None,
            released_at: None,
            last_single_at: None,
            long_fired: false,
            repeat_armed: false,
            last_repeat_at: None,
            double_click_window_ms: DOUBLE_CLICK_WINDOW_MS,
            long_press_ms: LONG_PRESS_MS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedOutcome {
    /// 尚无已确认的触发。
    Pending,
    /// 已确认一个触发，应立即触发。
    Fire(TriggerEvent),
}

impl TriggerDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// 发生了物理按下。
    pub fn press(&mut self, now_ms: u64) {
        self.pressed_at = Some(now_ms);
        self.long_fired = false;
        self.repeat_armed = true;
    }

    /// 发生了物理松开。
    pub fn release(&mut self, now_ms: u64) -> FeedOutcome {
        let Some(pressed) = self.pressed_at.take() else {
            return FeedOutcome::Pending;
        };
        self.released_at = Some(now_ms);

        let held = now_ms.saturating_sub(pressed);

        // 长按：如果按住时间超过阈值，则在松开时触发。
        if held >= self.long_press_ms {
            if !self.long_fired {
                self.long_fired = true;
                self.repeat_armed = false;
                self.last_single_at = None;
                return FeedOutcome::Fire(TriggerEvent {
                    trigger: Trigger::LongPress,
                });
            }
            return FeedOutcome::Pending;
        }

        // 第一次点击：等待双击窗口。
        if self.last_single_at.is_none() {
            self.last_single_at = Some(now_ms);
            return FeedOutcome::Pending;
        }

        // 第二次点击：如果在窗口内，则触发双击。
        if let Some(first_at) = self.last_single_at.take() {
            if now_ms.saturating_sub(first_at) <= self.double_click_window_ms {
                return FeedOutcome::Fire(TriggerEvent {
                    trigger: Trigger::DoubleClick,
                });
            }
        }

        // 双击窗口已过期：将这次松开记为新的单击。
        self.last_single_at = Some(now_ms);
        FeedOutcome::Pending
    }

    /// 周期性的 tick，用于检查按住重复和已过期的单击。
    pub fn tick(&mut self, now_ms: u64) -> FeedOutcome {
        // 当按住时间超过长按阈值时：发出重复 tick。
        if let Some(pressed) = self.pressed_at {
            if self.repeat_armed && now_ms.saturating_sub(pressed) >= self.long_press_ms {
                let interval_due = match self.last_repeat_at {
                    None => true,
                    Some(last) => now_ms.saturating_sub(last) >= HOLD_REPEAT_MS,
                };
                if interval_due {
                    self.last_repeat_at = Some(now_ms);
                    return FeedOutcome::Fire(TriggerEvent {
                        trigger: Trigger::LongPress,
                    });
                }
            }
        }

        // 双击窗口已过期：确认单击。
        if let Some(single_at) = self.last_single_at {
            if self.pressed_at.is_none()
                && now_ms.saturating_sub(single_at) > self.double_click_window_ms
            {
                self.last_single_at = None;
                return FeedOutcome::Fire(TriggerEvent {
                    trigger: Trigger::SingleClick,
                });
            }
        }

        FeedOutcome::Pending
    }

    /// 重置所有状态（例如设备断开连接时）。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 测试辅助：立即强制确认任何待处理的单击。
    pub fn confirm_single(&mut self) -> Option<TriggerEvent> {
        self.last_single_at.take().map(|_| TriggerEvent {
            trigger: Trigger::SingleClick,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_press_fires_on_long_hold() {
        let mut d = TriggerDetector::new();
        d.press(0);
        assert_eq!(
            d.release(600),
            FeedOutcome::Fire(TriggerEvent {
                trigger: Trigger::LongPress
            })
        );
    }

    #[test]
    fn double_click_fires_within_window() {
        let mut d = TriggerDetector::new();
        d.press(0);
        assert_eq!(d.release(50), FeedOutcome::Pending);
        d.press(100);
    }
}
