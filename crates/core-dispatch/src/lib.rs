//! core-dispatch — 按键调度器：物理按键 → 触发检测 → 映射解析 → 动作执行。
//!
//! 这是把「按键映射」从纯配置变成运行时闭环的核心一环：
//! Raw Input 标准流与 HOGP 旁路在 src-tauri 汇聚后调用
//! [`KeyDispatcher::on_vkey`]；调度器为每个按键维护一个
//! `TriggerDetector`（单击/双击/长按/按下/松开），确认触发后查
//! [`MappingConfig`](core_mapping::MappingConfig)，在独立执行线程上经
//! `core-input` 注入动作，并写入 `core-stats`。
//!
//! 麦克风键也走调度器：默认映射为 Press→Voice、Release→Voice
//! （`core_input::open_voice_typing`，Win+H）。用户可在映射页把麦克风
//! 的按下/松开改成任意动作（如第三方语音助手），映射表不是摆设。
//!
//! 重复投递防护分工：
//! - WM_APPCOMMAND 的合成按下/松开在 core-hid 源头抑制（键盘路径
//!   已上报过同一物理按键时跳过）；
//! - 同一按键按住期间的重复按下由 `down` 状态机忽略；
//! - 低层钩子与 Raw Input 的双路事件在 src-tauri 的转发漏斗去重，
//!   且只有 Raw Input（设备可辨）会喂给调度器。
//!
//! 线程模型：
//! - 事件源线程（Raw Input / 钩子）只调用 [`KeyDispatcher::on_vkey`]，
//!   做触发判定；
//! - 触发产生的动作经 mpsc 交给 [`KeyDispatcher::spawn_runtime`]
//!   启动的执行线程，避免在消息循环线程里做 SendInput / 启动进程；
//! - tick 线程每 25ms 驱动一次触发检测（确认延迟单击、长按重复）。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_config::KeyCalibration;
use core_mapping::trigger::{FeedOutcome, TriggerDetector};
use core_mapping::{ActionKind, ButtonId, MappingConfig, Trigger};

/// tick 线程轮询间隔。
const TICK_INTERVAL_MS: u64 = 25;

/// 单个按键的运行时状态。
#[derive(Default)]
struct ButtonRuntime {
    detector: TriggerDetector,
    down: bool,
    /// 本次按住期间是否已触发过长按。
    long_executed: bool,
}

struct Inner {
    mapping: MappingConfig,
    /// 虚拟键 -> 物理按键（默认表 + 校准表覆盖）。
    vkey_map: HashMap<u16, ButtonId>,
    buttons: HashMap<ButtonId, ButtonRuntime>,
    enabled: bool,
}

/// 一条待执行的动作任务。
#[derive(Debug, Clone)]
pub struct ActionJob {
    pub button: ButtonId,
    pub trigger: Trigger,
    pub action: ActionKind,
}

/// 按键调度器。构造后通过 [`KeyDispatcher::spawn_runtime`] 启动
/// 执行线程与 tick 线程；测试可直接调用 [`KeyDispatcher::feed`]
/// 注入合成时间戳，不触碰真实输入。
pub struct KeyDispatcher {
    start: Instant,
    inner: Mutex<Inner>,
    jobs_tx: mpsc::Sender<ActionJob>,
    jobs_rx: Mutex<Option<mpsc::Receiver<ActionJob>>>,
}

impl KeyDispatcher {
    /// 创建调度器（不启动任何线程）。
    pub fn new(
        mapping: MappingConfig,
        calibrations: &HashMap<String, KeyCalibration>,
    ) -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        Arc::new(Self {
            start: Instant::now(),
            inner: Mutex::new(Inner {
                vkey_map: build_vkey_map(calibrations),
                buttons: default_button_runtimes(),
                mapping,
                enabled: true,
            }),
            jobs_tx: tx,
            jobs_rx: Mutex::new(Some(rx)),
        })
    }

    /// 启动动作执行线程与 tick 线程。
    ///
    /// `stats_dir` 为按键统计存储目录（`core-stats`）。
    pub fn spawn_runtime(self: &Arc<Self>, stats_dir: PathBuf) {
        if let Some(rx) = self.jobs_rx.lock().unwrap().take() {
            std::thread::Builder::new()
                .name("rc003-dispatch-exec".into())
                .spawn(move || execute_loop(rx, stats_dir))
                .ok();
        }
        let weak = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("rc003-dispatch-tick".into())
            .spawn(move || {
                while let Some(dispatcher) = weak.upgrade() {
                    let now = dispatcher.now_ms();
                    for job in dispatcher.tick_once(now) {
                        let _ = dispatcher.jobs_tx.send(job);
                    }
                    std::thread::sleep(Duration::from_millis(TICK_INTERVAL_MS));
                }
            })
            .ok();
    }

    /// 事件入口：一个虚拟键的按下/松开。
    pub fn on_vkey(&self, vkey: u16, pressed: bool) {
        for job in self.feed(vkey, pressed, self.now_ms()) {
            let _ = self.jobs_tx.send(job);
        }
    }

    /// 热更新映射（保存映射后调用）。
    pub fn update_mapping(&self, mapping: MappingConfig) {
        self.inner.lock().unwrap().mapping = mapping;
    }

    /// 热更新校准表（保存校准后调用），并重建虚拟键反查表。
    pub fn update_calibrations(&self, calibrations: &HashMap<String, KeyCalibration>) {
        self.inner.lock().unwrap().vkey_map = build_vkey_map(calibrations);
    }

    /// 热更新触发判定时间：长按阈值与双击窗口（毫秒）。
    pub fn set_trigger_timing(&self, long_press_ms: u64, double_click_ms: u64) {
        let mut inner = self.inner.lock().unwrap();
        for rt in inner.buttons.values_mut() {
            rt.detector.set_long_press_ms(long_press_ms);
            rt.detector.set_double_click_window_ms(double_click_ms);
        }
    }

    /// 暂停/恢复调度。按键测试与校准界面应暂停，避免测试按键
    /// 触发真实动作。切换时清空所有按键的中间状态。
    ///
    /// 返回值指示状态是否真正发生了改变（若已处于该状态则返回 `false`）。
    pub fn set_enabled(&self, enabled: bool) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.enabled == enabled {
            return false;
        }
        inner.enabled = enabled;
        for rt in inner.buttons.values_mut() {
            *rt = ButtonRuntime::default();
        }
        true
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.lock().unwrap().enabled
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// 注入一个虚拟键事件，返回产生的任务（不发往执行线程）。
    fn feed(&self, vkey: u16, pressed: bool, now: u64) -> Vec<ActionJob> {
        let mut jobs = Vec::new();
        let mut inner = self.inner.lock().unwrap();
        if !inner.enabled {
            return jobs;
        }
        let Inner {
            mapping,
            vkey_map,
            buttons,
            ..
        } = &mut *inner;
        let Some(&button) = vkey_map.get(&vkey) else {
            return jobs;
        };
        let rt = buttons.entry(button).or_default();
        if pressed {
            if rt.down {
                // 系统按住重复：只算一次物理按下
                return jobs;
            }
            rt.down = true;
            rt.long_executed = false;
            rt.detector.press(now);
            // Press 边沿触发不在这里发：等 tick 识别为长按后才发，
            // 快速点按（单击/双击）不产生 Press/Release。
        } else {
            if !rt.down {
                // 没有对应按下的释放（被抑制的回声），直接忽略
                return jobs;
            }
            rt.down = false;
            // 先让检测器确认本次按住是否达到长按阈值（tick 漏掉时兜底）。
            let outcome = rt.detector.release(now);
            // Release 边沿触发：只有长按结束才发。
            if rt.detector.is_long_held() {
                if let Some(job) = build_job(mapping, button, Trigger::Release, rt) {
                    jobs.push(job);
                }
            }
            // 单击/双击/长按等手势确认。
            if let FeedOutcome::Fire(ev) = outcome {
                if let Some(job) = build_job(mapping, button, ev.trigger, rt) {
                    jobs.push(job);
                }
            }
        }
        jobs
    }

    /// 驱动一次触发检测（确认延迟的单击与长按）。
    fn tick_once(&self, now: u64) -> Vec<ActionJob> {
        let mut jobs = Vec::new();
        let mut inner = self.inner.lock().unwrap();
        if !inner.enabled {
            return jobs;
        }
        let Inner {
            mapping, buttons, ..
        } = &mut *inner;
        for (button, rt) in buttons.iter_mut() {
            let was_long = rt.detector.is_long_held();
            let outcome = rt.detector.tick(now);
            // 长按刚被识别：发 Press 边沿触发（麦克风 PTT 的「按下」）。
            if !was_long && rt.detector.is_long_held() {
                if let Some(job) = build_job(mapping, *button, Trigger::Press, rt) {
                    jobs.push(job);
                }
            }
            if let FeedOutcome::Fire(ev) = outcome {
                if let Some(job) = build_job(mapping, *button, ev.trigger, rt) {
                    jobs.push(job);
                }
            }
        }
        jobs
    }
}

/// 由触发事件构建动作任务；无绑定或禁用的按键返回 `None`。
///
/// 长按只执行一次：tick 已触发过则松开时的兜底确认直接跳过。
fn build_job(
    mapping: &MappingConfig,
    button: ButtonId,
    trigger: Trigger,
    rt: &mut ButtonRuntime,
) -> Option<ActionJob> {
    let action = mapping.resolve(button, trigger)?.clone();
    if matches!(action, ActionKind::Disabled) {
        return None;
    }
    if trigger == Trigger::LongPress {
        if rt.long_executed {
            return None;
        }
        rt.long_executed = true;
    }
    Some(ActionJob {
        button,
        trigger,
        action,
    })
}

/// 构建虚拟键 -> 物理按键反查表。
///
/// 默认项来自 core-hid 的 usage 表（含麦克风 F5 兜底 116），随后应用
/// 校准表覆盖（校准表里 `vkey` 非空的条目优先）。
fn build_vkey_map(calibrations: &HashMap<String, KeyCalibration>) -> HashMap<u16, ButtonId> {
    let mut map = HashMap::new();
    for button in ButtonId::ALL {
        if let Some(usage) = core_hid::button_to_usage(button) {
            if let Some(vk) = core_hid::usage_to_vkey(usage) {
                map.insert(vk, button);
            }
        }
    }
    for cal in calibrations.values() {
        let Some(vk) = cal.vkey else { continue };
        let Ok(vk) = u16::try_from(vk) else { continue };
        let Some(button) = ButtonId::ALL
            .iter()
            .copied()
            .find(|b| b.key() == cal.button)
        else {
            continue;
        };
        // 移除该按钮的默认虚拟键绑定，避免同一按钮由两个键触发；
        // 校准值之间冲突时后写的优先。
        map.retain(|_, b| *b != button);
        map.insert(vk, button);
    }
    map
}

fn default_button_runtimes() -> HashMap<ButtonId, ButtonRuntime> {
    ButtonId::ALL
        .iter()
        .copied()
        .map(|b| (b, ButtonRuntime::default()))
        .collect()
}

/// 执行线程主体：执行动作并记录按键统计。
fn execute_loop(rx: mpsc::Receiver<ActionJob>, stats_dir: PathBuf) {
    let stats = core_stats::StatsStore::new(stats_dir).ok();
    while let Ok(job) = rx.recv() {
        let outcome = execute_action(&job.action);
        match &outcome {
            Ok(()) => core_log::log_info(&format!(
                "[dispatch] 已执行: {} {:?} -> {:?}",
                job.button.display_name(),
                job.trigger,
                job.action
            )),
            Err(e) => core_log::log_error(&format!(
                "[dispatch] 执行失败: {} {:?} -> {:?}: {e}",
                job.button.display_name(),
                job.trigger,
                job.action
            )),
        }
        if outcome.is_ok() {
            if let Some(stats) = &stats {
                let _ = stats.record_key(job.button.key());
            }
        }
    }
}

fn send_combo(tokens: &[&str]) -> Result<(), String> {
    core_input::send_key_combo(tokens).map_err(|e| e.to_string())
}

fn execute_action(action: &ActionKind) -> Result<(), String> {
    use ActionKind as A;
    match action {
        A::Disabled => Ok(()),
        A::KeyCombo(tokens) => {
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            send_combo(&refs)
        }
        A::Escape => core_input::press_escape().map_err(|e| e.to_string()),
        A::Return => send_combo(&["enter"]),
        A::ArrowUp => send_combo(&["up"]),
        A::ArrowDown => send_combo(&["down"]),
        A::ArrowLeft => send_combo(&["left"]),
        A::ArrowRight => send_combo(&["right"]),
        A::DeleteBackward => send_combo(&["backspace"]),
        A::ShowDesktop => send_combo(&["win", "d"]),
        A::ContextMenu => send_combo(&["apps"]),
        A::AppSwitcher => send_combo(&["alt", "tab"]),
        A::SystemVolumeUp => send_combo(&["volume_up"]),
        A::SystemVolumeDown => send_combo(&["volume_down"]),
        A::SystemVolumeMute => send_combo(&["volume_mute"]),
        A::PlayPause => send_combo(&["play_pause"]),
        A::Voice => core_input::open_voice_typing()
            .map(|_| ())
            .map_err(|e| e.to_string()),
        A::OpenApp(name) => core_input::open_app(name).map_err(|e| e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_mapping::default_mapping;

    fn dispatcher() -> Arc<KeyDispatcher> {
        KeyDispatcher::new(MappingConfig::default(), &HashMap::new())
    }

    fn jobs_of(dispatcher: &KeyDispatcher, vkey: u16, pressed: bool, now: u64) -> Vec<ActionJob> {
        dispatcher.feed(vkey, pressed, now)
    }

    #[test]
    fn vkey_map_defaults_including_mic() {
        let map = build_vkey_map(&HashMap::new());
        assert_eq!(map.get(&38), Some(&ButtonId::Up));
        assert_eq!(map.get(&166), Some(&ButtonId::Back));
        assert_eq!(map.get(&175), Some(&ButtonId::VolumeUp));
        assert_eq!(map.get(&174), Some(&ButtonId::VolumeDown));
        // 主页：实测 Windows 映射为 VK_HOME(36)
        assert_eq!(map.get(&36), Some(&ButtonId::Home));
        assert!(
            !map.contains_key(&172),
            "主页不应再绑定 VK_BROWSER_HOME(172)"
        );
        // 麦克风 F5 兜底 116 进调度器，走 Press/Release 映射
        assert_eq!(map.get(&116), Some(&ButtonId::Mic));
    }

    #[test]
    fn calibration_overrides_vkey() {
        let mut cals = HashMap::new();
        cals.insert(
            "up".to_string(),
            KeyCalibration {
                button: "up".to_string(),
                code: "KeyY".to_string(),
                key: "y".to_string(),
                vkey: Some(89),
            },
        );
        let map = build_vkey_map(&cals);
        assert_eq!(map.get(&89), Some(&ButtonId::Up));
        assert_eq!(map.get(&38), None, "默认虚拟键应被覆盖");
    }

    #[test]
    fn single_click_confirmed_after_double_click_window() {
        let d = dispatcher();
        assert!(jobs_of(&d, 38, true, 0).is_empty());
        assert!(jobs_of(&d, 38, false, 50).is_empty());
        let jobs = d.tick_once(400);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].trigger, Trigger::SingleClick);
        assert_eq!(jobs[0].action, ActionKind::ArrowUp);
    }

    #[test]
    fn double_click_fires_on_second_release() {
        let d = dispatcher();
        d.update_mapping(MappingConfig {
            bindings: vec![core_mapping::KeyBinding {
                button: ButtonId::Up,
                trigger: Trigger::DoubleClick,
                action: ActionKind::Return,
            }],
        });
        jobs_of(&d, 38, true, 0);
        jobs_of(&d, 38, false, 50);
        jobs_of(&d, 38, true, 100);
        let jobs = jobs_of(&d, 38, false, 150);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].trigger, Trigger::DoubleClick);
        assert_eq!(jobs[0].action, ActionKind::Return);
    }

    #[test]
    fn key_repeat_press_is_ignored() {
        let d = dispatcher();
        assert!(jobs_of(&d, 38, true, 0).is_empty());
        // 按住期间系统重复的按下事件不应重置触发状态
        assert!(jobs_of(&d, 38, true, 100).is_empty());
        assert!(
            jobs_of(&d, 38, false, 150).is_empty(),
            "正常松开产生待确认单击"
        );
        let _ = d.tick_once(500);
    }

    #[test]
    fn spurious_release_is_ignored() {
        let d = dispatcher();
        // 没有按下的释放（回声）不应进入触发状态机
        assert!(jobs_of(&d, 38, false, 0).is_empty());
        assert!(jobs_of(&d, 38, true, 100).is_empty());
        jobs_of(&d, 38, false, 150);
        // 释放时 held=50ms，单击在双击窗口后确认
        let jobs = d.tick_once(500);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].trigger, Trigger::SingleClick);
    }

    #[test]
    fn long_press_fires_on_hold_and_release_does_not_double_fire() {
        let d = dispatcher();
        d.update_mapping(MappingConfig {
            bindings: vec![core_mapping::KeyBinding {
                button: ButtonId::Back,
                trigger: Trigger::LongPress,
                action: ActionKind::OpenApp("notepad".into()),
            }],
        });
        jobs_of(&d, 166, true, 0);
        // 按住 600ms：tick 的第一个长按节拍触发（不可重复动作只此一次）
        let jobs = d.tick_once(600);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].trigger, Trigger::LongPress);
        assert!(d.tick_once(750).is_empty());
        // 松开时的长按确认不应重复执行
        assert!(jobs_of(&d, 166, false, 800).is_empty());
    }

    #[test]
    fn long_press_fires_only_once() {
        let d = dispatcher();
        d.update_mapping(MappingConfig {
            bindings: vec![core_mapping::KeyBinding {
                button: ButtonId::VolumeUp,
                trigger: Trigger::LongPress,
                action: ActionKind::SystemVolumeUp,
            }],
        });
        jobs_of(&d, 175, true, 0);
        assert_eq!(d.tick_once(600).len(), 1, "按住超过阈值触发一次");
        assert!(d.tick_once(750).is_empty(), "继续按住不再重复");
        assert!(jobs_of(&d, 175, false, 800).is_empty(), "松开确认不重复");
    }

    #[test]
    fn disabled_action_produces_no_job() {
        let d = dispatcher();
        d.update_mapping(MappingConfig {
            bindings: vec![core_mapping::KeyBinding {
                button: ButtonId::Up,
                trigger: Trigger::SingleClick,
                action: ActionKind::Disabled,
            }],
        });
        jobs_of(&d, 38, true, 0);
        jobs_of(&d, 38, false, 50);
        assert!(d.tick_once(400).is_empty());
    }

    #[test]
    fn set_enabled_resets_and_blocks() {
        let d = dispatcher();
        d.set_enabled(false);
        assert!(jobs_of(&d, 38, true, 0).is_empty());
        assert!(jobs_of(&d, 38, false, 50).is_empty());
        assert!(d.tick_once(400).is_empty());
        d.set_enabled(true);
        assert!(d.is_enabled());
        // 重新启用后状态应已重置：立即再次按下可用
        assert!(jobs_of(&d, 38, true, 500).is_empty());
    }

    #[test]
    fn update_mapping_takes_effect_immediately() {
        let d = dispatcher();
        d.update_mapping(MappingConfig {
            bindings: vec![core_mapping::KeyBinding {
                button: ButtonId::VolumeUp,
                trigger: Trigger::SingleClick,
                action: ActionKind::SystemVolumeMute,
            }],
        });
        jobs_of(&d, 175, true, 0);
        jobs_of(&d, 175, false, 50);
        let jobs = d.tick_once(400);
        assert_eq!(jobs[0].action, ActionKind::SystemVolumeMute);
    }

    #[test]
    fn all_default_buttons_have_binding() {
        let map = build_vkey_map(&HashMap::new());
        let cfg = MappingConfig::default();
        assert_eq!(map.len(), 13);
        for (vk, button) in map {
            assert!(
                cfg.bindings.iter().any(|b| b.button == button),
                "vkey {vk} ({button:?}) 缺少默认映射"
            );
        }
        assert_eq!(default_mapping().len(), 14);
    }

    #[test]
    fn mic_ptt_fires_after_long_press() {
        let d = dispatcher();
        assert!(jobs_of(&d, 116, true, 0).is_empty(), "按下瞬间不发 Press");
        let press_jobs = d.tick_once(600);
        assert_eq!(press_jobs.len(), 1);
        assert_eq!(press_jobs[0].trigger, Trigger::Press);
        assert_eq!(press_jobs[0].action, ActionKind::Voice);

        let release_jobs = jobs_of(&d, 116, false, 800);
        assert_eq!(release_jobs.len(), 1);
        assert_eq!(release_jobs[0].trigger, Trigger::Release);
        assert_eq!(release_jobs[0].action, ActionKind::Voice);
    }

    #[test]
    fn mic_quick_tap_fires_nothing() {
        let d = dispatcher();
        assert!(jobs_of(&d, 116, true, 0).is_empty());
        assert!(
            jobs_of(&d, 116, false, 100).is_empty(),
            "快速点按不产生 Press/Release"
        );
        assert!(d.tick_once(400).is_empty());
    }

    #[test]
    fn vkeys_never_collide() {
        let map = build_vkey_map(&HashMap::new());
        let mut seen = std::collections::HashSet::new();
        for vk in map.keys() {
            assert!(seen.insert(*vk), "虚拟键 {vk} 被映射到多个按键");
        }
    }
}
