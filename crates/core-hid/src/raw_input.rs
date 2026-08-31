//! Windows Raw Input 捕获框架（隐藏窗口 + 消息循环）。
//! 仅在 Windows 上编译。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUT,
    RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS, RAWINPUTHEADER, RAWKEYBOARD, RIDEV_INPUTSINK,
    RIDI_DEVICENAME, RID_INPUT, RIM_TYPEHID, RIM_TYPEKEYBOARD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, CW_USEDEFAULT, MSG, WM_DESTROY, WM_INPUT, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::parse_hid_report_vkeys;

type RawInputCallback = Box<dyn Fn(RawInputEvent) + Send>;

static CALLBACK: Mutex<Option<RawInputCallback>> = Mutex::new(None);
static HID_DOWN: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static LOGGED_SIZE_FAIL: AtomicBool = AtomicBool::new(false);
/// 键盘路径最近一次上报某虚拟键的时刻，用于识别 WM_APPCOMMAND 回声。
static LAST_KEYBOARD_EMIT: Mutex<Vec<(u16, Instant)>> = Mutex::new(Vec::new());
/// 超过该间隔仍未从键盘路径见过同一虚拟键时，WM_APPCOMMAND 视为
/// 独立来源（仅发应用命令的设备）予以放行。
const APPCOMMAND_ECHO_WINDOW: Duration = Duration::from_millis(500);

/// 来自遥控器的一个原始键盘事件。
#[derive(Debug, Clone, Copy)]
pub struct RawInputEvent {
    pub vkey: u16,
    pub make_code: u16,
    pub pressed: bool,
}

/// 运行中的 Raw Input 监听线程（隐藏窗口 + 消息循环）。
pub struct RawInputListener {
    _thread: thread::JoinHandle<()>,
}

/// 开始监听键盘 Raw Input。回调在消息循环线程上运行。
pub fn start_listener(
    callback: impl Fn(RawInputEvent) + Send + 'static,
) -> Result<RawInputListener, String> {
    *CALLBACK.lock().unwrap() = Some(Box::new(callback));

    let (tx, rx) = std::sync::mpsc::channel();

    let thread = thread::spawn(move || unsafe {
        let class_name: Vec<u16> = "RemoteMicRawInput\0".encode_utf16().collect();
        let class_ptr = PCWSTR(class_name.as_ptr());

        let hinstance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                let _ = tx.send(Err(e.to_string()));
                return;
            }
        };

        let window_class = WNDCLASSW {
            style: Default::default(),
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance.into(),
            hIcon: Default::default(),
            hCursor: Default::default(),
            hbrBackground: Default::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_ptr,
        };

        let _ = RegisterClassW(&window_class);

        let hwnd = match CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_ptr,
            PCWSTR::null(),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            None,
            None,
            Some(hinstance.into()),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                let _ = tx.send(Err(e.to_string()));
                return;
            }
        };

        let devices = [
            RAWINPUTDEVICE {
                usUsagePage: 0x01, // 通用桌面
                usUsage: 0x06,     // 键盘
                dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_INPUTSINK.0),
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 0x0C, // 消费类音频 / 遥控器
                usUsage: 0x01,     // 消费类控制
                dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_INPUTSINK.0),
                hwndTarget: hwnd,
            },
            RAWINPUTDEVICE {
                usUsagePage: 0x01, // 通用桌面
                usUsage: 0x80,     // 系统控制（电源）
                dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_INPUTSINK.0),
                hwndTarget: hwnd,
            },
        ];

        if let Err(e) =
            RegisterRawInputDevices(&devices, std::mem::size_of::<RAWINPUTDEVICE>() as u32)
        {
            core_log::log_error(&format!("[raw_input] 注册原始输入设备失败: {e}"));
            let _ = tx.send(Err(e.to_string()));
            return;
        }

        core_log::log_info(
            "[raw_input] 原始输入窗口及设备（键盘、消费类、系统）已在线程上初始化成功",
        );
        let _ = tx.send(Ok(()));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });

    rx.recv().map_err(|e| e.to_string())??;
    Ok(RawInputListener { _thread: thread })
}

fn emit_appcommand(cmd: u32) -> bool {
    let vk = match cmd {
        1 => Some(166),  // 浏览器后退
        7 => Some(172),  // 浏览器主页
        8 => Some(173),  // 音量静音
        9 => Some(175),  // 音量加
        10 => Some(174), // 音量减
        _ => None,
    };
    if let Some(vkey) = vk {
        // 键盘 Raw Input 路径通常已经上报过同一物理按键；WM_APPCOMMAND
        // 附带的合成"按下+松开"会把按住时长截断为 0，破坏长按/按住
        // 重复检测，因此近期上报过时直接吞掉。
        let recent = LAST_KEYBOARD_EMIT
            .lock()
            .unwrap()
            .iter()
            .any(|(vk, t)| *vk == vkey && t.elapsed() < APPCOMMAND_ECHO_WINDOW);
        if recent {
            core_log::log_debug(&format!(
                "[raw_input] 抑制 WM_APPCOMMAND 回声: vkey={vkey}（键盘路径已上报）"
            ));
            return true;
        }
        emit(RawInputEvent {
            vkey,
            make_code: 0,
            pressed: true,
        });
        emit(RawInputEvent {
            vkey,
            make_code: 0,
            pressed: false,
        });
        true
    } else {
        false
    }
}

pub(crate) fn emit(event: RawInputEvent) {
    if let Ok(mut cb) = CALLBACK.lock() {
        if let Some(f) = cb.as_mut() {
            f(event);
        }
    }
}

fn is_bluetooth_device(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    u.contains("BTH") || u.contains("BLUETOOTH") || u.contains("1812")
}

/// 字母、数字、空格、Tab、退格——PC 打字键，绝不会是 RC003 按键。
fn is_pc_typing_vkey(vk: u16) -> bool {
    matches!(
        vk,
        0x08 | 0x09 | 0x20 | 0x30..=0x39 | 0x41..=0x5A | 0x60..=0x69 | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2
    )
}

unsafe fn device_name(hdevice: HANDLE) -> String {
    let mut size = 0u32;
    GetRawInputDeviceInfoW(Some(hdevice), RIDI_DEVICENAME, None, &mut size);
    if size == 0 {
        return String::new();
    }
    let mut buf = vec![0u16; size as usize];
    let n = GetRawInputDeviceInfoW(
        Some(hdevice),
        RIDI_DEVICENAME,
        Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
        &mut size,
    );
    if n == u32::MAX {
        return String::new();
    }
    String::from_utf16_lossy(&buf)
        .trim_end_matches('\0')
        .to_string()
}

unsafe fn read_raw_input(lparam: LPARAM) -> Option<Vec<u8>> {
    let raw = HRAWINPUT(lparam.0 as *mut core::ffi::c_void);
    let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
    let mut size = 0u32;
    let probe = GetRawInputData(raw, RID_INPUT, None, &mut size, header_size);
    if probe == u32::MAX || size == 0 {
        if !LOGGED_SIZE_FAIL.swap(true, Ordering::Relaxed) {
            core_log::log_error(&format!(
                "[raw_input] 获取原始输入数据大小失败（ret={probe}, size={size}, header={header_size}）"
            ));
        }
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    let got = GetRawInputData(
        raw,
        RID_INPUT,
        Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
        &mut size,
        header_size,
    );
    if got == u32::MAX || got == 0 {
        if !LOGGED_SIZE_FAIL.swap(true, Ordering::Relaxed) {
            core_log::log_error(&format!(
                "[raw_input] 读取原始输入数据失败（ret={got}, size={size}）"
            ));
        }
        return None;
    }
    buf.truncate(got as usize);
    Some(buf)
}

fn hid_payload(buf: &[u8]) -> &[u8] {
    let header_size = std::mem::size_of::<RAWINPUTHEADER>();
    // RAWHID：dwSizeHid (u32) + dwCount (u32) + bRawData[]
    if buf.len() < header_size + 8 {
        return &[];
    }
    let dw_size_hid =
        u32::from_le_bytes(buf[header_size..header_size + 4].try_into().unwrap()) as usize;
    let dw_count =
        u32::from_le_bytes(buf[header_size + 4..header_size + 8].try_into().unwrap()) as usize;
    let start = header_size + 8;
    let total = dw_size_hid.saturating_mul(dw_count.max(1));
    let end = (start + total).min(buf.len());
    if start >= end {
        &[]
    } else {
        &buf[start..end]
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_INPUT {
            let Some(buf) = read_raw_input(lparam) else {
                return LRESULT(0);
            };
            if buf.len() < std::mem::size_of::<RAWINPUTHEADER>() {
                return LRESULT(0);
            }
            let header = &*(buf.as_ptr() as *const RAWINPUTHEADER);
            let name = device_name(header.hDevice);

            if header.dwType == RIM_TYPEKEYBOARD.0 {
                let input = &*(buf.as_ptr() as *const RAWINPUT);
                let keyboard: RAWKEYBOARD = input.data.keyboard;
                let pressed = keyboard.Flags & 0x01 == 0;

                // 1. 普通 PC 打字键直接过滤，不记录日志也不转发
                if is_pc_typing_vkey(keyboard.VKey) {
                    return LRESULT(0);
                }

                let from_remote = is_bluetooth_device(&name);
                // PC 键盘仍会到达 WebView；只转发蓝牙 HID
                // 以及测试器看不到的未映射 usage（VKey 0 / 255）。
                let unmapped = keyboard.VKey == 0 || keyboard.VKey == 0xFF;
                if from_remote || unmapped {
                    let mut vkey = keyboard.VKey;
                    if vkey == 0 {
                        if let Some(mapped) = crate::usage_to_vkey(u32::from(keyboard.MakeCode)) {
                            vkey = mapped;
                        }
                    }
                    if vkey != 0 && !is_pc_typing_vkey(vkey) {
                        core_log::log_debug(&format!(
                            "[raw_input] 遥控器键盘事件: vkey={}, make_code=0x{:02X}, 按下={}",
                            vkey, keyboard.MakeCode, pressed
                        ));
                        {
                            let mut recent = LAST_KEYBOARD_EMIT.lock().unwrap();
                            recent.retain(|(vk, t)| {
                                vk != &vkey && t.elapsed() < APPCOMMAND_ECHO_WINDOW
                            });
                            recent.push((vkey, Instant::now()));
                        }
                        emit(RawInputEvent {
                            vkey,
                            make_code: keyboard.MakeCode,
                            pressed,
                        });
                    }
                }
            } else if header.dwType == RIM_TYPEHID.0 {
                let from_remote = is_bluetooth_device(&name);
                if from_remote {
                    let raw_slice = hid_payload(&buf);
                    core_log::log_debug(&format!(
                        "[raw_input] 遥控器 HID 数据包: {:02X?} 设备={}",
                        raw_slice, name
                    ));
                    let now = parse_hid_report_vkeys(raw_slice);
                    let mut prev = HID_DOWN.lock().unwrap();
                    for vk in &now {
                        if !prev.contains(vk) {
                            emit(RawInputEvent {
                                vkey: *vk,
                                make_code: 0,
                                pressed: true,
                            });
                        }
                    }
                    for vk in prev.iter() {
                        if !now.contains(vk) {
                            emit(RawInputEvent {
                                vkey: *vk,
                                make_code: 0,
                                pressed: false,
                            });
                        }
                    }
                    *prev = now;
                }
            }

            return LRESULT(0);
        }

        // WM_APPCOMMAND (0x0319)
        if msg == 0x0319 {
            let cmd = ((lparam.0 as u32) >> 16) & 0xFFF;
            core_log::log_info(&format!(
                "[raw_input] 收到应用命令（WM_APPCOMMAND）: cmd={cmd}"
            ));
            if emit_appcommand(cmd) {
                return LRESULT(1);
            }
        }

        if msg == WM_DESTROY {
            PostQuitMessage(0);
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
