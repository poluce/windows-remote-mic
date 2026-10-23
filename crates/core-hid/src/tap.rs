//! 可选的 HOGP 旁路，仅用于返回 / 音量加 / 音量减。
//!
//! 在 ATVV 通知就绪后启动。缺少 gadget 或用户拒绝 UAC 时，
//! 语音和 Raw Input 按键仍可正常工作。
//!
//! 子模块职责：
//! - `config`：端口 / 吃掉模式热文件 / Frida gadget 路径 / 注入 PID 记忆；
//! - `hub`：localhost IPC 与 Frida 消息分发；
//! - `inject`：提权注入流程（UAC / 远程线程 LoadLibrary）；
//! - `runtime`：旁路生命周期（启动准备 / 注入器入口 / hub 线程）；
//! - `status`：状态机与前端状态事件；
//! - `watchdog`：按住状态跟踪、看门狗兜底释放。

mod config;
mod hub;
mod inject;
mod runtime;
mod status;
mod watchdog;

pub use config::{gadget_available, write_eat_mode_file};
pub use runtime::{maybe_run_injector, start_after_atvv};
pub use status::{last_status, set_status_callback, TapState, TapStatusEvent};
pub use watchdog::{clear_active_usages, release_mic_on_audio_stop, touch_mic_activity};
