//! Single instance: a named mutex; a second launch asks the running copy
//! to open its flyout and exits. The name differs from the C# app's, so the
//! two can run side by side.
use super::wnd::{Poster, Window};
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
        System::Threading::CreateMutexW,
        UI::WindowsAndMessaging::FindWindowW,
    },
    core::{HSTRING, w},
};

const MUTEX: &str = "Local\\BetterTrumpet-Rust-5f0c2a4e";

/// Holds the mutex for the life of the process.
pub struct Instance(#[allow(dead_code)] HANDLE);

/// Takes the instance mutex, or `None` if another copy holds it.
pub fn acquire() -> Option<Instance> {
    // SAFETY: creates (or opens) a named mutex; the handle is kept open for
    // the life of the process.
    unsafe {
        let handle = CreateMutexW(None, true, &HSTRING::from(MUTEX)).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return None;
        }
        Some(Instance(handle))
    }
}

/// The running copy's hidden window, by its title.
pub fn find(title: &str) -> Option<Poster> {
    // SAFETY: plain lookup by class and title.
    let hwnd = unsafe { FindWindowW(w!("BetterTrumpetRust.Window"), &HSTRING::from(title)).ok()? };
    Some(Window::from_raw(hwnd.0 as isize).poster())
}
