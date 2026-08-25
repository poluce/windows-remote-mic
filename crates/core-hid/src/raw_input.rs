//! Windows Raw Input capture framework (hidden window + message loop).
//! Only compiled on Windows.

use std::sync::Mutex;
use std::thread;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS,
    RegisterRawInputDevices, RID_INPUT, RIDEV_INPUTSINK, RAWKEYBOARD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
    RegisterClassW, TranslateMessage, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
    CW_USEDEFAULT, MSG, WM_DESTROY, WM_INPUT,
};

static CALLBACK: Mutex<Option<Box<dyn Fn(RawInputEvent) + Send>>> = Mutex::new(None);

/// One raw keyboard event from the remote.
#[derive(Debug, Clone, Copy)]
pub struct RawInputEvent {
    pub vkey: u16,
    pub make_code: u16,
    pub pressed: bool,
}

/// A running Raw Input listener thread (hidden window + message loop).
pub struct RawInputListener {
    _thread: thread::JoinHandle<()>,
}

/// Start listening for keyboard Raw Input. The callback runs on the message
/// loop thread.
pub fn start_listener(
    callback: impl Fn(RawInputEvent) + Send + 'static,
) -> Result<RawInputListener, String> {
    *CALLBACK.lock().unwrap() = Some(Box::new(callback));

    let class_name: Vec<u16> = "RemoteMicRawInput\0".encode_utf16().collect();
    let class_ptr = PCWSTR(class_name.as_ptr());

    let hinstance = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;

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

    let atom = unsafe { RegisterClassW(&window_class) };
    if atom == 0 {
        return Err("RegisterClassW failed".to_string());
    }

    let hwnd = unsafe {
        CreateWindowExW(
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
        )
    }
    .map_err(|e| e.to_string())?;

    let device = RAWINPUTDEVICE {
        usUsagePage: 0x01, // Generic Desktop
        usUsage: 0x06,     // Keyboard
        dwFlags: RAWINPUTDEVICE_FLAGS(RIDEV_INPUTSINK.0),
        hwndTarget: hwnd,
    };

    unsafe {
        RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32)
            .map_err(|e| e.to_string())?;
    }

    let thread = thread::spawn(move || unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });

    Ok(RawInputListener { _thread: thread })
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_INPUT {
            let raw = HRAWINPUT(lparam.0 as *mut core::ffi::c_void);

            let mut size = 0u32;
            GetRawInputData(
                raw,
                RID_INPUT,
                None,
                &mut size,
                std::mem::size_of::<RAWINPUT>() as u32,
            );

            let mut buf = vec![0u8; size as usize];
            if size > 0 {
                GetRawInputData(
                    raw,
                    RID_INPUT,
                    Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                    &mut size,
                    std::mem::size_of::<RAWINPUT>() as u32,
                );

                let input = &*(buf.as_ptr() as *const RAWINPUT);
                let keyboard: RAWKEYBOARD = input.data.keyboard;
                let event = RawInputEvent {
                    vkey: keyboard.VKey,
                    make_code: keyboard.MakeCode,
                    pressed: keyboard.Flags & 0x01 == 0,
                };
                if let Ok(mut cb) = CALLBACK.lock() {
                    if let Some(f) = cb.as_mut() {
                        f(event);
                    }
                }
            }
            return LRESULT(0);
        }

        if msg == WM_DESTROY {
            PostQuitMessage(0);
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
