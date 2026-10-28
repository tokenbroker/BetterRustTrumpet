//! The foreground window's process, for the focus-lost volume feature. The
//! original polls GetForegroundWindow every 250 ms; this port listens for
//! EVENT_SYSTEM_FOREGROUND instead. The hook is out of context, so Windows
//! delivers it through the installing thread's message loop (the UI thread)
//! and nothing runs inside other processes. It is installed only while the
//! feature needs it; the callback only forwards.
use std::sync::{Mutex, OnceLock};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HWND},
        System::Threading::{
            OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::{
            Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
            WindowsAndMessaging::{
                EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetWindowThreadProcessId,
                WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
            },
        },
    },
    core::PWSTR,
};

type Handler = Box<dyn Fn() + Send + Sync>;
static HANDLER: OnceLock<Handler> = OnceLock::new();
/// The installed hook handle, if any.
static HOOK: Mutex<Option<isize>> = Mutex::new(None);

unsafe extern "system" fn hook(
    _hook: HWINEVENTHOOK,
    event: u32,
    _hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if event == EVENT_SYSTEM_FOREGROUND {
        if let Some(handler) = HANDLER.get() {
            handler();
        }
    }
}

/// Sets the handler called when the foreground window changes (once per
/// process).
pub fn init(handler: impl Fn() + Send + Sync + 'static) {
    let _ = HANDLER.set(Box::new(handler));
}

/// Installs the hook for the calling thread's message loop; no-op if
/// already installed. Our own windows are skipped (the original ignores its
/// own process).
pub fn install() {
    let Ok(mut current) = HOOK.lock() else {
        return;
    };
    if current.is_some() {
        return;
    }
    // SAFETY: an out-of-context hook needs no module handle; the callback
    // is a plain function that lives for the whole process.
    let handle = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(hook),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    if !handle.is_invalid() {
        *current = Some(handle.0 as isize);
    }
}

/// Removes the hook if installed.
pub fn uninstall() {
    let Some(handle) = HOOK.lock().ok().and_then(|mut h| h.take()) else {
        return;
    };
    // SAFETY: `handle` came from SetWinEventHook and is unhooked only once.
    unsafe {
        let _ = UnhookWinEvent(HWINEVENTHOOK(handle as *mut _));
    }
}

pub fn installed() -> bool {
    HOOK.lock().is_ok_and(|h| h.is_some())
}

/// The foreground window's process id, or 0 when there is none.
pub fn foreground_pid() -> u32 {
    let mut pid = 0u32;
    // SAFETY: both calls accept any window handle, including null; `pid`
    // outlives the call.
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_invalid() {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
    }
    pid
}

/// A process's image path, if it can be queried.
pub fn process_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // SAFETY: limited query access is enough for QueryFullProcessImageNameW;
    // the handle is closed before returning and `buffer`/`size` outlive the
    // call, with `size` holding the buffer's length in characters.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; 1024];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        Some(String::from_utf16_lossy(&buffer[..size as usize]))
    }
}
