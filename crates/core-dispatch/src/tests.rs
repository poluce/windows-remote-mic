use crate::executor::{execute_action, voice_primitive, VoicePrimitive};

use super::*;
use core_mapping::default_mapping;

fn dispatcher() -> Arc<KeyDispatcher> {
    KeyDispatcher::new(MappingConfig::default(), &HashMap::new())
}

/// 构造一份只覆盖指定绑定的应用配置。
fn profile_with(bindings: Vec<core_mapping::KeyBinding>) -> AppProfile {
    AppProfile {
        process: core_app_profile::ProcessSpec::One("Codex.exe".into()),
        name: "测试应用".into(),
        bindings,
        ..Default::default()
    }
}

fn binding(button: ButtonId, trigger: Trigger, action: ActionKind) -> core_mapping::KeyBinding {
    core_mapping::KeyBinding {
        button,
        trigger,
        action,
    }
}

fn jobs_of(dispatcher: &KeyDispatcher, vkey: u16, pressed: bool, now: u64) -> Vec<ActionJob> {
    dispatcher.feed(vkey, pressed, now)
}

/// 抽干直接投进执行队列的任务（`release_all_held` 这类兜底路径）。
/// 测试不会调 `spawn_runtime`，接收端还在自己手里。
fn drain_jobs(dispatcher: &KeyDispatcher) -> Vec<ActionJob> {
    let guard = dispatcher.jobs_rx.lock().unwrap_or_else(|e| e.into_inner());
    let rx = guard.as_ref().expect("测试中执行队列接收端仍在");
    let mut jobs = Vec::new();
    while let Ok(job) = rx.try_recv() {
        jobs.push(job);
    }
    jobs
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
fn single_click_fires_immediately_without_double_click_binding() {
    let d = dispatcher();
    assert!(jobs_of(&d, 38, true, 0).is_empty());
    // 默认映射没有双击绑定：松开立即确认单击，不再等双击窗口。
    let jobs = jobs_of(&d, 38, false, 50);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].trigger, Trigger::SingleClick);
    assert_eq!(jobs[0].action, ActionKind::ArrowUp);
    assert!(d.tick_once(400).is_empty(), "单击已确认，不应再延迟触发");
}

#[test]
fn double_click_binding_keeps_single_click_delayed() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(ButtonId::Up, Trigger::SingleClick, ActionKind::ArrowUp),
            binding(ButtonId::Up, Trigger::DoubleClick, ActionKind::Return),
        ],
    });
    assert!(jobs_of(&d, 38, true, 0).is_empty());
    assert!(
        jobs_of(&d, 38, false, 50).is_empty(),
        "有双击绑定时单击必须等窗口确认"
    );
    let jobs = d.tick_once(400);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].trigger, Trigger::SingleClick);
    assert_eq!(jobs[0].action, ActionKind::ArrowUp);
}

#[test]
fn rapid_presses_all_fire_without_double_click_binding() {
    let d = dispatcher();
    let mut fired = Vec::new();
    for i in 0..4u64 {
        let at = i * 150; // 150ms 间隔，远小于双击窗口
        fired.extend(jobs_of(&d, 38, true, at));
        fired.extend(jobs_of(&d, 38, false, at + 60));
    }
    assert_eq!(fired.len(), 4, "没有双击绑定时快速连按应全部生效");
    assert!(fired
        .iter()
        .all(|j| j.trigger == Trigger::SingleClick && j.action == ActionKind::ArrowUp));
}

#[test]
fn single_click_with_long_binding_is_immediate_and_long_press_fires() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(ButtonId::Up, Trigger::SingleClick, ActionKind::ArrowUp),
            binding(ButtonId::Up, Trigger::LongPress, ActionKind::Return),
        ],
    });
    // 短按：松开立即单击。
    assert!(jobs_of(&d, 38, true, 0).is_empty());
    let jobs = jobs_of(&d, 38, false, 100);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].trigger, Trigger::SingleClick);
    // 长按：阈值时触发长按，松开不再补单击。
    assert!(jobs_of(&d, 38, true, 200).is_empty());
    let jobs = d.tick_once(800);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].trigger, Trigger::LongPress);
    assert_eq!(jobs[0].action, ActionKind::Return);
    assert!(jobs_of(&d, 38, false, 900).is_empty());
}

#[test]
fn hold_without_long_binding_fires_single_on_release() {
    let d = dispatcher();
    // 只有单击绑定：按住超过长按阈值也不该产生任何动作，
    // 松开时按单击处理（以前会被 LongPress 判定吞掉）。
    jobs_of(&d, 38, true, 0);
    assert!(d.tick_once(600).is_empty(), "没有长按绑定不应触发动作");
    let jobs = jobs_of(&d, 38, false, 700);
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
    let jobs = jobs_of(&d, 38, false, 150);
    assert_eq!(jobs.len(), 1, "松开立即确认单击");
    assert_eq!(jobs[0].trigger, Trigger::SingleClick);
    assert!(d.tick_once(500).is_empty());
}

#[test]
fn spurious_release_is_ignored() {
    let d = dispatcher();
    // 没有按下的释放（回声）不应进入触发状态机
    assert!(jobs_of(&d, 38, false, 0).is_empty());
    assert!(jobs_of(&d, 38, true, 100).is_empty());
    // 松开立即确认单击（默认映射没有双击绑定）
    let jobs = jobs_of(&d, 38, false, 150);
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
fn volume_double_click_binding_is_not_swallowed() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::VolumeUp,
                Trigger::SingleClick,
                ActionKind::SystemVolumeUp,
            ),
            binding(ButtonId::VolumeUp, Trigger::DoubleClick, ActionKind::Return),
        ],
    });
    // 用户显式绑定了双击：必须退回手势判定，不能按下即执行。
    assert!(
        jobs_of(&d, 175, true, 0).is_empty(),
        "有双击绑定时不能跳过手势"
    );
    assert!(jobs_of(&d, 175, false, 50).is_empty());
    assert!(jobs_of(&d, 175, true, 100).is_empty());
    let jobs = jobs_of(&d, 175, false, 150);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].trigger, Trigger::DoubleClick);
    assert_eq!(jobs[0].action, ActionKind::Return, "用户配置的双击必须生效");
}

#[test]
fn volume_long_press_with_different_action_falls_back_to_gesture() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::VolumeUp,
                Trigger::SingleClick,
                ActionKind::SystemVolumeUp,
            ),
            binding(
                ButtonId::VolumeUp,
                Trigger::LongPress,
                ActionKind::SystemVolumeMute,
            ),
        ],
    });
    assert!(
        jobs_of(&d, 175, true, 0).is_empty(),
        "长按绑了别的动作 → 不能按下即执行"
    );
    let jobs = d.tick_once(600);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].trigger, Trigger::LongPress);
    assert_eq!(jobs[0].action, ActionKind::SystemVolumeMute);
    assert!(jobs_of(&d, 175, false, 800).is_empty());
}

#[test]
fn volume_long_press_with_same_action_keeps_repeat() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::VolumeUp,
                Trigger::SingleClick,
                ActionKind::SystemVolumeUp,
            ),
            binding(
                ButtonId::VolumeUp,
                Trigger::LongPress,
                ActionKind::SystemVolumeUp,
            ),
        ],
    });
    // 「长按也是音量+」属于冗余配置，应继续按下即执行 + 按住连发。
    assert_eq!(jobs_of(&d, 175, true, 0).len(), 1);
    let mut repeats = Vec::new();
    for at in (575u64..=1000).step_by(125) {
        repeats.extend(d.tick_once(at));
    }
    assert!(repeats.len() >= 2, "同动作长按绑定不应阻止连发");
    assert!(jobs_of(&d, 175, false, 1050).is_empty());
    assert!(d.tick_once(1500).is_empty());
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
    // 静音属于开关类动作：按下即执行，不再等待双击窗口。
    let jobs = jobs_of(&d, 175, true, 0);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].action, ActionKind::SystemVolumeMute);
    assert!(jobs_of(&d, 175, false, 50).is_empty());
    assert!(d.tick_once(400).is_empty());
}

#[test]
fn volume_action_fires_on_every_press_without_double_click_merge() {
    let d = dispatcher(); // 默认映射：音量+ 单击 -> SystemVolumeUp
    let mut fired = Vec::new();
    for i in 0..4u64 {
        let at = i * 200; // 200ms 间隔，落在 300ms 双击窗口内
        let jobs = jobs_of(&d, 175, true, at);
        assert_eq!(jobs.len(), 1, "第 {} 次按下应立即执行一格音量", i + 1);
        fired.extend(jobs);
        assert!(jobs_of(&d, 175, false, at + 80).is_empty());
    }
    assert_eq!(fired.len(), 4, "快速连按不能被合并成双击吞掉");
    assert!(fired
        .iter()
        .all(|j| j.action == ActionKind::SystemVolumeUp && j.trigger == Trigger::SingleClick));
}

#[test]
fn volume_action_repeats_while_held_and_stops_on_release() {
    let d = dispatcher();
    assert_eq!(jobs_of(&d, 175, true, 0).len(), 1, "按下立即执行一次");
    // 默认长按阈值 550ms 之前不连发（避免轻按也当成长按）。
    let mut t = 25u64;
    while t < 550 {
        assert!(d.tick_once(t).is_empty(), "阈值前不应连发: t={t}");
        t += 25;
    }
    // 阈值之后按固定间隔连发。
    let mut repeats = Vec::new();
    for at in (575u64..=1000).step_by(125) {
        repeats.extend(d.tick_once(at));
    }
    assert!(repeats.len() >= 2, "按住应连发，实际 {} 次", repeats.len());
    assert!(repeats
        .iter()
        .all(|j| j.action == ActionKind::SystemVolumeUp && j.trigger == Trigger::SingleClick));
    // 松手立即停止，且在真的松开后不再有任何重复。
    assert!(jobs_of(&d, 175, false, 1050).is_empty());
    assert!(d.tick_once(1500).is_empty(), "松手后不应继续连发");
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
fn mic_combo_ptt_holds_from_physical_down() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            core_mapping::KeyBinding {
                button: ButtonId::Mic,
                trigger: Trigger::Press,
                action: ActionKind::KeyCombo(vec!["rctrl".into()]),
            },
            core_mapping::KeyBinding {
                button: ButtonId::Mic,
                trigger: Trigger::Release,
                action: ActionKind::KeyCombo(vec!["rctrl".into()]),
            },
        ],
    });
    let down = jobs_of(&d, 116, true, 0);
    assert_eq!(down.len(), 1);
    assert_eq!(down[0].trigger, Trigger::Press);
    assert_eq!(down[0].action, ActionKind::KeyCombo(vec!["rctrl".into()]));
    assert!(d.tick_once(600).is_empty(), "快捷键 Press 不应再发一次");
    let up = jobs_of(&d, 116, false, 700);
    assert_eq!(up.len(), 1);
    assert_eq!(up[0].trigger, Trigger::Release);
    assert_eq!(up[0].action, ActionKind::KeyCombo(vec!["rctrl".into()]));
}

/// 按下与松开之间映射被换掉（切前台应用 / 热更新映射）时，
/// 松开必须仍然松开**按下时真正按住**的那组键。
#[test]
fn combo_release_uses_remembered_tokens_after_mapping_change() {
    let combo =
        |tokens: &[&str]| ActionKind::KeyCombo(tokens.iter().map(|t| (*t).to_string()).collect());
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(ButtonId::Mic, Trigger::Press, combo(&["rctrl"])),
            binding(ButtonId::Mic, Trigger::Release, combo(&["rctrl"])),
        ],
    });
    let down = jobs_of(&d, 116, true, 0);
    assert_eq!(down.len(), 1);
    assert_eq!(down[0].action, combo(&["rctrl"]));

    // 按住期间换了映射：松开边沿不该被新映射改写
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(ButtonId::Mic, Trigger::Press, combo(&["lalt"])),
            binding(ButtonId::Mic, Trigger::Release, combo(&["lalt"])),
        ],
    });
    let up = jobs_of(&d, 116, false, 700);
    assert_eq!(up.len(), 1);
    assert_eq!(up[0].trigger, Trigger::Release);
    assert_eq!(
        up[0].action,
        combo(&["rctrl"]),
        "松开必须沿用按下时按住的那组键，否则那组键永远没人松开"
    );
}

/// 即使按住期间该映射被禁用，松开也必须照发——多松一次无害，
/// 少松一次就是全局卡修饰键。
#[test]
fn combo_release_is_not_lost_when_binding_disabled_mid_hold() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::Mic,
                Trigger::Press,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
            binding(
                ButtonId::Mic,
                Trigger::Release,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
        ],
    });
    assert_eq!(jobs_of(&d, 116, true, 0).len(), 1);
    d.update_mapping(MappingConfig {
        bindings: vec![binding(
            ButtonId::Mic,
            Trigger::Release,
            ActionKind::Disabled,
        )],
    });
    let up = jobs_of(&d, 116, false, 500);
    assert_eq!(up.len(), 1);
    assert_eq!(up[0].action, ActionKind::KeyCombo(vec!["rctrl".into()]));
}

/// 兜底释放：按下快捷键后走强制释放路径，必须生成配对的松开任务。
#[test]
fn release_all_held_emits_paired_release() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::Mic,
                Trigger::Press,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
            binding(
                ButtonId::Mic,
                Trigger::Release,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
        ],
    });
    assert_eq!(jobs_of(&d, 116, true, 0).len(), 1);

    assert_eq!(d.release_all_held("测试"), 1);
    let jobs = drain_jobs(&d);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].trigger, Trigger::Release);
    assert_eq!(jobs[0].action, ActionKind::KeyCombo(vec!["rctrl".into()]));

    // 状态已清干净：重复调用不再产出，后续物理松开也不会重复松。
    assert_eq!(d.release_all_held("再试一次"), 0);
    assert!(drain_jobs(&d).is_empty());
    assert!(jobs_of(&d, 116, false, 900).is_empty());
    assert!(d.tick_once(1000).is_empty());
}

/// Voice（Win+H）的 Press 是一次完整点按，没有需要配对的系统按住状态：
/// 强制释放只清状态，不产生任务。
#[test]
fn release_all_held_ignores_voice_hold() {
    let d = dispatcher();
    assert!(jobs_of(&d, 116, true, 0).is_empty());
    assert_eq!(d.tick_once(600).len(), 1, "长按识别后发 Win+H");

    assert_eq!(d.release_all_held("测试"), 0);
    assert!(drain_jobs(&d).is_empty());
    assert!(
        jobs_of(&d, 116, false, 700).is_empty(),
        "状态已清空，物理松开不再产生 Voice Release"
    );
}

/// 诊断页暂停调度会吞掉后续松开事件：暂停时必须先松开已按住的快捷键。
#[test]
fn set_enabled_false_releases_held_combo() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::Mic,
                Trigger::Press,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
            binding(
                ButtonId::Mic,
                Trigger::Release,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
        ],
    });
    assert_eq!(jobs_of(&d, 116, true, 0).len(), 1);

    assert!(d.set_enabled(false));
    let jobs = drain_jobs(&d);
    assert_eq!(jobs.len(), 1, "暂停调度前必须松开按住的快捷键");
    assert_eq!(jobs[0].action, ActionKind::KeyCombo(vec!["rctrl".into()]));

    assert!(d.set_enabled(true));
    assert!(drain_jobs(&d).is_empty(), "恢复调度不应重复产出松开");
    // 按下状态已清空：暂停前的释放不会串到恢复之后
    assert!(jobs_of(&d, 116, false, 800).is_empty());
}

/// 快捷菜单独占模式不处理松开事件：切进去之前必须松开已按住的快捷键。
#[test]
fn quick_menu_switch_releases_held_combo() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::Mic,
                Trigger::Press,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
            binding(
                ButtonId::Mic,
                Trigger::Release,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
        ],
    });
    assert_eq!(jobs_of(&d, 116, true, 0).len(), 1);

    d.set_input_mode(InputMode::QuickMenu);
    let jobs = drain_jobs(&d);
    assert_eq!(jobs.len(), 1, "切模式前必须松开按住的快捷键");
    assert_eq!(jobs[0].trigger, Trigger::Release);
    assert_eq!(jobs[0].action, ActionKind::KeyCombo(vec!["rctrl".into()]));

    d.set_input_mode(InputMode::Normal);
    assert!(drain_jobs(&d).is_empty());
    assert!(jobs_of(&d, 116, false, 900).is_empty());
}

/// 按住超过兜底上限（松开事件丢失）时，tick 必须强制松开。
#[test]
fn combo_hold_watchdog_releases_after_timeout() {
    let d = dispatcher();
    d.set_max_combo_hold_ms(1_000);
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::Mic,
                Trigger::Press,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
            binding(
                ButtonId::Mic,
                Trigger::Release,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
        ],
    });
    assert_eq!(jobs_of(&d, 116, true, 0).len(), 1);

    assert!(d.tick_once(500).is_empty(), "未到上限不释放");
    let jobs = d.tick_once(1_000);
    assert_eq!(jobs.len(), 1, "到上限强制释放");
    assert_eq!(jobs[0].trigger, Trigger::Release);
    assert_eq!(jobs[0].action, ActionKind::KeyCombo(vec!["rctrl".into()]));
    assert!(d.tick_once(1_100).is_empty(), "释放只做一次");
    assert!(jobs_of(&d, 116, false, 1_200).is_empty());
}

/// 麦克风按键映射为自定义组合键时，默认不应在 10s 处被强制掐断，保障长语音输入。
#[test]
fn mic_combo_hold_does_not_release_at_10s_default() {
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            binding(
                ButtonId::Mic,
                Trigger::Press,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
            binding(
                ButtonId::Mic,
                Trigger::Release,
                ActionKind::KeyCombo(vec!["rctrl".into()]),
            ),
        ],
    });
    assert_eq!(jobs_of(&d, 116, true, 0).len(), 1);

    // 默认普通键上限为 10s；麦克风在 10s / 20s / 30s 都不应被强制释放
    assert!(d.tick_once(10_000).is_empty(), "10s 时不应掐断麦克风长语音");
    assert!(d.tick_once(20_000).is_empty(), "20s 时不应掐断麦克风长语音");
    assert!(d.tick_once(30_000).is_empty(), "30s 时不应掐断麦克风长语音");

    // 直到物理松开信号到达，正常释放
    let jobs = jobs_of(&d, 116, false, 35_000);
    assert_eq!(jobs.len(), 1, "物理松开正常释放");
    assert_eq!(jobs[0].trigger, Trigger::Release);
}

/// 暂停/恢复调度不能丢掉用户配置的长按阈值。
#[test]
fn set_enabled_keeps_trigger_timing() {
    let d = dispatcher();
    d.set_trigger_timing(900, 400);
    d.update_mapping(MappingConfig {
        bindings: vec![binding(
            ButtonId::Up,
            Trigger::LongPress,
            ActionKind::OpenApp("notepad".into()),
        )],
    });
    assert!(d.set_enabled(false));
    assert!(d.set_enabled(true));

    jobs_of(&d, 38, true, 0);
    assert!(
        d.tick_once(600).is_empty(),
        "自定义阈值 900ms 应仍在生效（不是默认 550ms）"
    );
    assert_eq!(d.tick_once(1_000).len(), 1, "超过 900ms 才触发长按");
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

#[test]
fn voice_primitive_maps_triggers_to_edges() {
    assert_eq!(voice_primitive(Trigger::Press), VoicePrimitive::Press);
    assert_eq!(voice_primitive(Trigger::Release), VoicePrimitive::Release);
    assert_eq!(voice_primitive(Trigger::SingleClick), VoicePrimitive::Open);
    assert_eq!(voice_primitive(Trigger::DoubleClick), VoicePrimitive::Open);
    assert_eq!(voice_primitive(Trigger::LongPress), VoicePrimitive::Open);
}

#[test]
fn voice_job_preserves_trigger_for_ptt_dispatch() {
    // 麦克风默认映射 Press/Release → Voice：任务必须保留触发边沿，
    // 执行层才能区分「按住说话」与「松手收尾」。
    let d = dispatcher();
    d.update_mapping(MappingConfig {
        bindings: vec![
            core_mapping::KeyBinding {
                button: ButtonId::Mic,
                trigger: Trigger::Press,
                action: ActionKind::Voice,
            },
            core_mapping::KeyBinding {
                button: ButtonId::Mic,
                trigger: Trigger::Release,
                action: ActionKind::Voice,
            },
        ],
    });
    // 模拟长按：按住超过阈值 → tick 产生 Press；松开 → Release。
    assert!(jobs_of(&d, 116, true, 0).is_empty());
    let press_jobs = d.tick_once(600);
    assert_eq!(press_jobs.len(), 1);
    assert_eq!(press_jobs[0].trigger, Trigger::Press);
    assert_eq!(press_jobs[0].action, ActionKind::Voice);
    let release_jobs = jobs_of(&d, 116, false, 700);
    assert_eq!(release_jobs.len(), 1);
    assert_eq!(release_jobs[0].trigger, Trigger::Release);
    assert_eq!(release_jobs[0].action, ActionKind::Voice);
}

#[test]
fn toggle_quick_menu_without_handler_errors() {
    let dummy_target = Arc::new(Mutex::new(VoiceTarget::default()));
    let job = ActionJob {
        button: ButtonId::Menu,
        trigger: Trigger::SingleClick,
        action: ActionKind::ToggleQuickMenu,
    };
    let err = execute_action(&job, None, &dummy_target).unwrap_err();
    assert!(err.contains("未接线"), "缺少回调时应报错：{err}");
}

#[test]
fn toggle_quick_menu_calls_app_event_handler() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let handler: AppEventHandler = Arc::new({
        let calls = calls.clone();
        move |event| {
            assert_eq!(event, AppEvent::ToggleQuickMenu);
            calls.fetch_add(1, Ordering::SeqCst);
        }
    });
    let dummy_target = Arc::new(Mutex::new(VoiceTarget::default()));
    let job = ActionJob {
        button: ButtonId::Menu,
        trigger: Trigger::SingleClick,
        action: ActionKind::ToggleQuickMenu,
    };
    execute_action(&job, Some(&handler), &dummy_target).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn quick_menu_mode_routes_keys_to_handler_and_skips_mapping() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let d = dispatcher();
    let calls = Arc::new(AtomicUsize::new(0));
    let handler: AppEventHandler = Arc::new({
        let calls = calls.clone();
        move |event| {
            if let AppEvent::MenuKey(ButtonId::Up, true) = event {
                calls.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    d.set_app_event_handler(Some(handler));
    d.set_input_mode(InputMode::QuickMenu);
    d.on_vkey(38, true);
    d.on_vkey(38, false);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "按下边沿应转发一次");
    assert!(
        d.tick_once(400).is_empty(),
        "菜单独占模式下不应产生普通映射任务"
    );
    // 切回普通模式后恢复普通单击（没有双击绑定：松开立即生效）。
    d.set_input_mode(InputMode::Normal);
    d.on_vkey(38, true);
    d.on_vkey(38, false);
    let jobs = drain_jobs(&d);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].action, ActionKind::ArrowUp);
}

// ---- 应用专属配置（profile）覆盖语义 ----

#[test]
fn profile_overrides_global_mapping() {
    let mapping = MappingConfig::default();
    let profile = profile_with(vec![binding(
        ButtonId::Ok,
        Trigger::SingleClick,
        ActionKind::KeyCombo(vec!["lctrl".into(), "k".into()]),
    )]);

    // 全局映射里 Ok 是 Return；命中应用配置时应改为 Ctrl+K。
    assert_eq!(
        resolve_action(&mapping, Some(&profile), ButtonId::Ok, Trigger::SingleClick),
        Some(ActionKind::KeyCombo(vec!["lctrl".into(), "k".into()]))
    );
}

#[test]
fn profile_falls_back_to_global_when_not_covered() {
    let mapping = MappingConfig::default();
    let profile = profile_with(vec![binding(
        ButtonId::Ok,
        Trigger::SingleClick,
        ActionKind::Escape,
    )]);

    // 配置只覆盖了 Ok 的单击；其它按键与其它手势都沿用全局映射。
    assert_eq!(
        resolve_action(&mapping, Some(&profile), ButtonId::Ok, Trigger::SingleClick),
        Some(ActionKind::Escape)
    );
    assert_eq!(
        resolve_action(&mapping, Some(&profile), ButtonId::Ok, Trigger::DoubleClick),
        mapping.resolve(ButtonId::Ok, Trigger::DoubleClick).cloned()
    );
    assert_eq!(
        resolve_action(&mapping, Some(&profile), ButtonId::Up, Trigger::SingleClick),
        mapping.resolve(ButtonId::Up, Trigger::SingleClick).cloned()
    );
}

#[test]
fn no_profile_uses_global_mapping() {
    let mapping = MappingConfig::default();
    assert_eq!(
        resolve_action(&mapping, None, ButtonId::Ok, Trigger::SingleClick),
        mapping.resolve(ButtonId::Ok, Trigger::SingleClick).cloned()
    );
}

/// 约定：语音/按住说话是全局长按语义，profile 不得覆盖 Mic。
#[test]
fn profile_never_overrides_mic() {
    let mapping = MappingConfig::default();
    let profile = profile_with(vec![
        binding(
            ButtonId::Mic,
            Trigger::Press,
            ActionKind::KeyCombo(vec!["ralt".into()]),
        ),
        binding(ButtonId::Mic, Trigger::Release, ActionKind::Disabled),
    ]);

    for trigger in [Trigger::Press, Trigger::Release, Trigger::SingleClick] {
        assert_eq!(
            resolve_action(&mapping, Some(&profile), ButtonId::Mic, trigger),
            mapping.resolve(ButtonId::Mic, trigger).cloned(),
            "Mic 的 {trigger:?} 不应被应用配置改写"
        );
    }
}

/// 配置可以把某个按键在某个应用里禁用掉。
#[test]
fn profile_can_disable_a_button() {
    let mapping = MappingConfig::default();
    let profile = profile_with(vec![binding(
        ButtonId::Home,
        Trigger::SingleClick,
        ActionKind::Disabled,
    )]);
    assert_eq!(
        resolve_action(
            &mapping,
            Some(&profile),
            ButtonId::Home,
            Trigger::SingleClick
        ),
        Some(ActionKind::Disabled)
    );
}

/// 未加载任何配置时不去查前台窗口，且行为与从前一致。
#[test]
fn empty_registry_has_no_foreground_lookup() {
    let registry = ProfileRegistry::default();
    assert!(foreground_profile(&registry).is_none());
}

/// 快捷菜单内圈：只列配了 icon 的配置，顺序与声明一致。
#[test]
fn menu_apps_lists_only_profiles_with_icon() {
    let dispatcher = KeyDispatcher::new(MappingConfig::default(), &HashMap::new());
    let mut registry = ProfileRegistry::default();

    registry.upsert(AppProfile {
        id: "no-icon".into(),
        process: core_app_profile::ProcessSpec::One("__no_icon__.exe".into()),
        name: "没有图标".into(),
        ..Default::default()
    });
    registry.upsert(AppProfile {
        id: "first".into(),
        process: core_app_profile::ProcessSpec::One("__first__.exe".into()),
        name: "第一个".into(),
        icon: Some(core_app_profile::IconSpec {
            label: "1st".into(),
            color: "#111111".into(),
        }),
        ..Default::default()
    });
    registry.upsert(AppProfile {
        id: "second".into(),
        process: core_app_profile::ProcessSpec::One("__second__.exe".into()),
        name: "第二个".into(),
        icon: Some(core_app_profile::IconSpec {
            label: "2nd".into(),
            color: "#222222".into(),
        }),
        ..Default::default()
    });

    dispatcher.set_profiles(registry);
    let apps = dispatcher.menu_apps();

    assert_eq!(apps.len(), 2, "没有 icon 的配置不该出现在菜单里");
    assert_eq!(apps[0].id, "first");
    assert_eq!(apps[0].name, "第一个");
    assert_eq!(apps[0].label, "1st");
    assert_eq!(apps[0].color, "#111111");
    assert_eq!(apps[1].id, "second");
    assert_eq!(apps[1].name, "第二个");
    // 这两个进程都不存在，所以都是「点了会启动」。
    assert!(!apps[0].open && !apps[1].open);

    // 按 id 能取回配置副本，供 open_app_profile 使用。
    assert!(dispatcher.profile_by_id("second").is_some());
    assert!(dispatcher.profile_by_id("no-icon").is_some());
    assert!(dispatcher.profile_by_id("不存在").is_none());
    // 展示名不是身份：改了名字不影响按 id 定位。
    assert!(dispatcher.profile_by_id("第二个").is_none());

    // 编辑器要的全量快照含没有图标的配置。
    assert_eq!(dispatcher.profiles_snapshot().len(), 3);
}

/// 真机端到端：按「当前真实前台进程名」造一份配置，确认能被匹配到。
/// 无人值守环境读不到前台窗口时跳过，避免 CI 抖动。
#[test]
fn foreground_profile_matches_real_foreground_process() {
    let Some(exe) = core_app_profile::foreground_process_name() else {
        eprintln!("skip: 当前环境读不到前台窗口");
        return;
    };
    eprintln!("foreground = {exe}");

    let mut registry = ProfileRegistry::default();
    registry.upsert(AppProfile {
        process: core_app_profile::ProcessSpec::One(exe),
        name: "前台测试".into(),
        bindings: vec![binding(
            ButtonId::Menu,
            Trigger::SingleClick,
            ActionKind::KeyCombo(vec!["lctrl".into(), "k".into()]),
        )],
        ..Default::default()
    });

    let hit = foreground_profile(&registry).expect("应命中当前前台进程");
    assert_eq!(hit.display_name(), "前台测试");

    // 命中后，被覆盖的按键确实改走应用配置。
    let mapping = MappingConfig::default();
    assert_eq!(
        resolve_action(&mapping, Some(hit), ButtonId::Menu, Trigger::SingleClick),
        Some(ActionKind::KeyCombo(vec!["lctrl".into(), "k".into()]))
    );
    // 未覆盖的按键仍走全局映射。
    assert_eq!(
        resolve_action(&mapping, Some(hit), ButtonId::Up, Trigger::SingleClick),
        mapping.resolve(ButtonId::Up, Trigger::SingleClick).cloned()
    );
}

/// 真机端到端：只给**窗口标题**（不给进程名）也要能命中。
///
/// 这条路径服务于 Chrome PWA 和跑在 WSL 里的服务——它们在 Windows 侧
/// 只能靠标题认出来。
#[test]
fn foreground_profile_matches_real_foreground_title() {
    let Some(title) = core_app_profile::foreground_window_title() else {
        eprintln!("skip: 当前环境读不到前台窗口标题");
        return;
    };
    eprintln!("foreground title = {title}");

    let mut registry = ProfileRegistry::default();
    registry.upsert(AppProfile {
        // 进程名故意留空：这条测试要证明标题自己就够用。
        name: "标题测试".into(),
        window_title_contains: vec![title.clone()],
        ..Default::default()
    });

    let hit = foreground_profile(&registry).expect("应能只靠标题命中");
    assert_eq!(hit.display_name(), "标题测试");
}
