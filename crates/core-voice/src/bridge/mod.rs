//! 仅 Windows 的真实设备桥接：BLE -> 解码 -> CABLE 输出 -> Win+H。
//!
//! 编排只负责「建立会话 + 注册回调 + 启动输出泵 + 收尾」；具体职责拆分到：
//! - [`diag`]：断链诊断（时间戳 / 包计数 / 会话 summary）；
//! - [`handlers`]：ATVV 音频/控制通知回调（解码 / 捕获 / 诊断 / 输出通道）；
//! - [`pump`]：输出泵（推 sink、失败重建、空闲心跳、退出原因）；
//! - [`report`]：会话统计快照（供监督线程判断「链路通但零数据」）。
//!
//! 传输 / 音频输出 / 按键注入通过 [`crate::ports`] 的端口 trait 使用，
//! 真实实现见 [`crate::transport`] / [`crate::sink`] / [`crate::keys`]。

mod diag;
mod handlers;
mod pump;
mod report;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use core_atvv::protocol::GET_CAPABILITIES_V10;
use core_ble::capture::CaptureRecorder;

use crate::keys::system_key_sink;
use crate::ports::{AudioSink, KeySink, VoiceTransport};
use crate::sink::open_sink;
use crate::transport::AtvvTransport;
use crate::VoiceEngine;

use diag::BridgeDiag;
pub use report::{last_session_report, SessionReport};

/// 控制通道 → 输出泵的会话信号（供 ATVV `MIC_OPEN` 握手实验使用）。
///
/// 实验由环境变量 `REMOTE_MIC_ATVV_MIC_OPEN` 控制，取值见
/// [`pump::MicOpenMode::from_env`]；默认关闭，不影响正常使用。
#[derive(Debug, Clone, Copy)]
pub(crate) enum ControlSignal {
    /// 遥控器回报的能力协商结果（含协议版本号）。
    Caps(u16),
    /// 遥控器开始推流（按键被按下）。
    AudioStarted { session_id: Option<u8> },
    /// 遥控器停止推流（按键被松开）。
    AudioStopped,
}

/// 标记会话已建立，并通知外部（UI）。
///
/// # 顺序很重要
///
/// 必须先置位「端点就绪」，再调用 `on_status(true)`。外层回调会立即生成一份
/// `bridge-status` 快照（其中包含 `endpoints_ready`）；如果先回调后置位，
/// 快照就会读到旧值 `false`，UI 出现「语音桥运行中 + ATVV 端点未就绪」的
/// 自相矛盾状态（v0.6.0 实测过，用 v0.6.2 修复）。
fn mark_session_started<F: Fn(bool)>(on_status: &F) {
    crate::set_connection_active(true);
    crate::set_bridge_session_active(true);
    // ATVV 端点已由 connect_rc003 / AtvvLink::connect 发现，这里标记为就绪。
    crate::set_atvv_endpoints_ready(true);
    // 连接建立时连接状态事件不一定再触发（注册回调前链路已是 Connected），
    // 这里主动回调一次，保证 UI 能从「启动中」切换到「运行中」。
    on_status(true);
}

/// 使用真实的 ATVV 控制事件运行语音桥接。
///
/// BLE 连接建立时以 `true` 调用 `on_status`，断开时以 `false` 调用。
pub fn run_bridge<F>(device_id: &str, output_device: &str, on_status: F) -> Result<Option<&'static str>, String>
where
    F: Fn(bool) + Send + 'static,
{
    // COM 套间必须在**任何** WinRT 调用之前定死。会话线程原本没有任何 COM
    // 初始化，直到 open_sink() 打开 WASAPI 时才被 cpal 隐式初始化为 STA，
    // 而 STA 的 WinRT 事件需要泵消息才会派发——输出泵从不泵消息，导致整场
    // 会话的 ATVV 通知被压在套间队列里，直到会话结束才被一次性冲出。
    // 详见 core_ble::com 模块文档。
    let apartment = core_ble::ensure_com_mta();

    core_log::log_info(&format!(
        "[bridge] starting voice bridge for device_id='{device_id}', output='{output_device}'"
    ));
    core_log::log_debug(&format!(
        "[bridge] 会话线程 COM 套间={apartment}，线程={:?}",
        std::thread::current().id()
    ));
    if crate::bridge_session_active() {
        core_log::log_warn(
            "[bridge] 启动时已有语音桥会话在运行（疑似并发双桥，可能争用同一 GATT 会话）",
        );
    }

    // 传输 / 按键 / 音频输出都通过端口 trait 使用真实实现。
    let transport: Box<dyn VoiceTransport> =
        Box::new(AtvvTransport::connect(device_id).map_err(|e| {
            core_log::log_error(&format!("[bridge] 连接 ATVV 链路失败: {e}"));
            e
        })?);
    let key_sink: Arc<dyn KeySink> = Arc::from(system_key_sink());
    core_log::log_info("[bridge] ATVV 链路已连接");
    mark_session_started(&on_status);

    let diag = Arc::new(BridgeDiag::new());
    let disconnected = Arc::new(AtomicBool::new(false));
    let disconnected_cb = disconnected.clone();
    let diag_cb = diag.clone();
    let key_sink_conn = key_sink.clone();
    transport
        .register_connection_handler(Box::new(move |connected| {
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
                // 蓝牙断链瞬间立即清空驱动层活跃按键并重置虚拟 HID，防止修饰键残留
                key_sink_conn.clear_active_usages();
                key_sink_conn.release_all_keys();
            }
            on_status(connected);
        }))
        .map_err(|e| e.to_string())?;
    core_log::log_debug("[bridge] BLE 连接状态监听已注册");

    // ---- 回调必须早于订阅 ----
    //
    // 这里是「先挂处理器、再写 CCCD / 发能力查询」。反过来做（先订阅再注册）
    // 存在一个真实丢包窗口：遥控器回 CAPS 很快（实测 10~30ms），首包完全可能
    // 落在「订阅已生效、回调还没挂」的空档里被直接丢掉。
    let capture_dir = std::env::var("LOCALAPPDATA")
        .map(|base| std::path::Path::new(&base).join("RemoteMic/RC003/captures"))
        .unwrap_or_default();
    let capture = CaptureRecorder::new(capture_dir);
    let engine = Arc::new(Mutex::new(VoiceEngine::new()));
    let is_active = Arc::new(Mutex::new(false));

    let (frame_tx, frame_rx) = mpsc::channel::<Vec<f32>>();
    // 控制回调 → 输出泵的会话信号（MIC_OPEN 实验用；默认模式下仅用于日志统计）。
    let (signal_tx, signal_rx) = mpsc::channel::<ControlSignal>();
    // 回调注册会消费 key_sink，但后面还要用它启动返回/音量旁路，先留一份。
    let key_sink_for_start = key_sink.clone();
    let _audio_cookie = handlers::register_audio_handler(
        &*transport,
        engine.clone(),
        capture.clone(),
        diag.clone(),
        key_sink.clone(),
        frame_tx,
    )?;
    core_log::log_debug("[bridge] 音频回调已注册");

    let _control_cookie = handlers::register_control_handler(
        &*transport,
        engine,
        capture,
        diag.clone(),
        is_active,
        key_sink,
        signal_tx,
    )?;
    core_log::log_debug("[bridge] 控制回调已注册");

    transport.enable_audio_notifications().map_err(|e| {
        core_log::log_error(&format!("[bridge] 启用音频通知失败: {e}"));
        e
    })?;
    core_log::log_debug("[bridge] 音频通知已启用");

    transport.enable_control_notifications().map_err(|e| {
        core_log::log_error(&format!("[bridge] 启用控制通知失败: {e}"));
        e
    })?;
    core_log::log_debug("[bridge] 控制通知已启用");

    transport.write_tx(&GET_CAPABILITIES_V10).map_err(|e| {
        core_log::log_error(&format!(
            "[bridge] 发送能力查询（GET_CAPABILITIES_V10）失败: {e}"
        ));
        e
    })?;
    core_log::log_debug("[bridge] 已向遥控器发送能力查询（GET_CAPABILITIES_V10）");

    // ATVV 通知已生效。仅在此时启动可选的 Back/Volume 轻触注入，
    // 以免 HOGP 注入在特征设置期间抢占 GATT 会话。
    key_sink_for_start.start_after_atvv();
    let mut sink: Box<dyn AudioSink> = open_sink(output_device).map_err(|e| {
        core_log::log_error(&format!("[bridge] 初始化音频输出（AudioSink）失败: {e}"));
        e
    })?;
    core_log::log_info(&format!(
        "[bridge] 音频输出（AudioSink）已初始化：{output_device}（{}）",
        sink.format_label()
    ));

    let mic_open_mode = pump::MicOpenMode::from_env();
    if mic_open_mode != pump::MicOpenMode::Off {
        core_log::log_warn(&format!(
            "[bridge][试验] ATVV MIC_OPEN 握手实验已开启：{mic_open_mode:?}（环境变量 REMOTE_MIC_ATVV_MIC_OPEN）"
        ));
    }

    let exit_reason = pump::run_output_pump(
        &frame_rx,
        &mut sink,
        output_device,
        &*transport,
        &disconnected,
        &diag,
        pump::PumpExperiment {
            signals: signal_rx,
            mode: mic_open_mode,
        },
    );

    // 实验路径：会话收尾时补一条 MIC_CLOSE，观察遥控器状态机是否因此归位。
    if mic_open_mode != pump::MicOpenMode::Off {
        let cmd = core_atvv::protocol::mic_close_command(0x0100, 0);
        core_log::log_info(&format!(
            "[bridge][试验] 会话结束，发送 MIC_CLOSE 载荷={cmd:02X?}"
        ));
        if let Err(e) = transport.write_tx(&cmd) {
            core_log::log_warn(&format!("[bridge][试验] 发送 MIC_CLOSE 失败（忽略）: {e}"));
        }
    }

    // 语音桥会话结束，标记链路不再活动（由外层自动重连再次置 true）。
    core_log::log_info(&format!(
        "[bridge] 会话结束: exit_reason={exit_reason}, 即将 Drop AtvvLink（若随后看到 ble/gatt Drop 日志可对照时序）"
    ));
    crate::set_connection_active(false);
    crate::set_bridge_session_active(false);

    // 留一份会话统计给外层重连循环：用于识别「链路已连接但从未收到任何
    // 控制/音频通知」的半死状态（实测遥控器语音固件挂起时会连续数小时如此）。
    report::store_report(SessionReport {
        duration_secs: diag.started_at.elapsed().as_secs_f32(),
        control_pkts: diag.control_pkts.load(Ordering::Relaxed),
        audio_pkts: diag.audio_pkts.load(Ordering::Relaxed),
        exit_reason,
    });

    Ok(Some(exit_reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_ready_is_set_before_running_callback() {
        // 模拟一次会话建立；回调里检查 UI 快照会读到的状态。
        crate::set_atvv_endpoints_ready(false);
        let seen = Arc::new(AtomicBool::new(false));
        let seen_in_cb = seen.clone();
        mark_session_started(&move |connected: bool| {
            assert!(connected, "running 回调必须传 true");
            seen_in_cb.store(crate::atvv_endpoints_ready(), Ordering::SeqCst);
            assert!(crate::connection_active(), "回调时连接状态应已置位");
            assert!(
                crate::bridge_session_active(),
                "回调时语音桥会话状态应已置位"
            );
        });
        assert!(
            seen.load(Ordering::SeqCst),
            "on_status(true) 回调发生时端点就绪标志必须已为 true（否则 UI 会显示矛盾状态）"
        );
        // 收尾：避免污染同进程内其他测试。
        crate::set_connection_active(false);
        crate::set_bridge_session_active(false);
    }
}
