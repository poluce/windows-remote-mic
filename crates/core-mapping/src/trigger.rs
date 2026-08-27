//! Trigger detection: single / double / long-press, plus hold-repeat.

use serde::{Deserialize, Serialize};

use crate::Trigger;

/// Timing constants (ms), aligned with common remote conventions.
pub const DOUBLE_CLICK_WINDOW_MS: u64 = 300;
pub const LONG_PRESS_MS: u64 = 550;
/// After long-press triggers, this is the interval for hold-repeat.
pub const HOLD_REPEAT_MS: u64 = 120;

/// An event produced by the trigger detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerEvent {
    pub trigger: Trigger,
}

/// Detects click triggers for one physical button.
///
/// Feed `press(now_ms)` and `release(now_ms)`; it emits triggers as they
/// are confirmed (single click is delayed until the double-click window
/// elapses).
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
    /// No confirmed trigger yet.
    Pending,
    /// A trigger was confirmed and should fire now.
    Fire(TriggerEvent),
}

impl TriggerDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// A physical press happened.
    pub fn press(&mut self, now_ms: u64) {
        self.pressed_at = Some(now_ms);
        self.long_fired = false;
        self.repeat_armed = true;
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
                return FeedOutcome::Fire(TriggerEvent {
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

        // Second click: if within the window, fire double click.
        if let Some(first_at) = self.last_single_at.take() {
            if now_ms.saturating_sub(first_at) <= self.double_click_window_ms {
                return FeedOutcome::Fire(TriggerEvent {
                    trigger: Trigger::DoubleClick,
                });
            }
        }

        // Double-click window expired: remember this release as a new single.
        self.last_single_at = Some(now_ms);
        FeedOutcome::Pending
    }

    /// Periodic tick to check for hold-repeat and expired single-clicks.
    pub fn tick(&mut self, now_ms: u64) -> FeedOutcome {
        // While held past long-press threshold: emit repeat ticks.
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

        // Double-click window expired: confirm the single click.
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

    /// Reset all state (e.g. on device disconnect).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Test helper: force-confirm any pending single click immediately.
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
