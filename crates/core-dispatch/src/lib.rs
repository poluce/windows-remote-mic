//! core-dispatch — 按键调度器：物理按键 → 触发检测 → 映射解析 → 动作执行。
//!
//! 这是把「按键映射」从纯配置变成运行时闭环的核心一环：
//! Raw Input 标准流与 HOGP 旁路在 src-tauri 汇聚后调用
//! [`KeyDispatcher::on_vkey`]；调度器为每个按键维护一个
//! `TriggerDetector`（单击/双击/长按/按下/松开），确认触发后查
//! [`MappingConfig`](core_mapping::MappingConfig)，在独立执行线程上经
//! `core-input` 注入动作，并写入 `core-stats`。
//!
//! 麦克风键也走调度器：默认映射为 Press→Voice、Release→Voice。
//! Voice 动作按当前「语音识别目标」（默认 Windows 语音 Win+H）与触发
//! 边沿分发：Press/Release 走 PTT（按住说话/松手收尾），非 PTT 边沿走 Tap。
//! 用户可在映射页把麦克风的按下/松开改成任意动作（如第三方语音助手），
//! 映射表不是摆设。
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

use core_app_profile::{AppProfile, ProfileRegistry};
use core_config::KeyCalibration;
use core_mapping::trigger::{FeedOutcome, TriggerDetector};
use core_mapping::{ActionKind, ButtonId, MappingConfig, Trigger, VoiceTarget};

/// tick 线程轮询间隔。
const TICK_INTERVAL_MS: u64 = 25;

/// 单个按键的运行时状态。
#[derive(Default)]
struct ButtonRuntime {
    detector: TriggerDetector,
    down: bool,
    /// 本次按住期间是否已触发过长按。
    long_executed: bool,
    /// 自定义快捷键已在物理按下时按住，松开时必须配对松开。
    combo_held: bool,
}

struct Inner {
    mapping: MappingConfig,
    /// 应用专属配置（一个应用一个文件）。命中前台进程时覆盖 `mapping`
    /// 中对应的 (按键, 触发)；未覆盖的项继续沿用全局映射。
    profiles: ProfileRegistry,
    /// 虚拟键 -> 物理按键（默认表 + 校准表覆盖）。
    vkey_map: HashMap<u16, ButtonId>,
    buttons: HashMap<ButtonId, ButtonRuntime>,
    enabled: bool,
    /// 输入路由模式：普通映射 / 快捷菜单独占（按键直转菜单回调）。
    mode: InputMode,
}

/// 输入路由模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// 普通模式：按键走触发检测与映射。
    Normal,
    /// 快捷菜单独占模式：所有按键事件直接转发给应用事件回调。
    QuickMenu,
}

/// 需要 Tauri 层执行的应用事件（调度器 → 应用壳的唯一出口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    /// 映射动作：打开/关闭快捷菜单（由 `ToggleQuickMenu` 动作触发）。
    ToggleQuickMenu,
    /// 菜单独占模式下遥控器按键直转（参数 = 物理按键, 是否按下边沿）。
    MenuKey(ButtonId, bool),
}

/// 应用事件回调：需要 Tauri 层执行的动作（开关快捷菜单、菜单独占按键直转）。
type AppEventHandler = Arc<dyn Fn(AppEvent) + Send + Sync>;

/// 一条待执行的动作任务。
#[derive(Debug, Clone)]
pub struct ActionJob {
    pub button: ButtonId,
    pub trigger: Trigger,
    pub action: ActionKind,
}

/// 前台应用与其命中的应用配置（诊断页用来核对进程名）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ForegroundStatus {
    /// 当前前台窗口的可执行文件名，例如 `Codex.exe`；读不到时为 None。
    pub process: Option<String>,
    /// 当前前台窗口的标题；只按标题匹配的配置（Chrome PWA、WSL 里的服务）靠它命中。
    pub title: Option<String>,
    /// 命中的应用配置展示名；没命中时为 None。
    pub profile: Option<String>,
    /// 已加载的应用配置总数。
    pub profile_count: usize,
    /// 命中的配置覆盖了哪些按键（稳定小写标识，已排序去重）。
    pub overridden_buttons: Vec<String>,
}

/// 快捷菜单内圈的一个应用图标。
#[derive(Debug, Clone, serde::Serialize)]
pub struct MenuAppEntry {
    /// 稳定标识：回传 `open_app_profile` 时用它定位配置（当前即展示名）。
    pub name: String,
    /// 图标上的 1–2 个字符。
    pub label: String,
    /// 品牌色，形如 `#4D6BFE`。
    pub color: String,
    /// 当前是否已有可聚焦的窗口（前端据此区分「已打开」与「将启动」）。
    pub open: bool,
}

/// 按键调度器。构造后通过 [`KeyDispatcher::spawn_runtime`] 启动
/// 执行线程与 tick 线程；测试可直接调用 [`KeyDispatcher::feed`]
/// 注入合成时间戳，不触碰真实输入。
pub struct KeyDispatcher {
    start: Instant,
    inner: Mutex<Inner>,
    jobs_tx: mpsc::Sender<ActionJob>,
    jobs_rx: Mutex<Option<mpsc::Receiver<ActionJob>>>,
    /// 应用事件出口：需要 Tauri 层执行的动作（开关快捷菜单、
    /// 菜单独占模式的按键直转）统一经此回调解出。须在 `spawn_runtime`
    /// 之前设置。
    app_event: Mutex<Option<AppEventHandler>>,
    /// 语音识别目标：Voice 动作唤起哪一家的语音输入。可热更新
    /// （连接页切换「识别方案」时调用 [`KeyDispatcher::set_voice_target`]）。
    /// 执行线程每次执行 Voice 动作时读取当前值。
    voice_target: Arc<Mutex<VoiceTarget>>,
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
                profiles: ProfileRegistry::default(),
                enabled: true,
                mode: InputMode::Normal,
            }),
            jobs_tx: tx,
            jobs_rx: Mutex::new(Some(rx)),
            app_event: Mutex::new(None),
            voice_target: Arc::new(Mutex::new(VoiceTarget::default())),
        })
    }

    /// 设置应用事件出口（开关快捷菜单、菜单独占按键直转等）。
    /// 须在 `spawn_runtime` 之前调用。
    pub fn set_app_event_handler(&self, handler: Option<AppEventHandler>) {
        *self.app_event.lock().unwrap() = handler;
    }

    /// 热更新语音识别目标（连接页切换「识别方案」后调用）。
    pub fn set_voice_target(&self, target: VoiceTarget) {
        *self.voice_target.lock().unwrap() = target;
        core_log::log_line(&format!("[dispatch] 语音识别目标已更新: {:?}", target));
    }

    /// 替换应用专属配置集合（启动时加载，或用户改过文件后重载）。
    pub fn set_profiles(&self, profiles: ProfileRegistry) {
        let count = profiles.profiles().len();
        self.inner.lock().unwrap().profiles = profiles;
        core_log::log_line(&format!("[dispatch] 已加载 {count} 份应用专属配置"));
    }

    /// 当前前台应用与命中的配置，供诊断页显示 / 核对进程名。
    pub fn foreground_status(&self) -> ForegroundStatus {
        let inner = self.inner.lock().unwrap();
        let process = core_app_profile::foreground_process_name();
        let title = core_app_profile::foreground_window_title();
        let profile = process.as_deref().and_then(|exe| {
            inner
                .profiles
                .match_context(exe, title.as_deref().unwrap_or_default())
        });
        ForegroundStatus {
            profile_count: inner.profiles.profiles().len(),
            profile: profile.map(|p| p.display_name()),
            overridden_buttons: profile
                .map(|p| {
                    let mut keys: Vec<String> = p
                        .bindings
                        .iter()
                        .map(|b| b.button.key().to_string())
                        .collect();
                    keys.sort();
                    keys.dedup();
                    keys
                })
                .unwrap_or_default(),
            process,
            title,
        }
    }

    /// 快捷菜单内圈要显示的应用图标。
    ///
    /// 只返回配了 `icon` 的配置——**图标即入菜单的开关**，不在菜单里露脸的应用
    /// 仍然可以正常享受前台自动切换映射，只是没有入口。
    pub fn menu_apps(&self) -> Vec<MenuAppEntry> {
        let inner = self.inner.lock().unwrap();
        inner
            .profiles
            .menu_entries()
            .into_iter()
            .filter_map(|p| {
                let icon = p.icon.as_ref()?;
                Some(MenuAppEntry {
                    name: p.display_name(),
                    label: icon.label.clone(),
                    color: icon.color.clone(),
                    open: core_app_profile::find_window(p).is_some(),
                })
            })
            .collect()
    }

    /// 按展示名取一份应用配置的副本（供「点图标」时启动/聚焦用）。
    ///
    /// 返回副本而不是引用：调用方要去做启动窗口这类可能阻塞的事，
    /// 不该一直占着调度器的锁。
    pub fn profile_named(&self, name: &str) -> Option<AppProfile> {
        let inner = self.inner.lock().unwrap();
        inner
            .profiles
            .profiles()
            .iter()
            .find(|p| p.display_name() == name)
            .cloned()
    }

    /// 当前语音识别目标。
    pub fn voice_target(&self) -> VoiceTarget {
        *self.voice_target.lock().unwrap()
    }

    /// 切换输入路由模式：快捷菜单独占模式下所有按键绕过触发检测与映射，
    /// 直接以 `AppEvent::MenuKey` 转发给应用事件出口；切回普通模式恢复
    /// 常规调度。切换时清空按键中间状态。
    pub fn set_input_mode(&self, mode: InputMode) {
        let mut inner = self.inner.lock().unwrap();
        if inner.mode == mode {
            return;
        }
        inner.mode = mode;
        for rt in inner.buttons.values_mut() {
            *rt = ButtonRuntime::default();
        }
        core_log::log_line(&format!("[dispatch] 输入模式已切换: {mode:?}"));
    }

    /// 启动动作执行线程与 tick 线程。
    ///
    /// `stats_dir` 为按键统计存储目录（`core-stats`）。
    pub fn spawn_runtime(self: &Arc<Self>, stats_dir: PathBuf) {
        let app_event = self.app_event.lock().unwrap().clone();
        let voice_target = self.voice_target.clone();
        if let Some(rx) = self.jobs_rx.lock().unwrap().take() {
            std::thread::Builder::new()
                .name("rc003-dispatch-exec".into())
                .spawn(move || execute_loop(rx, stats_dir, app_event, voice_target))
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
    ///
    /// 快捷菜单独占模式下不经过触发检测与映射，直接以
    /// `AppEvent::MenuKey` 转发给应用事件出口。
    pub fn on_vkey(&self, vkey: u16, pressed: bool) {
        let (mode, button) = {
            let inner = self.inner.lock().unwrap();
            (inner.mode, inner.vkey_map.get(&vkey).copied())
        };
        if mode == InputMode::QuickMenu {
            if let Some(cb) = self.app_event.lock().unwrap().as_ref() {
                if let Some(button) = button {
                    cb(AppEvent::MenuKey(button, pressed));
                }
            }
            return;
        }
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
            profiles,
            vkey_map,
            buttons,
            ..
        } = &mut *inner;
        let Some(&button) = vkey_map.get(&vkey) else {
            return jobs;
        };
        let profile = foreground_profile(profiles);
        let rt = buttons.entry(button).or_default();
        if pressed {
            if rt.down {
                // 系统按住重复：只算一次物理按下
                return jobs;
            }
            rt.down = true;
            rt.long_executed = false;
            rt.combo_held = false;
            rt.detector.press(now);
            // 自定义快捷键按住说话：物理按下立即按住，不等长按阈值
            // （豆包等 IME 要的是键盘那种「一按住就生效」）。
            // Voice（Win+H）仍等 tick 识别长按后再发，避免点按误开语音条。
            if let Some(job) = build_job(mapping, profile, button, Trigger::Press, rt) {
                if matches!(job.action, ActionKind::KeyCombo(_)) {
                    rt.combo_held = true;
                    jobs.push(job);
                }
            }
        } else {
            if !rt.down {
                // 没有对应按下的释放（被抑制的回声），直接忽略
                return jobs;
            }
            rt.down = false;
            // 先让检测器确认本次按住是否达到长按阈值（tick 漏掉时兜底）。
            let outcome = rt.detector.release(now);
            if rt.combo_held {
                rt.combo_held = false;
                if let Some(job) = build_job(mapping, profile, button, Trigger::Release, rt) {
                    jobs.push(job);
                }
            } else if rt.detector.is_long_held() {
                // Release 边沿触发（Voice）：只有长按结束才发。
                if let Some(job) = build_job(mapping, profile, button, Trigger::Release, rt) {
                    jobs.push(job);
                }
            }
            // 单击/双击/长按等手势确认。
            if let FeedOutcome::Fire(ev) = outcome {
                if let Some(job) = build_job(mapping, profile, button, ev.trigger, rt) {
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
            mapping,
            profiles,
            buttons,
            ..
        } = &mut *inner;
        let profile = foreground_profile(profiles);
        for (button, rt) in buttons.iter_mut() {
            let was_long = rt.detector.is_long_held();
            let outcome = rt.detector.tick(now);
            // 长按刚被识别：发 Press 边沿触发（Win+H 等）。快捷键已在按下时发过。
            if !was_long && rt.detector.is_long_held() {
                if let Some(job) = build_job(mapping, profile, *button, Trigger::Press, rt) {
                    if !matches!(job.action, ActionKind::KeyCombo(_)) {
                        jobs.push(job);
                    }
                }
            }
            if let FeedOutcome::Fire(ev) = outcome {
                if let Some(job) = build_job(mapping, profile, *button, ev.trigger, rt) {
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
///
/// `profile` 是本次事件发生时的前台应用配置（无前台应用 / 未命中时为 `None`）；
/// 由调用方在每次 `feed` / `tick_once` 时解析一次，避免每个按键都去查前台窗口。
fn build_job(
    mapping: &MappingConfig,
    profile: Option<&AppProfile>,
    button: ButtonId,
    trigger: Trigger,
    rt: &mut ButtonRuntime,
) -> Option<ActionJob> {
    let action = resolve_action(mapping, profile, button, trigger)?;
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

/// 解析 (按键, 触发) 对应的动作：**应用配置优先，未覆盖时回落到全局映射**。
fn resolve_action(
    mapping: &MappingConfig,
    profile: Option<&AppProfile>,
    button: ButtonId,
    trigger: Trigger,
) -> Option<ActionKind> {
    if let Some(action) = profile_override(profile, button, trigger) {
        return Some(action);
    }
    mapping.resolve(button, trigger).cloned()
}

/// 应用配置里对该 (按键, 触发) 的覆盖项。
///
/// 约定：**不覆盖 `Mic`**。语音 / 按住说话是全局长按语义、与具体应用无关，
/// 让某份配置改掉它只会让该应用里的语音静默失效。
fn profile_override(
    profile: Option<&AppProfile>,
    button: ButtonId,
    trigger: Trigger,
) -> Option<ActionKind> {
    if button == ButtonId::Mic {
        return None;
    }
    profile?
        .bindings
        .iter()
        .find(|b| b.button == button && b.trigger == trigger)
        .map(|b| b.action.clone())
}

/// 取当前前台应用命中的配置；没有加载任何配置时不查前台窗口。
///
/// 匹配用「进程名 + 窗口标题」：进程名优先，标题兜底是为了 Chrome PWA、
/// 以及跑在 WSL 里的服务（DSH 只有浏览器窗口是 Windows 进程）。
fn foreground_profile(profiles: &ProfileRegistry) -> Option<&AppProfile> {
    if profiles.profiles().is_empty() {
        return None;
    }
    let exe = core_app_profile::foreground_process_name()?;
    let title = core_app_profile::foreground_window_title().unwrap_or_default();
    profiles.match_context(&exe, &title)
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
fn execute_loop(
    rx: mpsc::Receiver<ActionJob>,
    stats_dir: PathBuf,
    app_event: Option<AppEventHandler>,
    voice_target: Arc<Mutex<VoiceTarget>>,
) {
    let stats = core_stats::StatsStore::new(stats_dir).ok();
    while let Ok(job) = rx.recv() {
        let outcome = execute_action(&job, app_event.as_ref(), &voice_target);
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

/// Voice 动作要调用的底层语音原语（由触发边沿决定，与识别目标无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoicePrimitive {
    /// PTT 按住说话开始（麦克风长按识别后的 Press）。
    Press,
    /// PTT 松手收尾（麦克风长按结束的 Release）。
    Release,
    /// 单次 Tap：开启/重置一次语音会话（非 PTT 边沿）。
    Open,
}

/// 由触发边沿决定 Voice 动作走哪个原语。
fn voice_primitive(trigger: Trigger) -> VoicePrimitive {
    match trigger {
        Trigger::Press => VoicePrimitive::Press,
        Trigger::Release => VoicePrimitive::Release,
        _ => VoicePrimitive::Open,
    }
}

fn execute_action(
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
            let target = *voice_target.lock().unwrap();
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

#[cfg(test)]
mod tests {
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
        // 切回普通模式后恢复普通单击。
        d.set_input_mode(InputMode::Normal);
        d.on_vkey(38, true);
        d.on_vkey(38, false);
        let jobs = d.tick_once(400);
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
            process: core_app_profile::ProcessSpec::One("__no_icon__.exe".into()),
            name: "没有图标".into(),
            ..Default::default()
        });
        registry.upsert(AppProfile {
            process: core_app_profile::ProcessSpec::One("__first__.exe".into()),
            name: "第一个".into(),
            icon: Some(core_app_profile::IconSpec {
                label: "1st".into(),
                color: "#111111".into(),
            }),
            ..Default::default()
        });
        registry.upsert(AppProfile {
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
        assert_eq!(apps[0].name, "第一个");
        assert_eq!(apps[0].label, "1st");
        assert_eq!(apps[0].color, "#111111");
        assert_eq!(apps[1].name, "第二个");
        // 这两个进程都不存在，所以都是「点了会启动」。
        assert!(!apps[0].open && !apps[1].open);

        // 按展示名能取回配置副本，供 open_app_profile 使用。
        assert!(dispatcher.profile_named("第二个").is_some());
        assert!(dispatcher.profile_named("没有图标").is_some());
        assert!(dispatcher.profile_named("不存在").is_none());
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
}
