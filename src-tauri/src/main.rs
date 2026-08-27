// 防止 Windows 发布版出现额外的控制台窗口，请勿删除！！
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    remote_mic_lib::run()
}
