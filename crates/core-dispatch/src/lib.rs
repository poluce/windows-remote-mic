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
use core_mapping::{ActionKind, ButtonId, MappingConfig, PressSemantics, Trigger, VoiceTarget};

/// tick 线程轮询间隔。
const TICK_INTERVAL_MS: u64 = 25;

/// 步进类动作（系统音量 ±）按住时的连发间隔（毫秒）。
///
/// 首次连发在长按阈值之后触发，之后按固定间隔重复；节奏接近 Windows
/// 键盘音量键的自动重复。
const REPEAT_STEP_INTERVAL_MS: u64 = 120;

/// 自定义快捷键单次按住的兜底上限（毫秒）。
///
/// 自定义快捷键按住超时上限（毫秒）。
///
/// 超过该上限的一定是松开事件丢了（BLE 断链、无线丢包等）。此时强制松开，
/// 避免修饰键一直卡在系统里把用户的物理键盘拖成 Ctrl+ 组合键。
/// 仅用于普通按键；麦克风按键（`ButtonId::Mic`）承载语音输入（如豆包等输入法），长按说话
/// 可能持续数十秒至数分钟，正常由物理松手或硬件 `AudioStopped` 释放，仅在极端丢包时
/// 使用专门的宽松兜底（默认 180s）。
const DEFAULT_MAX_COMBO_HOLD_MS: u64 = 10_000;
const DEFAULT_MAX_MIC_HOLD_MS: u64 = 180_000;

/// 本次按下的处理方式（由动作语义决定，见 [`PressSemantics`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PressMode {
    /// 正常手势（单击/双击/长按）。
    #[default]
    Gesture,
    /// 按下即执行一次，不参与手势判定，也不连发（静音、播放暂停）。
    Instant,
    /// 按下即执行，按住由 tick 连发（系统音量 ±）。
    Repeat,
}

/// 单个按键的运行时状态。
#[derive(Default)]
struct ButtonRuntime {
    detector: TriggerDetector,
    down: bool,
    /// 本次按下的处理方式。步进/开关类动作不走手势判定。
    press_mode: PressMode,
    /// 连发模式下下一次执行的时刻（毫秒，调度器时间轴）。
    next_repeat_at: u64,
    /// 本次按下是否启用「简单单击」：该键没有双击绑定，松开即确认单击，
    /// 不需要等双击窗口。手势判定仍保留给显式绑定了双击的按键。
    simple_single: bool,
    /// 简单单击模式下该键是否绑定了长按动作（用于长按阈值判定）。
    simple_has_long: bool,
    /// 本次按住期间是否已触发过长按。
    long_executed: bool,
    /// 自定义快捷键已在物理按下时按住，松开时必须配对松开。
    combo_held: bool,
    /// 按下时实际按住的那组 token，松开时**原样复用**。
    ///
    /// 不能等松开时再解析一遍映射：这期间前台应用配置可能已经变了
    /// （用户切了窗口）或映射被热更新过，解析出来的可能是另一组键、
    /// 甚至另一种动作，于是真正按住的那组键永远等不到松开——修饰键卡死。
    held_tokens: Option<Vec<String>>,
    /// 本次按住开始的时刻（毫秒），用于兜底超时释放。
    hold_started_ms: Option<u64>,
}

struct Inner {
    mapping: MappingConfig,
    /// 应用专属配置（一个应用一个文件）。命中前台进程时覆盖 `mapping`
    /// 中对应的 (按键, 触发)；未覆盖的项继续沿用全局映射。
    ///
    /// 用 `Arc` 是为了能在**不持锁**的情况下取一份快照去查前台窗口——
    /// 查询要走 Win32，而 `GetWindowText` 对自己进程的窗口是 `SendMessage`，
    /// 持锁调用会和正等着取锁的线程互等成死锁（见
    /// [`KeyDispatcher::foreground_profile_snapshot`]）。
    profiles: Arc<ProfileRegistry>,
    /// 虚拟键 -> 物理按键（默认表 + 校准表覆盖）。
    vkey_map: HashMap<u16, ButtonId>,
    buttons: HashMap<ButtonId, ButtonRuntime>,
    enabled: bool,
    /// 输入路由模式：普通映射 / 快捷菜单独占（按键直转菜单回调）。
    mode: InputMode,
    /// 自定义快捷键单次按住的兜底上限（毫秒），见
    /// [`DEFAULT_MAX_COMBO_HOLD_MS`]。
    max_combo_hold_ms: u64,
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
pub(crate) type AppEventHandler = Arc<dyn Fn(AppEvent) + Send + Sync>;

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
    /// 稳定标识：回传 `open_app_profile` 时用它定位配置（就是文件名）。
    ///
    /// **不要改用展示名**：展示名在编辑器里可改，改完快捷菜单就会点空。
    pub id: String,
    /// 展示名，只给人看。
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
                profiles: Arc::new(ProfileRegistry::default()),
                enabled: true,
                mode: InputMode::Normal,
                max_combo_hold_ms: DEFAULT_MAX_COMBO_HOLD_MS,
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
        *self.app_event.lock().unwrap_or_else(|e| e.into_inner()) = handler;
    }

    /// 热更新语音识别目标（连接页切换「识别方案」后调用）。
    pub fn set_voice_target(&self, target: VoiceTarget) {
        *self.voice_target.lock().unwrap_or_else(|e| e.into_inner()) = target;
        core_log::log_line(&format!("[dispatch] 语音识别目标已更新: {:?}", target));
    }

    /// 替换应用专属配置集合（启动时加载，或用户改过文件后重载）。
    pub fn set_profiles(&self, profiles: ProfileRegistry) {
        let count = profiles.profiles().len();
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .profiles = Arc::new(profiles);
        core_log::log_line(&format!("[dispatch] 已加载 {count} 份应用专属配置"));
    }

    /// 取一份「当前前台应用命中的配置」，**全程不持调度器锁**。
    ///
    /// [`feed`](Self::feed) / [`tick_once`](Self::tick_once) 都要在临界区里
    /// 知道「现在该用哪份配置」，但**不能**在持锁时去查前台窗口：
    /// `foreground_process_name` / `foreground_window_title` 走的是 Win32，
    /// 其中 `GetWindowText` 对属于本进程的窗口会 `SendMessage` 一次跨线程同步。
    /// 快捷菜单打开时前台窗口正是我们自己的窗口，于是「持锁 + 等主线程处理消息」
    /// 和「主线程正等着取这把锁」互等，直接把整个应用锁死（实测 AppHang）。
    ///
    /// 所以先短暂取锁把 `Arc` 快照拿出来，放掉锁之后再查。
    fn foreground_profile_snapshot(&self) -> Option<AppProfile> {
        let profiles = {
            self.inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .profiles
                .clone()
        };
        foreground_profile(&profiles).cloned()
    }

    /// 当前前台应用与命中的配置，供诊断页显示 / 核对进程名。
    pub fn foreground_status(&self) -> ForegroundStatus {
        // 同样先取快照再查前台窗口，不在持锁期间碰 Win32。
        let profiles = {
            self.inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .profiles
                .clone()
        };
        let process = core_app_profile::foreground_process_name();
        let title = core_app_profile::foreground_window_title();
        let profile = process
            .as_deref()
            .and_then(|exe| profiles.match_context(exe, title.as_deref().unwrap_or_default()));
        ForegroundStatus {
            profile_count: profiles.profiles().len(),
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
        // 先把要展示的配置拷出来、放掉调度器的锁，再去枚举窗口。
        // `find_window` 要遍历本机所有顶层窗口，不该让按键线程陪着一起等，
        // 更不该在持锁期间做系统调用。
        let profiles: Vec<AppProfile> = {
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            inner.profiles.menu_entries().into_iter().cloned().collect()
        };

        profiles
            .iter()
            .filter_map(|p| {
                let icon = p.icon.as_ref()?;
                Some(MenuAppEntry {
                    id: p.id.clone(),
                    name: p.display_name(),
                    label: icon.label.clone(),
                    color: icon.color.clone(),
                    open: core_app_profile::find_window(p).is_some(),
                })
            })
            .collect()
    }

    /// 当前全部应用配置的快照（编辑器的作用范围选择器与矩阵用它）。
    ///
    /// 返回副本：调用方要遍历它拼 UI 数据，不该占着调度器的锁。
    pub fn profiles_snapshot(&self) -> Vec<AppProfile> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.profiles.profiles().to_vec()
    }

    /// 按 id（文件名）取一份应用配置的副本（供「点图标」与编辑器定位目标用）。
    ///
    /// 返回副本而不是引用：调用方要去做启动窗口这类可能阻塞的事，
    /// 不该一直占着调度器的锁。
    pub fn profile_by_id(&self, id: &str) -> Option<AppProfile> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.profiles.get_by_id(id).cloned()
    }

    /// 当前语音识别目标。
    pub fn voice_target(&self) -> VoiceTarget {
        *self.voice_target.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 切换输入路由模式：快捷菜单独占模式下所有按键绕过触发检测与映射，
    /// 直接以 `AppEvent::MenuKey` 转发给应用事件出口；切回普通模式恢复
    /// 常规调度。切换时清空按键中间状态。
    ///
    /// **切模式前必须先松开已按住的快捷键**：独占模式不处理松开事件，
    /// 带着一组按住的修饰键切进去，那组键就再也没人松开了。
    pub fn set_input_mode(&self, mode: InputMode) {
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.mode == mode {
                return;
            }
            inner.mode = mode;
        }
        self.release_all_held(&format!("输入模式切换到 {mode:?}"));
        core_log::log_line(&format!("[dispatch] 输入模式已切换: {mode:?}"));
    }

    /// 启动动作执行线程与 tick 线程。
    ///
    /// `stats_dir` 为按键统计存储目录（`core-stats`）。
    pub fn spawn_runtime(self: &Arc<Self>, stats_dir: PathBuf) {
        let app_event = self
            .app_event
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let voice_target = self.voice_target.clone();
        if let Some(rx) = self
            .jobs_rx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
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
                    // 单次 tick panic 不能带走整个调度器：长按/单击确认与
                    // 组合键超时兜底都靠这个线程，必须继续跑。
                    let jobs = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dispatcher.tick_once(now)
                    }));
                    match jobs {
                        Ok(jobs) => {
                            for job in jobs {
                                let _ = dispatcher.jobs_tx.send(job);
                            }
                        }
                        Err(payload) => {
                            core_log::log_error(&format!(
                                "[dispatch] tick 处理 panic 已隔离（继续运行）：{}",
                                core_hid::panic_payload_text(payload.as_ref())
                            ));
                        }
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
            let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (inner.mode, inner.vkey_map.get(&vkey).copied())
        };
        if mode == InputMode::QuickMenu {
            if let Some(cb) = self
                .app_event
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
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
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).mapping = mapping;
    }

    /// 热更新校准表（保存校准后调用），并重建虚拟键反查表。
    pub fn update_calibrations(&self, calibrations: &HashMap<String, KeyCalibration>) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .vkey_map = build_vkey_map(calibrations);
    }

    /// 热更新触发判定时间：长按阈值与双击窗口（毫秒）。
    pub fn set_trigger_timing(&self, long_press_ms: u64, double_click_ms: u64) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.enabled == enabled {
                return false;
            }
            inner.enabled = enabled;
        }
        if enabled {
            self.reset_gestures();
        } else {
            // 暂停调度会吞掉后续松开事件：先把已按住的快捷键松开，
            // 否则测试完按键，那组修饰键会一直卡在系统里。
            self.release_all_held("调度已暂停");
        }
        true
    }

    /// 清空所有按键的手势中间状态，**不动已配置的判定时间**。
    ///
    /// 早期实现直接 `*rt = ButtonRuntime::default()`，会把
    /// `long_press_ms` / `double_click_window_ms` 一起打回默认值（550/300），
    /// 于是「暂停一次按键测试」就会悄悄丢掉用户设置的长按阈值。
    fn reset_gestures(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for rt in inner.buttons.values_mut() {
            rt.down = false;
            rt.press_mode = PressMode::Gesture;
            rt.next_repeat_at = 0;
            rt.simple_single = false;
            rt.simple_has_long = false;
            rt.long_executed = false;
            rt.combo_held = false;
            rt.held_tokens = None;
            rt.hold_started_ms = None;
            rt.detector.abort();
        }
    }

    /// 强制松开所有「按住未松开」的按键，返回松开按键的个数。
    ///
    /// 兜底路径专用（调度暂停、切换输入模式、BLE 断链、应用退出）：这些
    /// 情况下物理松开事件永远不会到达，而按下时已注入的键还按在系统里。
    ///
    /// 只对按下时记住了 token 的 `KeyCombo` 生成松开任务；`Voice`（Win+H）
    /// 的 Press 本身就是一次完整点按，没有需要配对松开的系统状态，只清状态。
    pub fn release_all_held(&self, reason: &str) -> usize {
        let mut jobs = Vec::new();
        {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            for (button, rt) in inner.buttons.iter_mut() {
                if !rt.down && !rt.combo_held && rt.held_tokens.is_none() {
                    continue;
                }
                if let Some(tokens) = rt.held_tokens.take() {
                    jobs.push(ActionJob {
                        button: *button,
                        trigger: Trigger::Release,
                        action: ActionKind::KeyCombo(tokens),
                    });
                }
                rt.down = false;
                rt.press_mode = PressMode::Gesture;
                rt.next_repeat_at = 0;
                rt.simple_single = false;
                rt.simple_has_long = false;
                rt.long_executed = false;
                rt.combo_held = false;
                rt.hold_started_ms = None;
                rt.detector.abort();
            }
        }
        if !jobs.is_empty() {
            core_log::log_warn(&format!(
                "[dispatch] 强制松开按住未松开的按键 {} 个（{reason}）",
                jobs.len()
            ));
        }
        for job in &jobs {
            let _ = self.jobs_tx.send(job.clone());
        }
        jobs.len()
    }

    /// 设置自定义快捷键的按住兜底上限（毫秒）。测试用，0 表示不限制。
    pub fn set_max_combo_hold_ms(&self, ms: u64) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .max_combo_hold_ms = ms;
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).enabled
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    /// 注入一个虚拟键事件，返回产生的任务（不发往执行线程）。
    fn feed(&self, vkey: u16, pressed: bool, now: u64) -> Vec<ActionJob> {
        let mut jobs = Vec::new();
        // 先在临界区之外解析前台配置——这一步会调 Win32，持锁调用会死锁。
        let profile = self.foreground_profile_snapshot();

        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
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
        let profile = profile.as_ref();
        let rt = buttons.entry(button).or_default();
        if pressed {
            if rt.down {
                // 系统按住重复：只算一次物理按下
                return jobs;
            }
            rt.down = true;
            rt.long_executed = false;
            rt.combo_held = false;
            rt.held_tokens = None;
            rt.hold_started_ms = Some(now);
            rt.press_mode = PressMode::Gesture;
            rt.next_repeat_at = 0;
            // 步进/开关类动作：按下立即执行一次，不进入单击/双击/长按判定。
            // 但如果用户显式给这个键绑了双击、或绑了不同动作的长按，必须让位给
            // 手势判定——不能把用户配置的双击/长按吞掉。
            let semantics = effective_press_semantics(mapping, profile, button);
            if semantics != PressSemantics::Gesture {
                if let Some(job) = build_job(mapping, profile, button, Trigger::SingleClick, rt) {
                    match semantics {
                        PressSemantics::Repeat => {
                            rt.press_mode = PressMode::Repeat;
                            rt.next_repeat_at = now + rt.detector.long_press_ms();
                            jobs.push(job);
                            return jobs;
                        }
                        PressSemantics::Instant => {
                            rt.press_mode = PressMode::Instant;
                            jobs.push(job);
                            return jobs;
                        }
                        PressSemantics::Gesture => {}
                    }
                }
            }
            // 没有双击绑定的按键不需要等双击窗口：松开即确认单击。
            // 有双击绑定才进入手势判定（单击延迟确认、双击不丢）。
            let has_single =
                resolve_action(mapping, profile, button, Trigger::SingleClick).is_some();
            let has_double =
                resolve_action(mapping, profile, button, Trigger::DoubleClick).is_some();
            rt.simple_single = has_single && !has_double;
            rt.simple_has_long =
                resolve_action(mapping, profile, button, Trigger::LongPress).is_some();
            if !rt.simple_single {
                rt.detector.press(now);
            }
            // 自定义快捷键按住说话：物理按下立即按住，不等长按阈值
            // （豆包等 IME 要的是键盘那种「一按住就生效」）。
            // Voice（Win+H）仍等 tick 识别长按后再发，避免点按误开语音条。
            if let Some(job) = build_job(mapping, profile, button, Trigger::Press, rt) {
                if let ActionKind::KeyCombo(tokens) = &job.action {
                    rt.combo_held = true;
                    // 记住这一次真正按住的键，松开时原样复用。
                    rt.held_tokens = Some(tokens.clone());
                    jobs.push(job);
                }
            }
        } else {
            if !rt.down {
                // 没有对应按下的释放（被抑制的回声），直接忽略
                return jobs;
            }
            let held_ms = rt
                .hold_started_ms
                .map(|started| now.saturating_sub(started))
                .unwrap_or(0);
            rt.down = false;
            rt.hold_started_ms = None;
            if rt.press_mode != PressMode::Gesture {
                // 步进/开关类动作：松手即结束，没有延迟确认的手势事件。
                rt.press_mode = PressMode::Gesture;
                rt.next_repeat_at = 0;
                return jobs;
            }
            if rt.simple_single {
                // 没有双击绑定：松开即确认单击；长按动作仍按阈值生效。
                let has_long = rt.simple_has_long;
                rt.simple_single = false;
                rt.simple_has_long = false;
                if rt.combo_held {
                    rt.combo_held = false;
                    if let Some(job) = release_job(mapping, profile, button, rt) {
                        jobs.push(job);
                    }
                } else if has_long && rt.long_executed {
                    // 长按已在 tick 触发，松开不再补单击。
                } else if has_long && held_ms >= rt.detector.long_press_ms() {
                    // tick 漏掉时的长按兜底。
                    if let Some(job) = build_job(mapping, profile, button, Trigger::LongPress, rt) {
                        jobs.push(job);
                    }
                } else if let Some(job) =
                    build_job(mapping, profile, button, Trigger::SingleClick, rt)
                {
                    jobs.push(job);
                }
                return jobs;
            }
            // 先让检测器确认本次按住是否达到长按阈值（tick 漏掉时兜底）。
            let outcome = rt.detector.release(now);
            if rt.combo_held {
                rt.combo_held = false;
                if let Some(job) = release_job(mapping, profile, button, rt) {
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
        // 同 feed：前台配置在临界区之外解析。
        let profile = self.foreground_profile_snapshot();

        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !inner.enabled {
            return jobs;
        }
        let max_combo_hold_ms = inner.max_combo_hold_ms;
        let Inner {
            mapping, buttons, ..
        } = &mut *inner;
        let profile = profile.as_ref();
        for (button, rt) in buttons.iter_mut() {
            // 步进/开关类动作不走手势判定：步进类按住连发，开关类不做任何 tick。
            match rt.press_mode {
                PressMode::Repeat => {
                    // 映射可能被热更新：中途加了双击/不同动作的长按绑定就退回手势，
                    // 保证用户的显式配置立即优先。
                    if effective_press_semantics(mapping, profile, *button)
                        != PressSemantics::Repeat
                    {
                        rt.press_mode = PressMode::Gesture;
                        continue;
                    }
                    if rt.down && now >= rt.next_repeat_at {
                        if let Some(job) =
                            build_job(mapping, profile, *button, Trigger::SingleClick, rt)
                        {
                            jobs.push(job);
                            rt.next_repeat_at = now + REPEAT_STEP_INTERVAL_MS;
                        } else {
                            rt.press_mode = PressMode::Gesture;
                        }
                    }
                    continue;
                }
                PressMode::Instant => {
                    if !rt.down {
                        rt.press_mode = PressMode::Gesture;
                    }
                    continue;
                }
                PressMode::Gesture => {}
            }
            // 兜底：自定义快捷键按住远超任何合理时长，说明松开事件丢了
            // （BLE 断链等）。强制松开，避免修饰键一直卡在系统里。
            // 麦克风按键承载语音输入（如豆包等输入法），正常由硬件 AudioStopped 释放，
            // 默认采用宽松的 180s 极端故障兜底，绝不在 10s 内掐断长语音。
            let hold_timeout_ms =
                if *button == ButtonId::Mic && max_combo_hold_ms == DEFAULT_MAX_COMBO_HOLD_MS {
                    DEFAULT_MAX_MIC_HOLD_MS
                } else {
                    max_combo_hold_ms
                };
            if hold_timeout_ms > 0 && rt.combo_held {
                if let Some(started) = rt.hold_started_ms {
                    if now.saturating_sub(started) >= hold_timeout_ms {
                        let job = release_job(mapping, profile, *button, rt);
                        core_log::log_warn(&format!(
                            "[dispatch] 按键 {button:?} 按住超过 {}s 未松开，已强制释放（松开事件丢失兜底）",
                            hold_timeout_ms / 1000
                        ));
                        rt.down = false;
                        rt.long_executed = false;
                        rt.combo_held = false;
                        rt.hold_started_ms = None;
                        rt.detector.abort();
                        if let Some(job) = job {
                            jobs.push(job);
                        }
                    }
                }
            }
            if rt.simple_single {
                // 没有双击绑定：单击已在松开时确认，这里只负责长按动作。
                if rt.down && rt.simple_has_long && !rt.long_executed {
                    if let Some(started) = rt.hold_started_ms {
                        if now.saturating_sub(started) >= rt.detector.long_press_ms() {
                            // 先构建任务再置 long_executed：build_job 的长按门控
                            // 会检查这个标志，提前置位会把任务吞掉。
                            let job = build_job(mapping, profile, *button, Trigger::LongPress, rt);
                            rt.long_executed = true;
                            if let Some(job) = job {
                                jobs.push(job);
                            }
                        }
                    }
                }
                continue;
            }
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

/// 构造「松开」任务：优先复用按下时记住的那组键。
///
/// 按下与松开之间，前台应用配置和映射都可能已经变了；如果松开时重新解析
/// 映射，很容易解析出另一组键（甚至另一个动作），结果就是**按下时那组键
/// 永远没人松开**——修饰键卡在系统里，用户的物理键盘跟着遭殃。
///
/// 因此只要记住了 token 就直接用它，连「动作是否被禁用」都不再检查：
/// 多松一次无害，少松一次就是全局卡键。
fn release_job(
    mapping: &MappingConfig,
    profile: Option<&AppProfile>,
    button: ButtonId,
    rt: &mut ButtonRuntime,
) -> Option<ActionJob> {
    if let Some(tokens) = rt.held_tokens.take() {
        return Some(ActionJob {
            button,
            trigger: Trigger::Release,
            action: ActionKind::KeyCombo(tokens),
        });
    }
    build_job(mapping, profile, button, Trigger::Release, rt)
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

/// 当前配置下，这个按键应该使用哪种「按下语义」。
///
/// 步进/开关类动作（音量 ±、静音、播放暂停）默认走即时/连发语义；但只要用户
/// **显式**给该键绑定了双击、或绑定了**不同动作**的长按，就必须退回到手势判定，
/// 否则用户配置的双击/长按会被吞掉。长按与单击绑定同一个动作视为冗余
/// （常见的「长按也是音量+」写法），仍然允许连发。
fn effective_press_semantics(
    mapping: &MappingConfig,
    profile: Option<&AppProfile>,
    button: ButtonId,
) -> PressSemantics {
    let Some(action) = resolve_action(mapping, profile, button, Trigger::SingleClick) else {
        return PressSemantics::Gesture;
    };
    let semantics = action.press_semantics();
    if semantics == PressSemantics::Gesture {
        return PressSemantics::Gesture;
    }
    // 显式双击绑定：必须保留双击判定（单击会被延迟确认）。
    if resolve_action(mapping, profile, button, Trigger::DoubleClick).is_some() {
        return PressSemantics::Gesture;
    }
    // 长按绑定不同动作：保留长按判定；同一动作视为冗余，继续连发。
    if let Some(long_action) = resolve_action(mapping, profile, button, Trigger::LongPress) {
        if long_action != action {
            return PressSemantics::Gesture;
        }
    }
    semantics
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
mod executor;

pub(crate) use executor::execute_loop;

#[cfg(test)]
mod tests;
