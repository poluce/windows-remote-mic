//! 仅 Windows 的非注入辅助函数。
//!
//! 按键注入已全部改走虚拟 HID 键盘（见 [`crate::vhid`]）；
//! 本模块不再包含任何 `SendInput` 代码，避免出现绕过虚拟 HID 的注入路径。

use crate::Result;

/// 通过 shell 打开应用（`cmd /c start "" <name>`）。
pub fn open_app(name: &str) -> Result<()> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", name])
        .spawn()
        .map_err(|e| crate::error::InputError::Windows(e.to_string()))?;
    Ok(())
}
