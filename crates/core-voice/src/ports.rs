//! 语音桥端口（Ports）：把「传输 / 音频输出 / 按键注入」抽象成 trait。
//!
//! 目的：真实桥接（`bridge::run_bridge`）与模拟链路（`simulate::simulate_voice_chain`）
//! 依赖**同一组接口**，而不是各自直接调用 `core-ble` / `core-audio` / `core-hid` /
//! `core-input` 的具体实现。
//!
//! - 接口定义在这里（与平台无关，仅用 std）；
//! - 真实实现见 [`crate::transport`]（ATVV GATT）、[`crate::sink`]（CABLE 输出）、
//!   [`crate::keys`]（虚拟 HID / Win+H）；
//! - 以后要加 RC001、测试用假实现或其他音频后端，只需要再提供一个实现。

/// 语音传输：ATVV GATT 会话（真实遥控器）或其替代实现。
///
/// 方法返回 `Result<_, String>`，避免把 `core-ble` 的错误类型泄漏到编排层。
pub trait VoiceTransport {
    /// 注册链路连接状态回调；链路连上/断开时以 `true`/`false` 调用。
    fn register_connection_handler(
        &self,
        callback: Box<dyn FnMut(bool) + Send + 'static>,
    ) -> Result<i64, String>;
    /// 启用音频特征通知（CCCD Notify）。
    fn enable_audio_notifications(&self) -> Result<(), String>;
    /// 启用控制特征通知（CCCD Notify）。
    fn enable_control_notifications(&self) -> Result<(), String>;
    /// 向 TX 特征写入命令字节（主机 -> 遥控器）。
    fn write_tx(&self, bytes: &[u8]) -> Result<(), String>;
    /// 注册音频通知回调（原始 ATVV 音频帧）。
    fn register_audio_handler(
        &self,
        callback: Box<dyn FnMut(Vec<u8>) + Send + 'static>,
    ) -> Result<i64, String>;
    /// 注册控制通知回调（原始 ATVV 控制帧）。
    fn register_control_handler(
        &self,
        callback: Box<dyn FnMut(Vec<u8>) + Send + 'static>,
    ) -> Result<i64, String>;
    /// 当前链路是否仍处于已连接状态。
    fn is_connected(&self) -> bool;
}

/// 音频输出：把 48kHz 立体声 f32 帧送到目标播放设备（通常是 `CABLE Input`）。
pub trait AudioSink {
    /// 推入一帧输出样本；实现内部负责格式适配与队列。
    fn push(&self, frames: &[f32]);
    /// 输出流是否已出错（设备被禁用 / 拔出等）。
    fn is_failed(&self) -> bool;
    /// 最近一次出错原因。
    fn last_error(&self) -> Option<String>;
    /// 可读的设备格式描述（日志用），例如 `48000Hz/2ch/F32`。
    fn format_label(&self) -> String;
}

/// 按键注入：会话期间对虚拟 HID / Win+H 的副作用。
///
/// 要求 `Send + Sync`：真实实现的回调可能在其它线程触发，方便用 `Arc<dyn KeySink>`
/// 在多个回调之间共享。
pub trait KeySink: Send + Sync {
    /// ATVV 通知已生效后启动可选的 Back/Volume 轻触注入。
    fn start_after_atvv(&self);
    /// 清空驱动层「吃掉模式」的活跃按键（断链瞬间防修饰键残留）。
    fn clear_active_usages(&self);
    /// 释放本应用注入的所有按键。
    fn release_all_keys(&self);
    /// 遥控器 `AudioStopped` 时兜底释放麦克风按键。
    fn release_mic_on_audio_stop(&self);
    /// 触发 Windows 语音输入（Win+H）。
    fn press_win_h(&self) -> Result<(), String>;
    /// 按 Esc 关闭/复位语音输入条。
    fn press_escape(&self) -> Result<(), String>;
}
