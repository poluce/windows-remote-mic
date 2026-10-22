//! 连接页相关命令。
//!
//! 按职责分文件：
//! - `status`：运行时状态快照（连接 / 旁路 / 端点 / 语音桥）；
//! - `device`：RC003 扫描/连接 + 上次设备记忆；
//! - `prefs`：用户偏好（吃掉模式 / 语音识别目标）；
//! - `system`：系统集成（打开设置 / 重启 / 重装虚拟 HID 驱动）。

pub mod device;
pub mod prefs;
pub mod status;
pub mod system;
