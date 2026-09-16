//! 用 UI Automation 把键盘焦点送进前台窗口的输入框。
//!
//! # 为什么需要它
//!
//! `SetForegroundWindow` 只能把**顶层窗口**抬到最前，它动不了别的进程内部的
//! 键盘焦点；Win32 的 `SetFocus` 也只能作用于调用线程自己的消息队列。所以
//! 「切到某个应用」和「光标落进它的输入框」是两件事，后者必须走 UIA——
//! 对顶层窗口调 UIA 的 `SetFocus` 会直接抛「目标元素无法接受焦点」（实测）。
//!
//! # 适用面
//!
//! 只对**向 UIA 暴露控件树**的应用有效。Chromium / Electron 系（ZCode、
//! Claude、Chrome 里的网页应用）在检测到 UIA 客户端后会自动开启无障碍，
//! 输入框会以 `Edit` 出现。游戏、自绘界面这类不暴露的，本函数返回
//! [`FocusOutcome::NoInputFound`]，静默跳过即可，不要当成错误。

use crate::error::Result;

/// 聚焦输入框的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusOutcome {
    /// 已把焦点交给某个输入框，附带它的名称（通常是占位符文本）。
    Focused { name: String },
    /// 前台窗口里没有找到可聚焦的输入框。
    NoInputFound,
}

/// 把键盘焦点送进**当前前台窗口**的输入框。
///
/// 会挑第一个既可见、又启用、且可键盘聚焦的输入控件：
/// 优先 `Edit`（标准输入框），没有再退而找 `Document`（有些编辑器把输入区
/// 暴露成文档）。
///
/// **会阻塞**：UIA 是跨进程调用，目标应用忙的时候要等。所以只允许在
/// 调度器的工作线程上调，不要放进 Tauri 主线程。
#[cfg(target_os = "windows")]
pub fn focus_foreground_input() -> Result<FocusOutcome> {
    imp::focus_foreground_input()
}

#[cfg(not(target_os = "windows"))]
pub fn focus_foreground_input() -> Result<FocusOutcome> {
    Err(crate::error::InputError::Windows(
        "focus_foreground_input only on Windows".to_string(),
    ))
}

#[cfg(target_os = "windows")]
mod imp {
    use super::FocusOutcome;
    use crate::error::{InputError, Result};

    use windows::core::Interface;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomation2, IUIAutomationElement, TreeScope_Descendants,
        UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    /// UIA 跨进程调用的等待上限（毫秒）。目标应用卡住时，宁可放弃这一次，
    /// 也不要让调度线程被无限期拖住。
    const UIA_TIMEOUT_MS: u32 = 2000;

    pub fn focus_foreground_input() -> Result<FocusOutcome> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return Ok(FocusOutcome::NoInputFound);
        }

        // 本函数跑在调度器的工作线程上，不是主线程，COM 要自己初始化。
        // 返回 RPC_E_CHANGED_MODE 说明该线程已用别的套间模型初始化过，
        // 照样能用，只是这里不该配对调用 CoUninitialize。
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let owns_com = hr.is_ok();

        let result = focus_in(hwnd);

        if owns_com {
            unsafe { CoUninitialize() };
        }
        result
    }

    fn focus_in(hwnd: HWND) -> Result<FocusOutcome> {
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| InputError::Windows(format!("创建 UI Automation 失败：{e}")))?;

        // 设了上限，目标应用无响应时 UIA 调用会超时返回而不是一直等。
        if let Ok(older) = automation.cast::<IUIAutomation2>() {
            unsafe {
                let _ = older.SetConnectionTimeout(UIA_TIMEOUT_MS);
                let _ = older.SetTransactionTimeout(UIA_TIMEOUT_MS);
            }
        }

        let root = unsafe { automation.ElementFromHandle(hwnd) }
            .map_err(|e| InputError::Windows(format!("读取前台窗口的自动化树失败：{e}")))?;

        // 用「全真条件 + 自己筛」而不是属性条件：属性条件要构造 VARIANT，
        // 而这里需要的判断（控件类型 / 可聚焦 / 启用）本来也要逐个读，
        // 少一层类型转换更不容易出错。
        let all = unsafe {
            root.FindAll(
                TreeScope_Descendants,
                &automation
                    .CreateTrueCondition()
                    .map_err(|e| InputError::Windows(format!("构造查询条件失败：{e}")))?,
            )
        }
        .map_err(|e| InputError::Windows(format!("遍历自动化树失败：{e}")))?;

        let count = unsafe { all.Length() }
            .map_err(|e| InputError::Windows(format!("读取控件数量失败：{e}")))?;

        let mut document_fallback: Option<IUIAutomationElement> = None;
        for index in 0..count {
            let Ok(element) = (unsafe { all.GetElement(index) }) else {
                continue;
            };
            if !is_focusable_input(&element) {
                continue;
            }

            let Ok(kind) = (unsafe { element.CurrentControlType() }) else {
                continue;
            };

            if kind == UIA_EditControlTypeId {
                return apply_focus(&element);
            }
            // 有些编辑器（含部分编辑器组件的网页应用）把输入区暴露成
            // Document。先记下，等整棵树扫完确实没有 Edit 再用它。
            if kind == UIA_DocumentControlTypeId && document_fallback.is_none() {
                document_fallback = Some(element);
            }
        }

        match document_fallback {
            Some(element) => apply_focus(&element),
            None => Ok(FocusOutcome::NoInputFound),
        }
    }

    /// 该控件是不是「可用的输入框」：启用 + 可键盘聚焦 + 有实际尺寸。
    fn is_focusable_input(element: &IUIAutomationElement) -> bool {
        let enabled = unsafe { element.CurrentIsEnabled() }.is_ok_and(|v| v.as_bool());
        if !enabled {
            return false;
        }
        let focusable = unsafe { element.CurrentIsKeyboardFocusable() }.is_ok_and(|v| v.as_bool());
        if !focusable {
            return false;
        }
        // 隐藏控件（0 尺寸）也报告 focusable，跳过它们。
        let rect = unsafe { element.CurrentBoundingRectangle() };
        rect.is_ok_and(|r| r.right > r.left && r.bottom > r.top)
    }

    fn apply_focus(element: &IUIAutomationElement) -> Result<FocusOutcome> {
        let name = unsafe { element.CurrentName() }
            .map(|b| b.to_string())
            .unwrap_or_default();

        unsafe { element.SetFocus() }
            .map_err(|e| InputError::Windows(format!("设置输入框焦点失败：{e}")))?;

        Ok(FocusOutcome::Focused { name })
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    /// 真机冒烟：把焦点送进当前前台窗口的输入框。
    ///
    /// 结果强依赖当时桌面上开着什么，所以默认不跑。需要时先把目标应用切到
    /// 前台，再执行 `cargo test -p core-input -- --ignored --nocapture`。
    #[test]
    #[ignore = "需要桌面上有一个带输入框的前台窗口"]
    fn focus_foreground_input_smoke() {
        let outcome = focus_foreground_input();
        eprintln!("前台窗口的输入框：{outcome:?}");
        assert!(outcome.is_ok(), "UIA 调用本身不应失败：{outcome:?}");
    }
}
