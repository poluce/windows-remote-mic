//! 仅 Windows 的真实设备桥接：BLE -> 解码 -> CABLE 输出 -> Win+H。

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

use core_atvv::protocol::{parse_control, ControlEvent, RawControlEvent, GET_CAPABILITIES_V10};
use core_ble::capture::CaptureRecorder;
use core_ble::gatt::AtvvLink;
use core_input::{press_escape, press_win_h};

use crate::VoiceEngine;

/// 单次语音桥会话的断链诊断快照（活动时间戳 / 包计数）。
struct BridgeDiag {
    started_at: Instant,
    last_control_at: Mutex<Option<Instant>>,
    last_audio_at: Mutex<Option<Instant>>,
    last_mic_at: Mutex<Option<Instant>>,
    control_pkts: AtomicU64,
    audio_pkts: AtomicU64,
    voice_active: AtomicBool,
}

impl BridgeDiag {
    fn new() -> Self {
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

    fn touch_control(&self) {
        self.control_pkts.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut g) = self.last_control_at.lock() {
            *g = Some(Instant::now());
        }
    }

    fn touch_audio(&self) {
        self.audio_pkts.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut g) = self.last_audio_at.lock() {
            *g = Some(Instant::now());
        }
    }

    fn touch_mic(&self) {
        if let Ok(mut g) = self.last_mic_at.lock() {
            *g = Some(Instant::now());
        }
    }

    fn set_voice_active(&self, active: bool) {
        self.voice_active.store(active, Ordering::Relaxed);
    }

    fn age_label(since: Option<Instant>) -> String {
        match since {
            Some(t) => format!("{:.1}s", t.elapsed().as_secs_f32()),
            None => "从未".into(),
        }
    }

    fn summary(&self, reason: &str) -> String {
        let last_control = self.last_control_at.lock().ok().and_then(|g| *g);
        let last_audio = self.last_audio_at.lock().ok().and_then(|g| *g);
        let last_mic = self.last_mic_at.lock().ok().and_then(|g| *g);
        format!(
            "[bridge] 断链诊断: reason={reason}, 会话时长={:.1}s, 距上次控制通知={}, 距上次音频={}, 距上次麦克风事件={}, 控制包={}, 音频包={}, 语音输入中={}, bridge_running={}",
            self.started_at.elapsed().as_secs_f32(),
            Self::age_label(last_control),
            Self::age_label(last_audio),
            Self::age_label(last_mic),
            self.control_pkts.load(Ordering::Relaxed),
            self.audio_pkts.load(Ordering::Relaxed),
            self.voice_active.load(Ordering::Relaxed),
            crate::bridge_running(),
        )
    }
}

/// 使用真实的 ATVV 控制事件运行语音桥接。
///
/// BLE 连接建立时以 `true` 调用 `on_status`，断开时以 `false` 调用。
pub fn run_bridge<F>(device_id: &str, output_device: &str, on_status: F) -> Result<(), String>
where
    F: Fn(bool) + Send + 'static,
{
    core_log::log_info(&format!(
        "[bridge] starting voice bridge for device_id='{device_id}', output='{output_device}'"
    ));
    if crate::bridge_running() {
        core_log::log_warn(
            "[bridge] 启动时 bridge_running 已为 true（疑似并发双桥，可能争用同一 GATT 会话）",
        );
    }

    let link = AtvvLink::connect(device_id).map_err(|e| {
        core_log::log_error(&format!("[bridge] 连接 ATVV 链路失败: {e}"));
        e.to_string()
    })?;
    core_log::log_info("[bridge] ATVV 链路已连接");
    crate::set_connection_active(true);
    crate::set_bridge_running(true);
    // ATVV 端点已由 connect_rc003 发现并传入，这里标记为就绪。
    crate::set_atvv_endpoints_ready(true);

    let diag = Arc::new(BridgeDiag::new());
    let disconnected = Arc::new(AtomicBool::new(false));
    let disconnected_cb = disconnected.clone();
    let diag_cb = diag.clone();
    link.register_connection_status_changed(move |connected| {
        let msg = if connected {
            "connected"
        } else {
            "disconnected"
        };
        core_log::log_line(&format!("[bridge] BLE 连接状态变化: {msg}"));
        crate::set_connection_active(connected);
        if !connected {
            // 先打诊断再置位，确保主循环退出日志能读到同一快照语境。
            core_log::log_info(&diag_cb.summary("winrt_ConnectionStatusChanged"));
            disconnected_cb.store(true, Ordering::SeqCst);
        }
        on_status(connected);
    })
    .map_err(|e| e.to_string())?;
    core_log::log_info("[bridge] BLE 连接状态监听已注册");

    link.enable_audio_notifications().map_err(|e| {
        core_log::log_error(&format!("[bridge] 启用音频通知失败: {e}"));
        e.to_string()
    })?;
    core_log::log_info("[bridge] 音频通知已启用");

    link.enable_control_notifications().map_err(|e| {
        core_log::log_error(&format!("[bridge] 启用控制通知失败: {e}"));
        e.to_string()
    })?;
    core_log::log_info("[bridge] 控制通知已启用");

    link.write_tx(&GET_CAPABILITIES_V10).map_err(|e| {
        core_log::log_error(&format!(
            "[bridge] 发送能力查询（GET_CAPABILITIES_V10）失败: {e}"
        ));
        e.to_string()
    })?;
    core_log::log_info("[bridge] 已向遥控器发送能力查询（GET_CAPABILITIES_V10）");

    // ATVV 通知已生效。仅在此时启动可选的 Back/Volume 轻触注入，
    // 以免 HOGP 注入在特征设置期间抢占 GATT 会话。
    core_hid::tap::start_after_atvv();

    let sink = core_audio::sink::AudioSink::new(Some(output_device)).map_err(|e| {
        core_log::log_error(&format!("[bridge] 初始化音频输出（AudioSink）失败: {e}"));
        e.to_string()
    })?;
    core_log::log_info(&format!(
        "[bridge] 音频输出（AudioSink）已初始化：{output_device}"
    ));

    let capture_dir = std::env::var("LOCALAPPDATA")
        .map(|base| std::path::Path::new(&base).join("RemoteMic/RC003/captures"))
        .unwrap_or_default();
    let capture = CaptureRecorder::new(capture_dir);
    let engine = Arc::new(Mutex::new(VoiceEngine::new()));
    let is_active = Arc::new(Mutex::new(false));

    let (frame_tx, frame_rx) = mpsc::channel::<Vec<f32>>();
    let engine_cb = engine.clone();
    let capture_audio = capture.clone();
    let diag_audio = diag.clone();
    let _audio_cookie = link
        .register_audio_handler(move |bytes| {
            diag_audio.touch_audio();
            capture_audio.record("audio", &bytes);
            let mut eng = match engine_cb.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let chunk = eng.feed(&bytes);
            if !chunk.output.is_empty() {
                core_log::log_debug(&format!(
                    "[bridge] 音频块解码完成：{} 采样 -> 输出 {} 采样",
                    chunk.pcm_samples, chunk.output_samples
                ));
                let _ = frame_tx.send(chunk.output);
            }
        })
        .map_err(|e| {
            core_log::log_error(&format!("[bridge] 注册音频回调失败: {e}"));
            e.to_string()
        })?;
    core_log::log_info("[bridge] 音频回调已注册");

    let engine_ctrl = engine.clone();
    let capture_ctrl = capture.clone();
    let is_active_ctrl = is_active.clone();
    let diag_ctrl = diag.clone();
    let _control_cookie = link
        .register_control_handler(move |bytes| {
            diag_ctrl.touch_control();
            capture_ctrl.record("control", &bytes);
            core_log::log_debug(&format!("[bridge] 收到控制通知: {:02X?}", bytes));
            let mut eng = match engine_ctrl.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let Some(event) = parse_control(&bytes) else {
                core_log::log_warn(&format!("[bridge] 无法识别的控制包: {:02X?}", bytes));
                return;
            };
            core_log::log_debug(&format!("[bridge] 已解析控制事件: {:?}", event));
            match event {
                RawControlEvent::Caps(caps) => {
                    if caps.sample_rate_hz != core_atvv::protocol::REMOTE_SAMPLE_RATE_HZ {
                        core_input::log_warn(&format!(
                            "[bridge] 不支持的 ATVV 采样率: {}",
                            caps.sample_rate_hz
                        ));
                    }
                }
                RawControlEvent::MicButtonPressed => {
                    diag_ctrl.touch_mic();
                    // Toggle 模式：按一下开启语音输入，再次点击关闭语音输入
                    let mut active = match is_active_ctrl.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    if !*active {
                        *active = true;
                        diag_ctrl.set_voice_active(true);
                        core_input::log_line("[bridge] 麦克风按键 -> 开启语音输入 (Win+H)");
                        let _ = press_escape();
                        std::thread::sleep(Duration::from_millis(100));
                        if let Err(e) = press_win_h() {
                            core_input::log_error(&format!("[bridge] Win+H 开启失败: {e}"));
                        }
                        let _ = eng.on_control(ControlEvent::StreamStart);
                    } else {
                        *active = false;
                        diag_ctrl.set_voice_active(false);
                        core_input::log_line(
                            "[bridge] 麦克风按键再次点击 -> 关闭语音输入 (Escape)",
                        );
                        let _ = eng.on_control(ControlEvent::StreamStop);
                        if let Err(e) = press_escape() {
                            core_input::log_error(&format!("[bridge] 关闭语音输入失败: {e}"));
                        }
                    }
                }
                RawControlEvent::AudioStarted { .. } => {
                    let _ = eng.on_control(ControlEvent::StreamStart);
                }
                RawControlEvent::AudioStopped => {
                    // 遥控器硬件停止通知
                    if let Ok(mut active) = is_active_ctrl.lock() {
                        if *active {
                            *active = false;
                            diag_ctrl.set_voice_active(false);
                            core_input::log_line(
                                "[bridge] 遥控器 AudioStopped -> 关闭语音输入 (Escape)",
                            );
                            let _ = eng.on_control(ControlEvent::StreamStop);
                            if let Err(e) = press_escape() {
                                core_input::log_error(&format!("[bridge] 关闭语音输入失败: {e}"));
                            }
                        }
                    }
                }
                RawControlEvent::AudioSynced { .. } => {
                    // TODO: 将 predictor/step_index 提供给解码器以重新同步。
                }
                RawControlEvent::Unknown(_) => {}
            }
        })
        .map_err(|e| e.to_string())?;

    // 主循环：保持链路存活，并将解码后的帧推送到 sink。
    // 每 2s timeout 计一次空闲心跳；满 30s 打一条 INFO，便于对照“空闲多久后断链”。
    let mut idle_ticks: u32 = 0;
    let exit_reason;
    loop {
        if disconnected.load(Ordering::SeqCst) {
            exit_reason = "ble_disconnected_flag";
            core_log::log_line("[bridge] BLE 已断开，停止语音桥以等待自动重连");
            core_log::log_info(&diag.summary(exit_reason));
            break;
        }
        match frame_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(frames) => {
                idle_ticks = 0;
                sink.push(&frames);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                idle_ticks = idle_ticks.saturating_add(1);
                // 15 * 2s = 30s
                if idle_ticks > 0 && idle_ticks % 15 == 0 {
                    let last_ctrl = diag.last_control_at.lock().ok().and_then(|g| *g);
                    let last_aud = diag.last_audio_at.lock().ok().and_then(|g| *g);
                    core_log::log_info(&format!(
                        "[bridge] 链路空闲心跳: 主循环已空闲 {}s, 距上次控制={}, 距上次音频={}, 语音输入中={}, ConnectionActive={}",
                        idle_ticks * 2,
                        BridgeDiag::age_label(last_ctrl),
                        BridgeDiag::age_label(last_aud),
                        diag.voice_active.load(Ordering::Relaxed),
                        crate::connection_active(),
                    ));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                exit_reason = "audio_channel_disconnected";
                core_log::log_warn("[bridge] 音频帧通道已断开（发送端丢弃），停止语音桥");
                core_log::log_info(&diag.summary(exit_reason));
                break;
            }
        }
    }

    // 语音桥会话结束，标记链路不再活动（由外层自动重连再次置 true）。
    core_log::log_info(&format!(
        "[bridge] 会话结束: exit_reason={exit_reason}, 即将 Drop AtvvLink（若随后看到 ble/gatt Drop 日志可对照时序）"
    ));
    crate::set_connection_active(false);
    crate::set_bridge_running(false);

    Ok(())
}
