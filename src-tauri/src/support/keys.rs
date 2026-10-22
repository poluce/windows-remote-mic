//! 按键兜底释放与短窗口事件去重。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::Manager;

use crate::AppState;

/// 兜底释放：先让调度器补发「按住未松开」的快捷键松开事件，
/// 再清空虚拟 HID 里所有残留的按住状态。
///
/// 用于任何「松开事件再也到不了」的路径：语音桥会话结束（BLE 断链）、应用退出。
/// 虚拟 HID（WinUHid）的按住状态只活在进程内，一旦没人松开就会一直按在系统上，
/// 用户的**物理键盘**会跟着变成 Ctrl+ 组合键（实测卡死过 rctrl）。
/// 多松一次只是无害的冗余报告，少松一次就是全局卡键，所以这里宁可过度释放。
pub(crate) fn force_release_held_keys(app: &tauri::AppHandle, reason: &str) -> usize {
    #[cfg(target_os = "windows")]
    core_hid::tap::clear_active_usages();
    let released = match app.try_state::<AppState>() {
        Some(state) => state.dispatcher.release_all_held(reason),
        None => 0,
    };
    let cleared = core_input::release_all_keys();
    if released > 0 || cleared {
        core_log::log_warn(&format!(
            "[app] 兜底释放按键（{reason}）：补发松开 {released} 个，虚拟 HID 残留={cleared}"
        ));
    }
    released + usize::from(cleared)
}

/// 短窗口去重：同一虚拟键的按下/松开若在 40ms 内被钩子与 Raw Input
/// 各投递一次，只向前端发一条，避免测试器矩阵翻倍计数。
pub(crate) struct KeyEmitDedup {
    last: Mutex<Option<(u32, bool, Instant)>>,
}

impl KeyEmitDedup {
    pub(crate) fn new() -> Self {
        Self {
            last: Mutex::new(None),
        }
    }

    pub(crate) fn take(&self, vkey: u32, pressed: bool) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if let Some((vk, p, t)) = *last {
            if vk == vkey && p == pressed && now.duration_since(t) < Duration::from_millis(40) {
                return false;
            }
        }
        *last = Some((vkey, pressed, now));
        true
    }
}
