//! WinRT 异步操作等待工具：**带超时**。
//!
//! `pollster::block_on(op.await)` 在设备/驱动/蓝牙栈卡死时会永久阻塞工作线程，
//! 导致语音桥无法退出、`stop_voice_bridge` 停不下来。这里改为轮询
//! `IAsyncOperation::Status()`，超过期限就请求取消并返回错误，保证调用方
//! 永远能在有限时间内拿到结果。

use std::time::{Duration, Instant};

use windows::core::RuntimeType;
use windows_future::{AsyncStatus, IAsyncOperation};

use crate::{BleError, Result};

/// 轮询间隔。WinRT 异步完成回调在其它线程，轮询只为等待状态位。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 等待一个 WinRT 异步操作完成；超时会 `Cancel()` 并返回错误。
pub(crate) fn wait_op<T>(op: &IAsyncOperation<T>, timeout: Duration, what: &str) -> Result<T>
where
    T: RuntimeType + 'static,
{
    let started = Instant::now();
    loop {
        let status = op
            .Status()
            .map_err(|e| BleError::Windows(format!("{what} 读取状态失败: {e}")))?;

        if status == AsyncStatus::Started {
            if started.elapsed() >= timeout {
                let _ = op.Cancel();
                return Err(BleError::Windows(format!(
                    "{what} 超时（{}s），已请求取消",
                    timeout.as_secs()
                )));
            }
            std::thread::sleep(POLL_INTERVAL);
            continue;
        }

        if status == AsyncStatus::Canceled {
            return Err(BleError::Windows(format!("{what} 被取消")));
        }

        // Completed / Error：统一交给 GetResults 判定，错误信息更完整。
        return op
            .GetResults()
            .map_err(|e| BleError::Windows(format!("{what} 失败: {e}")));
    }
}
