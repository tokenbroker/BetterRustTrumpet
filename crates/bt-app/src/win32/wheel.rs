//! Mouse-wheel events for "scroll over the tray icon to change volume".
//! The tray icon itself receives no wheel messages, so a low-level mouse
//! hook forwards wheel deltas. Windows calls such a hook for every mouse
//! event system-wide, so it is installed only while the pointer is over the
//! icon and removed when it leaves; the callback does no work beyond
//! forwarding.
use std::sync::{Mutex, OnceLock};
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE,
        PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        WH_MOUSE_LL, WM_MOUSEWHEEL, WM_QUIT,
    },
};

type Handler = Box<dyn Fn(i16) + Send + Sync>;
static HANDLER: OnceLock<Handler> = OnceLock::new();
/// Thread id of the running hook thread, if installed.
static HOOK_THREAD: Mutex<Option<u32>> = Mutex::new(None);

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

/// Sets the wheel handler (once per process). `handler` receives raw wheel
/// deltas (120 per notch, positive = up) while the hook is installed.
pub fn init(handler: impl Fn(i16) + Send + Sync + 'static) {
    let _ = HANDLER.set(Box::new(handler));
}

/// Installs the hook on its own message-loop thread; no-op if installed.
pub fn install() {
    let Ok(mut current) = HOOK_THREAD.lock() else {
        return;
    };
    if current.is_some() {
        return;
    }
    let (started, thread_id) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("bt-wheel-hook".into())
        .spawn(move || {
            // SAFETY: a low-level hook needs no module handle; this thread
            // pumps messages until WM_QUIT, as WH_MOUSE_LL requires, and
            // removes the hook before exiting.
            unsafe {
                let mut message = MSG::default();
                // Create the message queue before publishing the thread id
                // so a quick uninstall's WM_QUIT is not lost.
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook), None, 0);
                let _ = started.send(hook.is_ok().then(|| GetCurrentThreadId()));
                let Ok(hook) = hook else {
                    return;
                };
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                let _ = UnhookWindowsHookEx(hook);
            }
        });
    if spawned.is_ok() {
        *current = thread_id.recv().ok().flatten();
    }
}

/// Removes the hook if installed.
pub fn uninstall() {
    let Some(thread) = HOOK_THREAD.lock().ok().and_then(|mut t| t.take()) else {
        return;
    };
    // SAFETY: posts WM_QUIT to the hook thread's queue (created by its
    // SetWindowsHookExW/GetMessageW calls); harmless if it already exited.
    unsafe {
        let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
    }
}
