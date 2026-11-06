//! 输出泵：把解码后的音频帧推到 sink，并在空闲/异常时维护会话存活。
//!
//! 退出原因以 `&'static str` 返回，直接写进会话统计。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::diag::BridgeDiag;
use crate::ports::{AudioSink, VoiceTransport};
use crate::sink::open_sink;

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
) -> &'static str {
    // 空闲毫秒数：每 IDLE_TICK 累加一次；满 HEALTH_EVERY_MS 做健康检查 + 心跳。
    let mut idle_ms: u64 = 0;
    let mut audio_stream_warned = false;
    let mut last_sink_recover: Option<Instant> = None;
    let mut pumped_messages: u64 = 0;

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

        match frame_rx.recv_timeout(IDLE_TICK) {
            Ok(frames) => {
                idle_ms = 0;
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
                    // 无副作用的能力查询把会话留住。失败只告警，不终止会话。
                    match transport.write_tx(&core_atvv::protocol::GET_CAPABILITIES_V10) {
                        Ok(()) => core_log::log_debug("[bridge] 空闲保活：已发送能力查询"),
                        Err(e) => {
                            core_log::log_warn(&format!("[bridge] 空闲保活发送失败（忽略）: {e}"))
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
