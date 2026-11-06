//! 会话线程的 COM 套间与消息泵支撑。
//!
//! # 为什么需要这个模块
//!
//! WinRT 的 GATT 事件（`GattCharacteristic::ValueChanged`）是按 **COM 套间**
//! 派发的：
//!
//! - 线程是 **MTA** 时，事件由蓝牙栈的线程池直接回调进来，不需要消息循环；
//! - 线程是 **STA** 时，事件被投递到该线程的套间队列，**必须泵消息**才会派发。
//!
//! v0.7.6 及之前，语音桥会话线程（`commands::voice_bridge::start_voice_bridge`
//! 里的 `std::thread::spawn`）一开始没有任何 COM 初始化，直到 `open_sink()`
//! 打开 CABLE 输出时才由 cpal 的 WASAPI 后端以
//! `CoInitializeEx(None, COINIT_APARTMENTTHREADED)` 隐式初始化成 **STA**；
//! 线程随后阻塞在输出泵的 `recv_timeout` 循环里、从不泵消息，于是整场会话的
//! ATVV 通知（CAPS / 音频 / 控制）全部堆在套间队列中，直到会话结束、线程再次
//! 发起 COM 调用（`AtvvLink::drop` 里的 `ConnectionStatus()`）时才被一次性冲出。
//!
//! 真机日志（2026-11-06 23:52~00:05，连续 16 场会话）的表现是：
//! `控制包=0, 音频包=0`，CAPS 稳定出现在 `AtvvLink Drop` **之后 1~3ms**，
//! 音频在拆链时成批灌入（录制文件里出现 2259 帧/分钟的「爆发」）。
//!
//! # 对策
//!
//! - [`ensure_com_mta`]：在**任何** WinRT / GATT 调用之前把会话线程显式初始化成
//!   MTA，让事件走线程池直接回调，彻底摆脱「必须泵消息」的前提；
//! - [`pump_thread_messages`]：兜底。万一线程已被第三方库初始化成 STA
//!   （`RPC_E_CHANGED_MODE`），输出泵也会定期把套间队列里的消息派发出去。

/// 把当前线程显式初始化为 MTA，返回可直接写进日志的结论。
///
/// 每个线程只初始化一次，重复调用返回同一结论。必须在任何 WinRT / GATT 调用
/// **之前**调用（一旦线程被别的库初始化成 STA，本函数只能报告、无法纠正）。
///
/// 返回值：
/// - `MTA`：本次初始化成功（期望状态）；
/// - `STA（已由其它库初始化）`：`CoInitializeEx` 返回 `RPC_E_CHANGED_MODE`，
///   此时只能依赖 [`pump_thread_messages`] 兜底派发；
/// - `COM 初始化失败`：其它 HRESULT 失败（同时打一条 WARN 带 HRESULT）。
#[cfg(target_os = "windows")]
pub fn ensure_com_mta() -> &'static str {
    use std::cell::Cell;

    thread_local! {
        /// 本线程的 COM 初始化结论（只初始化/探测一次）。
        static LABEL: Cell<Option<&'static str>> = const { Cell::new(None) };
    }

    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    LABEL.with(|slot| {
        if let Some(label) = slot.get() {
            return label;
        }
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let label = if hr.is_ok() {
            "MTA"
        } else if hr == RPC_E_CHANGED_MODE {
            "STA（已由其它库初始化）"
        } else {
            core_log::log_warn(&format!(
                "[com] 会话线程 CoInitializeEx(COINIT_MULTITHREADED) 失败: 0x{:08X}",
                hr.0
            ));
            "COM 初始化失败"
        };
        slot.set(Some(label));
        label
    })
}

/// 非 Windows 占位实现，保证跨平台编译。
#[cfg(not(target_os = "windows"))]
pub fn ensure_com_mta() -> &'static str {
    "非 Windows，跳过 COM 初始化"
}

/// 兜底消息泵：把当前线程套间队列里的 Windows 消息派发掉，返回处理条数。
///
/// 正常路径（线程为 MTA）不需要它；只有线程被第三方库初始化成 STA、事件被
/// 投递进套间队列时才用得上。每次最多处理 [`MAX_MESSAGES_PER_CALL`] 条，
/// 避免长时间占用调用方（输出泵）线程。
#[cfg(target_os = "windows")]
pub fn pump_thread_messages() -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };

    let mut handled = 0u32;
    let mut msg = MSG::default();
    unsafe {
        while handled < MAX_MESSAGES_PER_CALL
            && PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool()
        {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
            handled += 1;
        }
    }
    handled
}

/// 单次 [`pump_thread_messages`] 最多派发的消息条数。
#[cfg(target_os = "windows")]
pub const MAX_MESSAGES_PER_CALL: u32 = 64;

/// 非 Windows 占位实现。
#[cfg(not(target_os = "windows"))]
pub fn pump_thread_messages() -> u32 {
    0
}
