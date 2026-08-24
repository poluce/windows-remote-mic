//! Gesture detection: single / double / long-press, plus hold-repeat.

use serde::{Deserialize, Serialize};

use crate::Trigger;

/// Timing constants (ms), aligned with common remote conventions.
pub const DOUBLE_CLICK_WINDOW_MS: u64 = 300;
pub const LONG_PRESS_MS: u64 = 550;
/// After long-press triggers, this is the interval for hold-repeat.
pub const HOLD_REPEAT_MS: u64 = 120;

/// An event produced by the gesture detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GestureEvent {
    pub trigger: Trigger,
}

/// Detects click gestures for one physical button.
///
/// Feed `press(now_ms)` and `release(now_ms)`; it emits triggers as they
/// are confirmed (single click is delayed until the double-click window
/// elapses).
#[derive(Debug, Clone)]
pub struct GestureDetector {
    pressed_at: Option<u64>,
    released_at: Option<u64>,
    last_single_at: Option<u64>,
    long_fired: bool,
    repeat_armed: bool,
    last_repeat_at: Option<u64>,
    double_click_window_ms: u64,
    long_press_ms: u64,
}

impl Default for GestureDetector {
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
    /// No confirmed gesture yet.
    Pending,
    /// A gesture was confirmed and should fire now.
    Fire(GestureEvent),
}

impl GestureDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// A physical press happened.
    pub fn press(&mut self, now_ms: u64) {
        self.pressed_at = Some(now_ms);
        self.long_fired = false;
        self.repeat_armed = true;
        // A new press cancels a not-yet-confirmed double click.
        if let Some(last) = self.last_single_at {
            if now_ms.saturating_sub(last) > self.double_click_window_ms {
                self.last_single_at = None;
            }
        }
    }

    /// A physical release happened.
    pub fn release(&mut self, now_ms: u64) -> FeedOutcome {
        let Some(pressed) = self.pressed_at.take() else {
            return FeedOutcome::Pending;
        };
        self.released_at = Some(now_ms);

        let held = now_ms.saturating_sub(pressed);

        // Long press: fired on release if the press exceeded the threshold.
        if held >= self.long_press_ms {
            if !self.long_fired {
                self.long_fired = true;
                self.repeat_armed = false;
                self.last_single_at = None;
                return FeedOutcome::Fire(GestureEvent {
                    trigger: Trigger::LongPress,
                });
            }
            return FeedOutcome::Pending;
        }

        // First click: wait for the double-click window.
        if self.last_single_at.is_none() {
            self.last_single_at = Some(now_ms);
            return FeedOutcome::Pending;
        }

        // Second click within window -> double click.
        if now_ms.saturating_sub(self.last_single_at.unwrap()) <= self.double_click_window_ms {
            self.last_single_at = None;
            self.repeat_armed = false;
            return FeedOutcome::Fire(GestureEvent {
                trigger: Trigger::DoubleClick,
            });
        }

        self.last_single_at = Some(now_ms);
        FeedOutcome::Pending
    }

    /// Call repeatedly while a key is held to emit hold-repeat long presses.
    /// Returns true when a repeat should fire.
    pub fn poll_hold_repeat(&mut self, now_ms: u64) -> bool {
        let Some(pressed) = self.pressed_at else {
            return false;
        };
        if !self.repeat_armed {
            return false;
        }
        if now_ms.saturating_sub(pressed) < self.long_press_ms {
            return false;
        }
        let since_last = self
            .last_repeat_at
            .map(|t| now_ms.saturating_sub(t))
            .unwrap_or(u64::MAX);
        if since_last >= HOLD_REPEAT_MS {
            self.last_repeat_at = Some(now_ms);
            return true;
        }
        false
    }

    /// Confirm a pending single click (used at the double-click window end,
    /// or by an external timer).
    pub fn confirm_single(&mut self) -> Option<GestureEvent> {
        self.last_single_at.take().map(|_| GestureEvent {
            trigger: Trigger::SingleClick,
        })
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_press_fires_long_and_no_repeat_after_release() {
        let mut d = GestureDetector::new();
        d.press(0);
        assert_eq!(d.release(600), FeedOutcome::Fire(GestureEvent { trigger: Trigger::LongPress }));
        assert!(!d.poll_hold_repeat(700));
    }

    #[test]
    fn double_click_detected() {
        let mut d = GestureDetector::new();
        d.press(0);
        assert_eq!(d.release(50), FeedOutcome::Pending);
        d.press(100);
        assert_eq!(d.release(150), FeedOutcome::Fire(GestureEvent { trigger: Trigger::DoubleClick }));
    }

    #[test]
    fn hold_repeat_emits_after_long_press() {
        let mut d = GestureDetector::new();
        d.press(0);
        let mut fired = 0;
        // poll until a repeat would fire
        for t in (LONG_PRESS_MS..LONG_PRESS_MS + HOLD_REPEAT_MS * 2 + 10).step_by(10) {
            if d.poll_hold_repeat(t) {
                fired += 1;
            }
        }
        assert!(fired >= 1);
    }
}
