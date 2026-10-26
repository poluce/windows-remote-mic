//! Remote Mic Tauri 应用外壳。
//! 菜单键 → 快捷菜单：调度器经统一应用事件出口（quick_menu::handle_app_event）处理。

mod commands;

mod services;

mod support;

pub(crate) use support::codec::{
    action_key, action_label, button_key, config_store, load_app_profiles, parse_action,
    parse_button, parse_trigger, trigger_key, MappingEdit, MappingEntry,
};
pub(crate) use support::keys::{force_release_held_keys, KeyEmitDedup};
pub(crate) use support::panic::install_panic_hook;
pub(crate) use support::paths::find_install_script;
pub(crate) use support::state::AppState;

use std::sync::Arc;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use core_atvv::ImaAdpcmDecoder;
use core_dispatch::{AppEvent, KeyDispatcher};

/// 用于验证前端 <-> 后端桥接是否正常工作的简单命令。
#[tauri::command]
fn ping() -> String {
    let mut decoder = ImaAdpcmDecoder::new();
    let _ = decoder.decode_bytes(&[0x00, 0x11]);
    "后端已连接（ATVV 解码正常，ADPCM 就绪）".to_string()
}

/// 当前运行版本号。
///
/// 与 updater 插件做版本比较时用的是同一个来源（`tauri.conf.json` 的 version），
/// 前端「关于与更新」不必再自己拼一份。
#[tauri::command]
fn app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 安装更新前的收尾：updater 拉起 NSIS 安装器后会直接 `std::process::exit(0)`，
/// 不会走 `RunEvent::Exit` 里那条兜底路径，所以提前释放按键，避免虚拟 HID
/// 上残留「按住」状态（重启后的新实例不会知道旧实例按下过哪些键）。
#[tauri::command]
fn prepare_for_update_exit(app: tauri::AppHandle) {
    core_log::log_info("[app] 准备退出并安装更新，强制释放按键");
    force_release_held_keys(&app, "安装更新");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "windows")]
    if core_hid::tap::maybe_run_injector() {
        return;
    }

    install_panic_hook();

    // 单实例保护：两个实例会各自开 GATT 会话、挂键盘钩子、注入 Frida，
    // 互相争用同一条 BLE 链路（实测会出现双桥/双注入的错乱）。
    #[cfg(target_os = "windows")]
    if !core_input::acquire_single_instance() {
        core_log::log_warn("[app] 检测到已有实例在运行，本次启动直接退出");
        core_input::notify_already_running();
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let win_width = 420.0;
            let win_height = 420.0;

            let right_margin = 2.0;
            let bottom_margin = 2.0;
            let (x, y) = if let Ok(Some(monitor)) = app.handle().primary_monitor() {
                let scale = monitor.scale_factor();
                // 使用工作区（不含任务栏）定位，并留出很小的边距
                let work = monitor.work_area();
                let wx = work.position.x as f64 / scale;
                let wy = work.position.y as f64 / scale;
                let ww = work.size.width as f64 / scale;
                let wh = work.size.height as f64 / scale;
                let x = wx + ww - win_width - right_margin;
                let y = wy + wh - win_height - bottom_margin;
                (x, y)
            } else {
                (0.0, 0.0)
            };

            WebviewWindowBuilder::new(app, "quick-menu", WebviewUrl::App("quick-menu.html".into()))
                .title("Quick Menu")
                .inner_size(win_width, win_height)
                .position(x, y)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .visible(false)
                .build()?;

            #[cfg(windows)]
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.with_webview(|webview| unsafe {
                    use webview2_com::Microsoft::Web::WebView2::Win32::{
                        ICoreWebView2Settings3, ICoreWebView2Settings6,
                    };
                    use windows_core::Interface;
                    match webview.controller().CoreWebView2() {
                        Ok(core) => {
                            if let Ok(settings) = core.Settings() {
                                if let Ok(s3) = settings.cast::<ICoreWebView2Settings3>() {
                                    let _ = s3.SetAreBrowserAcceleratorKeysEnabled(false);
                                }
                                if let Ok(s6) = settings.cast::<ICoreWebView2Settings6>() {
                                    let _ = s6.SetIsSwipeNavigationEnabled(false);
                                }
                                core_log::log_info(
                                    "[app] WebView2 accelerator keys and swipe navigation disabled",
                                );
                            }
                        }
                        Err(e) => {
                            core_log::log_warn(&format!(
                                "[app] 访问 WebView2 以禁用加速键失败: {e}"
                            ));
                        }
                    }
                });
            }

            #[cfg(target_os = "windows")]
            let dispatcher = {
                let mut cfg = config_store()
                    .map(|s| s.load_or_default())
                    .unwrap_or_default();
                // 旧版本把麦克风映射为 SingleClick；迁移为按下/松开 PTT。
                if cfg.mapping.migrate_mic_ptt() {
                    if let Some(store) = config_store() {
                        let _ = store.save(&cfg);
                    }
                    core_log::log_info("[app] 麦克风映射已迁移为按下/松开（PTT）");
                }
                // 旧版本把菜单键映射为右键菜单；迁移为打开/关闭快捷菜单。
                if cfg.mapping.migrate_menu_quickmenu() {
                    if let Some(store) = config_store() {
                        let _ = store.save(&cfg);
                    }
                    core_log::log_info("[app] 菜单键映射已迁移为快捷菜单（开/关）");
                }
                // 旧版本的「禁用」动作没有可恢复的动作信息；迁移为未绑定。
                if cfg.mapping.migrate_disabled_bindings() {
                    if let Some(store) = config_store() {
                        let _ = store.save(&cfg);
                    }
                    core_log::log_info("[app] 旧的「禁用」动作已迁移为未绑定");
                }
                // 把「吃掉」模式写入热更新文件：注入时据此生成 config，
                // 运行中 Frida 脚本每秒轮询该文件，设置页切换即时生效。
                core_hid::tap::write_eat_mode_file(cfg.hid_tap_eat);
                let dispatcher = KeyDispatcher::new(cfg.mapping, &cfg.key_calibrations);
                dispatcher.set_trigger_timing(cfg.long_press_ms, cfg.double_click_ms);
                // 语音识别目标随配置恢复（旧配置缺省为 Windows 语音）。
                dispatcher.set_voice_target(cfg.voice_target);
                // 应用专属配置：命中前台进程时覆盖对应按键，未覆盖的沿用全局映射。
                dispatcher.set_profiles(load_app_profiles());
                // 应用事件出口：开关快捷菜单、菜单独占模式的按键直转，
                // 统一交给 commands::quick_menu::handle_app_event 处理。
                let app_for_events = app.handle().clone();
                dispatcher.set_app_event_handler(Some(Arc::new(move |event: AppEvent| {
                    commands::quick_menu::handle_app_event(app_for_events.clone(), event);
                })));
                let stats_dir = std::env::var("LOCALAPPDATA")
                    .map(|base| std::path::PathBuf::from(base).join("RemoteMic/RC003"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("."));
                dispatcher.spawn_runtime(stats_dir);
                app.manage(AppState {
                    dispatcher: dispatcher.clone(),
                });
                dispatcher
            };

            // ---- 主线程看门狗（临时诊断，定位 AppHang）----
            //
            // 应用被 Windows 判成「无响应」时，从外面只能看到进程还活着、窗口
            // 不再泵消息，看不出卡在哪一步。这个线程每 2 秒请主线程打个卡，
            // 打不上就记一条 WARN——把「什么时候开始卡的」钉在日志里，
            // 和上一条业务日志一对，就知道当时正卡在哪个操作上。
            //
            // 定位到问题后应当删掉这段。
            {
                let beat_handle = app.handle().clone();
                let spawned = std::thread::Builder::new()
                    .name("main-watchdog".into())
                    .spawn(move || {
                        use std::sync::mpsc;
                        use std::time::{Duration, Instant};

                        let start = Instant::now();
                        let mut stalled_since: Option<u128> = None;
                        let mut last_notice = 0u128;
                        // 主线程卡死时按键松开路径可能永远不会执行：
                        // 卡住超过 15s 就补一次强制释放（只补一次，避免刷屏）。
                        let mut released_for_stall = false;

                        loop {
                            std::thread::sleep(Duration::from_secs(2));

                            let (tx, rx) = mpsc::channel();
                            let posted = beat_handle
                                .run_on_main_thread(move || {
                                    let _ = tx.send(());
                                })
                                .is_ok();
                            let alive = posted && rx.recv_timeout(Duration::from_secs(3)).is_ok();

                            let now = start.elapsed().as_millis();
                            if alive {
                                released_for_stall = false;
                                if let Some(since) = stalled_since.take() {
                                    core_log::log_warn(&format!(
                                        "[watchdog] 主线程恢复响应（曾卡住约 {} 秒）",
                                        (now - since) / 1000
                                    ));
                                }
                                continue;
                            }

                            match stalled_since {
                                None => {
                                    stalled_since = Some(now);
                                    last_notice = now;
                                    core_log::log_warn(
                                        "[watchdog] 主线程已超过 3 秒未响应，开始记录",
                                    );
                                }
                                Some(since) if !released_for_stall && now - since >= 15_000 => {
                                    released_for_stall = true;
                                    core_log::log_warn(
                                        "[watchdog] 主线程卡住超过 15s，补发一次强制释放按键（防卡键）",
                                    );
                                    std::thread::spawn(|| {
                                        core_hid::tap::clear_active_usages();
                                        core_input::release_all_keys();
                                    });
                                }
                                Some(_) if now - last_notice >= 10_000 => {
                                    last_notice = now;
                                    core_log::log_warn("[watchdog] 主线程仍无响应");
                                }
                                Some(_) => {}
                            }
                        }
                    });
                if let Err(e) = spawned {
                    core_log::log_warn(&format!("[watchdog] 启动失败: {e}"));
                }
            }

            // 钩子与 Raw Input 可能对同一物理按键各投递一次；短窗口去重后
            // 只向前端发一条。调度器只吃 Raw Input（能辨设备），避免注入
            // 回声与双路重复进入触发状态机。
            let emit_dedup = Arc::new(KeyEmitDedup::new());

            let handle = app.handle().clone();
            let dedup_hook = emit_dedup.clone();
            if let Err(e) = core_input::start_key_hook(move |evt| {
                if !dedup_hook.take(evt.vkey, evt.pressed) {
                    return;
                }
                let _ = handle.emit("raw-remote-key", evt);
            }) {
                core_log::log_error(&format!("[hook] 启动键盘钩子失败: {e}"));
            }

            #[cfg(target_os = "windows")]
            {
                let handle_raw = app.handle().clone();
                let dispatcher_raw = dispatcher.clone();
                let dedup_raw = emit_dedup.clone();
                if let Err(e) = core_hid::raw_input::start_listener(move |evt| {
                    dispatcher_raw.on_vkey(evt.vkey, evt.pressed);
                    if !dedup_raw.take(evt.vkey as u32, evt.pressed) {
                        return;
                    }
                    let _ = handle_raw.emit(
                        "raw-remote-key",
                        core_input::RawKeyEvent {
                            vkey: evt.vkey as u32,
                            pressed: evt.pressed,
                        },
                    );
                }) {
                    core_log::log_error(&format!("[raw_input] 启动原始输入监听失败: {e}"));
                }

                let handle_tap = app.handle().clone();
                core_hid::tap::set_status_callback(move |evt| {
                    let _ = handle_tap.emit("hid-tap-status", evt);
                });
            }

            // 日志实时推送：文件写入与 UI 推送互不干扰，持续推送新写入的日志行。
            let log_handle = app.handle().clone();
            std::thread::spawn(move || {
                let rx = core_log::subscribe_log_lines();
                while let Ok(line) = rx.recv() {
                    let _ = log_handle.emit("log-line", line);
                }
            });

            core_log::log_info("[app] Remote Mic 应用初始化完成，窗口与全局钩子已就绪");
            Ok(())
        })
        .on_window_event(|window, event| {
            // 主窗口是应用唯一的入口：关闭它就应该退出应用。
            //
            // 应用启动时还会创建一个**隐藏**的 `quick-menu` 窗口（见 setup）。
            // 如果只关闭主窗口，Tauri 认为还有窗口没关，事件循环不会退出，
            // 进程就驻留在后台：没有托盘图标，任务栏也看不到入口，用户
            // 只能去任务管理器结束进程（实测踩过）。这里在主窗口关闭请求
            // 时显式退出；退出路径仍会走 `RunEvent::Exit`，由那里兜底
            // 释放虚拟 HID 上可能残留的按住状态。
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    core_log::log_info("[app] 主窗口关闭，退出应用");
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            app_version,
            prepare_for_update_exit,
            commands::update::get_last_update_check_at,
            commands::update::set_last_update_check_at,
            commands::connection::device::scan_for_rc003,
            commands::connection::device::connect_rc003,
            commands::connection::status::get_runtime_status,
            commands::connection::device::get_saved_device_id,
            commands::connection::device::save_selected_device,
            commands::connection::prefs::get_hid_tap_eat,
            commands::connection::prefs::set_hid_tap_eat,
            commands::connection::system::open_system_settings,
            commands::connection::prefs::get_voice_target,
            commands::connection::prefs::set_voice_target,
            commands::connection::system::restart_app,
            commands::connection::system::install_vhid_driver,
            commands::log::log_message,
            commands::log::get_log_info,
            commands::log::read_log_tail,
            commands::log::clear_log,
            commands::log::open_log_dir,
            commands::log::set_debug_logging,
            commands::mapping::save_mapping,
            commands::mapping::get_mappings,
            commands::mapping::clear_mapping_binding,
            commands::mapping::set_mapping_binding_enabled,
            commands::mapping_catalog::get_mapping_catalog,
            commands::mapping::save_key_calibrations,
            commands::mapping::get_key_calibrations,
            commands::mapping::set_dispatch_enabled,
            commands::mapping::get_trigger_timing,
            commands::mapping::set_trigger_timing,
            commands::mapping::get_shortcuts,
            commands::mapping::save_shortcut,
            commands::mapping::delete_shortcut,
            commands::voice_bridge::start_voice_bridge,
            commands::voice_bridge::stop_voice_bridge,
            commands::voice_bridge::retry_voice_bridge,
            commands::audio_tools::simulate_voice_chain,
            commands::vb_cable::vb_cable_status,
            commands::vb_cable::install_vb_cable,
            commands::audio_tools::audio_diagnostics,
            commands::audio_tools::play_test_tone_loop,
            commands::audio_tools::trigger_voice_typing,
            commands::diagnostics::run_self_test,
            commands::app_profile::menu::app_profile_status,
            commands::app_profile::editor::app_profile_catalog,
            commands::app_profile::menu::app_menu_apps,
            commands::app_profile::menu::open_app_profile,
            commands::app_profile::menu::reload_app_profiles,
            commands::app_profile::editor::create_app_profile,
            commands::app_profile::editor::save_profile_binding,
            commands::app_profile::editor::clear_profile_binding,
            commands::app_profile::editor::delete_app_profile,
            commands::quick_menu::close_quick_menu,
            commands::quick_menu::toggle_quick_menu,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 退出是最后一个还能执行代码的时机：此后没人能补发松开事件，
            // 虚拟 HID 上残留的按住状态会一直卡在系统里（物理键盘跟着失灵）。
            if let tauri::RunEvent::Exit = event {
                force_release_held_keys(app, "应用退出");
            }
        });
}
