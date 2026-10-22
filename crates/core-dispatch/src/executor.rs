//! 动作执行线程：从队列取 `ActionJob`，调用 core-input / 应用事件出口。
//!
//! 与调度判定（`KeyDispatcher::on_vkey`）分离：事件源线程只做触发判定，
//! 这里在独立线程执行 SendInput / 启动进程等较慢动作。

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use core_mapping::{ActionKind, Trigger, VoiceTarget};

use crate::{ActionJob, AppEvent, AppEventHandler};

pub(crate) fn execute_loop(
    rx: mpsc::Receiver<ActionJob>,
    stats_dir: PathBuf,
    app_event: Option<AppEventHandler>,
    voice_target: Arc<Mutex<VoiceTarget>>,
) {
    let stats = core_stats::StatsStore::new(stats_dir).ok();
    while let Ok(job) = rx.recv() {
        // 单个动作执行 panic 不能带走执行线程（否则后续所有按键静默失效）。
        let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute_action(&job, app_event.as_ref(), &voice_target)
        })) {
            Ok(outcome) => outcome,
            Err(payload) => Err(format!(
                "动作执行 panic 已隔离：{}",
                core_hid::panic_payload_text(payload.as_ref())
            )),
        };
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

pub(crate) fn send_combo(tokens: &[&str]) -> Result<(), String> {
    core_input::send_key_combo(tokens).map_err(|e| e.to_string())
}

/// Voice 动作要调用的底层语音原语（由触发边沿决定，与识别目标无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoicePrimitive {
    /// PTT 按住说话开始（麦克风长按识别后的 Press）。
    Press,
    /// PTT 松手收尾（麦克风长按结束的 Release）。
    Release,
    /// 单次 Tap：开启/重置一次语音会话（非 PTT 边沿）。
    Open,
}

/// 由触发边沿决定 Voice 动作走哪个原语。
pub(crate) fn voice_primitive(trigger: Trigger) -> VoicePrimitive {
    match trigger {
        Trigger::Press => VoicePrimitive::Press,
        Trigger::Release => VoicePrimitive::Release,
        _ => VoicePrimitive::Open,
    }
}

pub(crate) fn execute_action(
    job: &ActionJob,
    app_event: Option<&AppEventHandler>,
    voice_target: &Arc<Mutex<VoiceTarget>>,
) -> Result<(), String> {
    use ActionKind as A;
    let action = &job.action;
    match action {
        A::Disabled => Ok(()),
        A::KeyCombo(tokens) => {
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            // 麦克风按下/松开要保持按住说话：Press 只按下，Release 只松开。
            // 单击/双击/长按仍是一次完整点按。
            match job.trigger {
                Trigger::Press => core_input::send_key_down(&refs).map_err(|e| e.to_string()),
                Trigger::Release => core_input::send_key_up(&refs).map_err(|e| e.to_string()),
                _ => send_combo(&refs),
            }
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
        A::Voice => {
            // 语音动作按「识别目标 + 触发边沿」分发：
            // - Press/Release（麦克风 PTT）→ 按住说话/松手收尾；
            // - 单击/双击/长按等非 PTT 边沿 → Tap（开启/重置一次会话）。
            let target = *voice_target.lock().unwrap_or_else(|e| e.into_inner());
            match voice_primitive(job.trigger) {
                VoicePrimitive::Press => core_input::voice_press(target)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                VoicePrimitive::Release => core_input::voice_release(target)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                VoicePrimitive::Open => core_input::voice_open(target)
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            }
        }
        A::FocusInput => match core_input::focus_foreground_input() {
            Ok(core_input::FocusOutcome::Focused { name }) => {
                core_log::log_line(&format!("[dispatch] 已把焦点送进输入框：{name}"));
                Ok(())
            }
            Ok(core_input::FocusOutcome::AlreadyFocused { name }) => {
                core_log::log_line(&format!("[dispatch] 输入框已有焦点，不重复聚焦：{name}"));
                Ok(())
            }
            Ok(core_input::FocusOutcome::NoInputFound) => {
                core_log::log_line("[dispatch] 前台窗口里没有可聚焦的输入框");
                Ok(())
            }
            Err(e) => Err(e.to_string()),
        },
        // 确定键的顺手版本：光标不在输入框就送进去（接下来用户要说话），
        // 已经在里面、或压根找不到输入框，就按原来的行为发回车。
        A::FocusInputOrSubmit => match core_input::focus_foreground_input() {
            Ok(core_input::FocusOutcome::Focused { name }) => {
                core_log::log_line(&format!("[dispatch] 已把焦点送进输入框：{name}"));
                Ok(())
            }
            Ok(core_input::FocusOutcome::AlreadyFocused { name }) => {
                core_log::log_line(&format!("[dispatch] 输入框已有焦点，回车发送：{name}"));
                send_combo(&["enter"])
            }
            Ok(core_input::FocusOutcome::NoInputFound) => {
                core_log::log_line("[dispatch] 没有可聚焦的输入框，按回车处理");
                send_combo(&["enter"])
            }
            Err(e) => Err(e.to_string()),
        },
        A::OpenApp(name) => core_input::open_app(name).map_err(|e| e.to_string()),
        A::ToggleQuickMenu => match app_event {
            Some(handler) => {
                handler(AppEvent::ToggleQuickMenu);
                Ok(())
            }
            None => Err("快捷菜单动作未接线（缺少应用事件出口）".to_string()),
        },
    }
}
