//! 输出泵：把解码后的音频帧推到 sink，并在空闲/异常时维护会话存活。
//!
//! 退出原因以 `&'static str` 返回，直接写进会话统计。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::diag::BridgeDiag;
use crate::ports::{AudioSink, VoiceTransport};
use crate::sink::open_sink;

/// 运行输出泵，直到用户停止 / 链路断开 / 音频通道关闭。
pub(crate) fn run_output_pump(
    frame_rx: &mpsc::Receiver<Vec<f32>>,
    sink: &mut Box<dyn AudioSink>,
    output_device: &str,
    transport: &dyn VoiceTransport,
    disconnected: &AtomicBool,
    diag: &BridgeDiag,
) -> &'static str {
    // 每 2s timeout 计一次空闲心跳；满 30s 打一条日志，便于对照「空闲多久后断链」。
    let mut idle_ticks: u32 = 0;
    let mut audio_stream_warned = false;
    let mut last_sink_recover: Option<Instant> = None;

    loop {
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

        match frame_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(frames) => {
                idle_ticks = 0;
                sink.push(&frames);
                // 输出流报错（CABLE Input 被禁用/拔出等）时提示，并尝试重建流。
                if sink.is_failed() {
                    if !audio_stream_warned {
                        audio_stream_warned = true;
                        let detail = sink.last_error().unwrap_or_else(|| "未知原因".to_string());
                        core_log::log_error(&format!(
                            "[bridge] 音频输出流已报错（{detail}）；正在尝试重建输出流"
                        ));
                    }
                    // 每 5 秒最多重建一次，避免设备持续异常时刷屏/抖动。
                    let due = last_sink_recover
                        .map(|t| t.elapsed() >= Duration::from_secs(5))
                        .unwrap_or(true);
                    if due {
                        last_sink_recover = Some(Instant::now());
                        match open_sink(output_device) {
                            Ok(new_sink) => {
                                *sink = new_sink;
                                audio_stream_warned = false;
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
                idle_ticks = idle_ticks.saturating_add(1);
                // 事件兜底：WinRT 的 ConnectionStatusChanged 偶尔不送达
                // （蓝牙无线电被关、驱动卡住等）。每 30s 主动读一次链路状态，
                // 避免会话永远挂在「以为还连着」的状态里。
                if idle_ticks.is_multiple_of(15) && !transport.is_connected() {
                    core_log::log_warn(
                        "[bridge] 链路状态轮询发现已断开（未收到状态事件），停止会话",
                    );
                    core_log::log_info(&diag.summary("winrt_status_poll_disconnected"));
                    return "winrt_status_poll_disconnected";
                }
                // 15 * 2s = 30s：DEBUG 级别每 30s 一条，INFO 每 5 分钟一条。
                // 之前每 30s 一条 INFO 占了日志的大头，把真正的异常淹没了。
                if idle_ticks > 0 && idle_ticks.is_multiple_of(15) {
                    let last_ctrl = diag.last_control_at();
                    let last_aud = diag.last_audio_at();
                    let message = format!(
                        "[bridge] 链路空闲心跳: 主循环已空闲 {}s, 距上次控制={}, 距上次音频={}, 语音输入中={}, ConnectionActive={}",
                        idle_ticks * 2,
                        BridgeDiag::age_label(last_ctrl),
                        BridgeDiag::age_label(last_aud),
                        diag.voice_active.load(Ordering::Relaxed),
                        crate::connection_active(),
                    );
                    if idle_ticks.is_multiple_of(150) {
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
