//! 音频端点模型。
//!
//! 端点是 Windows 暴露的播放（或采集）设备，例如
//! VB-CABLE 的 `CABLE Input`。整个音频层都通过该模型交互，
//! 因此更换虚拟音频驱动只需改动此模块。

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// 音频端点的方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EndpointKind {
    /// 我们向其中写入遥控器音频的播放设备（例如 CABLE Input）。
    Output,
    /// 被应用视为麦克风的采集设备（例如 CABLE Output）。
    Input,
}

/// 单个音频端点（设备）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioEndpoint {
    /// 用于持久化的稳定设备 ID（绝不是原始硬件 ID）。
    pub id: String,
    /// 设置界面中显示的可读名称。
    pub name: String,
    pub kind: EndpointKind,
}

/// 在接入 WASAPI 枚举之前使用的默认占位端点。
pub fn placeholder_output() -> AudioEndpoint {
    AudioEndpoint {
        id: "cable-input".to_string(),
        name: "CABLE Input (VB-CABLE)".to_string(),
        kind: EndpointKind::Output,
    }
}

/// 列出应用可以写入语音的输出端点。
///
/// Windows：枚举 WASAPI 播放端点，并暴露用户可作为虚拟声卡选择的端点
///（VB-CABLE 的 CABLE Input）。
/// 非 Windows：返回单个占位端点，便于预览 UI。
pub fn list_output_endpoints() -> Result<Vec<AudioEndpoint>> {
    #[cfg(target_os = "windows")]
    {
        crate::wasapi::list_output_endpoints()
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(vec![placeholder_output()])
    }
}

/// 列出采集端点（虚拟麦克风侧，例如 CABLE Output）。
///
/// Windows：枚举 WASAPI 采集端点。
/// 非 Windows：返回空列表，让诊断在预览中保持真实。
pub fn list_input_endpoints() -> Result<Vec<AudioEndpoint>> {
    #[cfg(target_os = "windows")]
    {
        crate::wasapi::list_input_endpoints()
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(Vec::new())
    }
}

/// 返回当前默认采集（麦克风）设备名称（如果可用）。
pub fn default_input_name() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        cpal::default_host()
            .default_input_device()
            .and_then(|device| device.name().ok())
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}
