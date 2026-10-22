//! 语音桥通知回调：ATVV 音频/控制字节 -> 解码 / 诊断 / 捕获 / 输出通道。
//!
//! 回调在这里构造并注册，主循环只消费解码后的音频帧（见 [`super::pump`]），
//! 避免把「解析 / 解码 / 录制 / 诊断」混进会话编排。

use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use core_atvv::protocol::{parse_control, ControlEvent, RawControlEvent};
use core_ble::capture::CaptureRecorder;

use super::diag::BridgeDiag;
use crate::ports::{KeySink, VoiceTransport};
use crate::VoiceEngine;

/// 注册音频通知回调：原始 ATVV 音频帧 -> ADPCM 解码 -> 输出帧通道。
pub(crate) fn register_audio_handler(
    transport: &dyn VoiceTransport,
    engine: std::sync::Arc<Mutex<VoiceEngine>>,
    capture: CaptureRecorder,
    diag: Arc<BridgeDiag>,
    frame_tx: mpsc::Sender<Vec<f32>>,
) -> Result<i64, String> {
    // 逻辑时钟：VoiceEngine 的时间语义（STOP_TAIL_DROP 等）依赖它。
    // 之前从不推进，导致所有基于时间的门控实际失效。
    let feed_clock = Arc::new(Mutex::new(Instant::now()));
    transport
        .register_audio_handler(Box::new(move |bytes| {
            diag.touch_audio();
            capture.record("audio", &bytes);
            let mut eng = match engine.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if let Ok(mut clock) = feed_clock.lock() {
                let now = Instant::now();
                let elapsed_ms = now.duration_since(*clock).as_millis() as u64;
                *clock = now;
                // 限制单次推进量，避免长时间空闲后一次跳太多导致时序误判。
                eng.advance(elapsed_ms.min(1000));
            }
            let chunk = eng.feed(&bytes);
            if !chunk.output.is_empty() {
                core_log::log_debug(&format!(
                    "[bridge] 音频块解码完成：{} 采样 -> 输出 {} 采样",
                    chunk.pcm_samples, chunk.output_samples
                ));
                let _ = frame_tx.send(chunk.output);
            }
        }))
        .map_err(|e| {
            core_log::log_error(&format!("[bridge] 注册音频回调失败: {e}"));
            e
        })
}

/// 注册控制通知回调：CAPS / 麦克风事件 / 音频开始停止。
pub(crate) fn register_control_handler(
    transport: &dyn VoiceTransport,
    engine: std::sync::Arc<Mutex<VoiceEngine>>,
    capture: CaptureRecorder,
    diag: Arc<BridgeDiag>,
    is_active: Arc<Mutex<bool>>,
    key_sink: Arc<dyn KeySink>,
) -> Result<i64, String> {
    transport
        .register_control_handler(Box::new(move |bytes| {
            diag.touch_control();
            capture.record("control", &bytes);
            core_log::log_debug(&format!("[bridge] 收到控制通知: {:02X?}", bytes));
            let mut eng = match engine.lock() {
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
                    core_log::log_info(&format!(
                        "[bridge] CAPS: version=0x{:04X}, codecs=0x{:02X}, frame_size={}B, {}Hz",
                        caps.version, caps.codecs, caps.frame_size, caps.sample_rate_hz
                    ));
                    if caps.sample_rate_hz != core_atvv::protocol::REMOTE_SAMPLE_RATE_HZ {
                        core_log::log_warn(&format!(
                            "[bridge] 不支持的 ATVV 采样率: {}",
                            caps.sample_rate_hz
                        ));
                    }
                    // 不同固件的音频帧长可能是 120/126/134/160 字节：
                    // 按 CAPS 协商结果适配，避免错位切帧产生噪音。
                    eng.set_frame_bytes(caps.frame_size);
                }
                RawControlEvent::MicButtonPressed => {
                    // 语音 toggle 已由 HID 兜底键（usage 0x3E -> 调度器 Voice 动作）
                    // 单一持有；控制通道事件仅计数，不再切换语音输入，避免双源。
                    diag.touch_mic();
                    core_log::log_debug(
                        "[bridge] 控制通道麦克风事件（语音 toggle 由 HID 兜底处理，忽略）",
                    );
                }
                RawControlEvent::AudioStarted { .. } => {
                    let _ = eng.on_control(ControlEvent::StreamStart);
                    // 遥控器开始推音频：同步诊断状态（心跳里的「语音输入中」）。
                    if let Ok(mut active) = is_active.lock() {
                        *active = true;
                    }
                    diag.set_voice_active(true);
                }
                RawControlEvent::AudioStopped => {
                    // 遥控器硬件停止通知：只停解码状态，不关语音条。
                    // 松手后识别需要时间收尾上屏，关闭动作留给用户手动。
                    if let Ok(mut active) = is_active.lock() {
                        if *active {
                            *active = false;
                            diag.set_voice_active(false);
                            core_log::log_line(
                                "[bridge] 遥控器 AudioStopped -> 停止解码（语音条保持打开）",
                            );
                            let _ = eng.on_control(ControlEvent::StreamStop);
                        }
                    }
                    // 联动释放麦克风按键：遥控器已停止推流（硬件已松手）。
                    // 若真实 HID 松开报告因无线丢包丢失，AudioStopped 兜底补发释放。
                    key_sink.release_mic_on_audio_stop();
                }
                RawControlEvent::AudioSynced { .. } => {
                    // TODO: 将 predictor/step_index 提供给解码器以重新同步。
                }
                RawControlEvent::Unknown(_) => {}
            }
        }))
        .map_err(|e| e.to_string())
}
