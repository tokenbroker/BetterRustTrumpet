//! Window geometry Tauri cannot set in one step.
use std::ffi::c_void;
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos},
};

/// Moves and resizes a top-level window at once (physical pixels), so a
/// window growing upwards never shows a frame at the old position.
pub fn set_bounds(hwnd: *mut c_void, x: i32, y: i32, width: i32, height: i32) {
    // SAFETY: `hwnd` is a window owned by this process (from Tauri); a stale
    // handle only makes the call fail.
    unsafe {
        let _ = SetWindowPos(
            HWND(hwnd),
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}
