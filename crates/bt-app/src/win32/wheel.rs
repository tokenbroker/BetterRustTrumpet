//! Mouse-wheel events for "scroll over the tray icon to change volume".
//! The tray icon itself receives no wheel messages, so a low-level mouse
//! hook forwards wheel deltas; the app acts only while the pointer is over
//! the icon. The hook callback does no work beyond forwarding.
use std::sync::OnceLock;
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, MSG, MSLLHOOKSTRUCT,
        SetWindowsHookExW, TranslateMessage, WH_MOUSE_LL, WM_MOUSEWHEEL,
    },
};

type Handler = Box<dyn Fn(i16) + Send + Sync>;
static HANDLER: OnceLock<Handler> = OnceLock::new();

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam.0 as u32 == WM_MOUSEWHEEL {
        // SAFETY: for WH_MOUSE_LL with HC_ACTION, lparam points at a valid
        // MSLLHOOKSTRUCT for the duration of the call.
        let data = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let delta = (data.mouseData >> 16) as u16 as i16;
        if let Some(handler) = HANDLER.get() {
            handler(delta);
        }
    }
    // SAFETY: always pass the event on; the wheel is never swallowed.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Installs the hook on a dedicated message-loop thread (once per process).
/// `handler` receives raw wheel deltas (120 per notch, positive = up).
pub fn start(handler: impl Fn(i16) + Send + Sync + 'static) {
    if HANDLER.set(Box::new(handler)).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("bt-wheel-hook".into())
        .spawn(|| {
            // SAFETY: a low-level hook needs no module handle; the thread
            // pumps messages for its whole life, as WH_MOUSE_LL requires.
            unsafe {
                let Ok(_hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(hook), None, 0) else {
                    return;
                };
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        })
        .ok();
}
