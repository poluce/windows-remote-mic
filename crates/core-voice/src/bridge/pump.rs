//! 输出泵：把解码后的音频帧推到 sink，并在空闲/异常时维护会话存活。
//!
//! 退出原因以 `&'static str` 返回，直接写进会话统计。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::diag::BridgeDiag;
use super::ControlSignal;
use crate::ports::{AudioSink, VoiceTransport};
use crate::sink::open_sink;

/// ATVV `MIC_OPEN` 握手实验模式（环境变量 `REMOTE_MIC_ATVV_MIC_OPEN`）。
///
/// 这是**实验开关**，默认 [`MicOpenMode::Off`]，不影响正常使用：
/// - `caps`：收到 CAPS 后立刻发一次 `MIC_OPEN`；
/// - `audio`：设备开始推流（AUDIO_START）后发一次 `MIC_OPEN`；
/// - `idle`：把 20s 空闲保活从能力查询换成 `MIC_OPEN`（验证是否续期）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MicOpenMode {
    Off,
    AfterCaps,
    AfterAudioStart,
    IdleKeepalive,
}

impl MicOpenMode {
    pub(crate) fn from_env() -> Self {
        match std::env::var("REMOTE_MIC_ATVV_MIC_OPEN")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "caps" => Self::AfterCaps,
            "audio" => Self::AfterAudioStart,
            "idle" => Self::IdleKeepalive,
            _ => Self::Off,
        }
    }
}

/// 输出泵里的 ATVV 握手实验上下文（默认关闭时 mode 为 Off）。
pub(crate) struct PumpExperiment {
    pub signals: mpsc::Receiver<ControlSignal>,
    pub mode: MicOpenMode,
}

/// 发送一条 `MIC_OPEN`（实验路径，只打 INFO 日志便于对照）。
fn send_mic_open(transport: &dyn VoiceTransport, version: u16) {
    let cmd = core_atvv::protocol::mic_open_command(version);
    core_log::log_info(&format!(
        "[bridge][试验] 发送 MIC_OPEN 载荷={cmd:02X?}（版本=0x{version:04X}）"
    ));
    if let Err(e) = transport.write_tx(&cmd) {
        core_log::log_warn(&format!("[bridge][试验] 发送 MIC_OPEN 失败（忽略）: {e}"));
    }
}

/// 空闲轮询节拍。取值要短：每一拍都要顺手把会话线程的 COM 套间消息队列泵一次，
/// 否则一旦线程被第三方库初始化成 STA，WinRT 的 GATT 通知会一直压在队列里派发
/// 不出来（详见 `core_ble::com` 模块文档）。
const IDLE_TICK: Duration = Duration::from_millis(200);
/// 每累计空闲多少毫秒做一次链路健康检查并打一条心跳日志（原实现是 30s）。
const HEALTH_EVERY_MS: u64 = 30_000;
/// 心跳日志升级为 INFO 的间隔（原实现是 5 分钟），其余级别为 DEBUG。
const HEARTBEAT_INFO_EVERY_MS: u64 = 300_000;

/// 空闲时向遥控器发一次轻量 ATVV 命令的间隔。
///
/// RC003 在没有任何 ATVV 交互时约 45s 就会主动断链（实测每场会话都精确
/// 45.0s 结束）。空闲时周期性发一次能力查询，既不改状态又能把链路留住，
/// 避免「说话说到一半链路被拆」。
const KEEPALIVE_EVERY_MS: u64 = 20_000;

/// 运行输出泵，直到用户停止 / 链路断开 / 音频通道关闭。
pub(crate) fn run_output_pump(
    frame_rx: &mpsc::Receiver<Vec<f32>>,
    sink: &mut Box<dyn AudioSink>,
    output_device: &str,
    transport: &dyn VoiceTransport,
    disconnected: &AtomicBool,
    diag: &BridgeDiag,
    experiment: PumpExperiment,
) -> &'static str {
    let PumpExperiment {
        signals,
        mode: mic_open_mode,
    } = experiment;
    // 空闲毫秒数：每 IDLE_TICK 累加一次；满 HEALTH_EVERY_MS 做健康检查 + 心跳。
    let mut idle_ms: u64 = 0;
    let mut audio_stream_warned = false;
    let mut sink_reconstructing: Option<Instant> = None;
    let mut pumped_messages: u64 = 0;
    // 协商到的 ATVV 版本（MIC_OPEN 载荷随版本不同）与会话 id（MIC_CLOSE 用）。
    let mut atvv_version: u16 = 0x0100;
    let mut session_id: u8 = 0;
    // 是否处于「遥控器正在推流」状态：MIC_CLOSE 只在 推流→停止 的边沿发一次，
    // 绝不对每一个控制包都发（实测会把遥控器状态机打崩、表现为麦克风键失灵）。
    let mut streaming = false;

    loop {
        // 控制通道信号：CAPS 版本 / 推流开始 / 推流结束。
        for signal in signals.try_iter() {
            match signal {
                ControlSignal::Caps(version) => {
                    atvv_version = version;
                    if mic_open_mode == MicOpenMode::AfterCaps {
                        send_mic_open(transport, atvv_version);
                    }
                }
                ControlSignal::AudioStarted { session_id: sid } => {
                    if let Some(sid) = sid {
                        session_id = sid;
                    }
                    streaming = true;
                    if mic_open_mode == MicOpenMode::AfterAudioStart {
                        send_mic_open(transport, atvv_version);
                    }
                }
                ControlSignal::AudioStopped => {
                    // 只在「推流 → 停止」的边沿发一次 MIC_CLOSE。
                    if streaming && mic_open_mode != MicOpenMode::Off {
                        streaming = false;
                        let cmd = core_atvv::protocol::mic_close_command(atvv_version, session_id);
                        core_log::log_info(&format!(
                            "[bridge][试验] 推流结束，发送 MIC_CLOSE 载荷={cmd:02X?}"
                        ));
                        if let Err(e) = transport.write_tx(&cmd) {
                            core_log::log_warn(&format!(
                                "[bridge][试验] 发送 MIC_CLOSE 失败（忽略）: {e}"
                            ));
                        }
                    }
                }
            }
        }

        if crate::bridge_stop_requested() {
            core_log::log_info("[bridge] 用户断开连接，停止语音桥");
            core_log::log_info(&diag.summary("user_disconnect"));
            return "user_disconnect";
        }
        if disconnected.load(Ordering::SeqCst) {
            core_log::log_line("[bridge] BLE 已断开，停止语音桥以等待自动重连");
            core_log::log_info(&diag.summary("ble_disconnected_flag"));
            return "ble_disconnected_flag";
        }

        match frame_rx.recv_timeout(IDLE_TICK) {
            Ok(frames) => {
                idle_ms = 0;
                if sink_reconstructing.is_none() {
                    sink.push(&frames);
                    if sink.is_failed() {
                        if !audio_stream_warned {
                            audio_stream_warned = true;
                            let detail =
                                sink.last_error().unwrap_or_else(|| "未知原因".to_string());
                            core_log::log_error(&format!(
                                "[bridge] 音频输出流已报错（{detail}）；正在尝试重建输出流"
                            ));
                        }
                        sink_reconstructing = Some(Instant::now());
                    }
                } else {
                    // 处于重建中，平滑丢弃音频帧
                }

                if let Some(last_try) = sink_reconstructing {
                    if last_try.elapsed() >= Duration::from_secs(5) {
                        sink_reconstructing = Some(Instant::now());
                        match open_sink(output_device) {
                            Ok(new_sink) => {
                                *sink = new_sink;
                                audio_stream_warned = false;
                                sink_reconstructing = None;

                                // 发送 10ms 静音帧消除爆音
                                let silence = vec![0.0f32; 480 * 2];
                                sink.push(&silence);

                                core_log::log_warn("[bridge] 音频输出流已重建成功（设备恢复）");
                            }
                            Err(e) => {
                                core_log::log_error(&format!("[bridge] 音频输出流重建失败: {e}"))
                            }
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                idle_ms = idle_ms.saturating_add(IDLE_TICK.as_millis() as u64);
                // 兜底：把会话线程套间队列里的消息派发掉。正常路径（线程为 MTA）
                // 不会有消息，这里返回 0；只有线程被第三方库初始化成 STA 时才有
                // 意义——否则 WinRT 的 GATT 通知会一直积压到会话结束。
                let pumped = core_ble::pump_thread_messages();
                if pumped > 0 {
                    pumped_messages += u64::from(pumped);
                    core_log::log_debug(&format!(
                        "[bridge] 兜底消息泵派发 {pumped} 条套间消息（累计 {pumped_messages}）"
                    ));
                }
                if idle_ms.is_multiple_of(KEEPALIVE_EVERY_MS) {
                    // 链路保活：遥控器空闲约 45s 会主动断链，这里每 20s 发一次
                    // 无副作用的命令把会话留住。实验模式下改发 MIC_OPEN，
                    // 用来验证它是否是设备侧「音频会话超时」的续期命令。
                    if mic_open_mode == MicOpenMode::IdleKeepalive && !streaming {
                        send_mic_open(transport, atvv_version);
                    } else if mic_open_mode != MicOpenMode::IdleKeepalive {
                        match transport.write_tx(&core_atvv::protocol::GET_CAPABILITIES_V10) {
                            Ok(()) => core_log::log_debug("[bridge] 空闲保活：已发送能力查询"),
                            Err(e) => core_log::log_warn(&format!(
                                "[bridge] 空闲保活发送失败（忽略）: {e}"
                            )),
                        }
                    }
                }
                if idle_ms.is_multiple_of(HEALTH_EVERY_MS) {
                    // 事件兜底：WinRT 的 ConnectionStatusChanged 偶尔不送达
                    // （蓝牙无线电被关、驱动卡住等）。每 30s 主动读一次链路状态，
                    // 避免会话永远挂在「以为还连着」的状态里。
                    if !transport.is_connected() {
                        core_log::log_warn(
                            "[bridge] 链路状态轮询发现已断开（未收到状态事件），停止会话",
                        );
                        core_log::log_info(&diag.summary("winrt_status_poll_disconnected"));
                        return "winrt_status_poll_disconnected";
                    }
                    // 每 30s 一条 DEBUG，每 5 分钟一条 INFO：之前每 30s 一条 INFO
                    // 占了日志的大头，把真正的异常淹没了。
                    let last_ctrl = diag.last_control_at();
                    let last_aud = diag.last_audio_at();
                    let message = format!(
                        "[bridge] 链路空闲心跳: 主循环已空闲 {}s, 距上次控制={}, 距上次音频={}, 语音输入中={}, ConnectionActive={}",
                        idle_ms / 1000,
                        BridgeDiag::age_label(last_ctrl),
                        BridgeDiag::age_label(last_aud),
                        diag.voice_active.load(Ordering::Relaxed),
                        crate::connection_active(),
                    );
                    if idle_ms.is_multiple_of(HEARTBEAT_INFO_EVERY_MS) {
                        core_log::log_info(&message);
                    } else {
                        core_log::log_debug(&message);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                core_log::log_warn("[bridge] 音频帧通道已断开（发送端丢弃），停止语音桥");
                core_log::log_info(&diag.summary("audio_channel_disconnected"));
                return "audio_channel_disconnected";
            }
        }
    }
}
