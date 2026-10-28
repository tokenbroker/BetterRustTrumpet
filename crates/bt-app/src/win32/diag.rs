//! What a diagnostic report records about the process and the system
//! (SnapshotData), the clipboard copy of its path, and opening it in
//! Explorer.
use std::ffi::c_void;
use windows::{
    Win32::{
        Foundation::{GlobalFree, HANDLE},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
            Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
            Threading::{
                GR_GDIOBJECTS, GR_USEROBJECTS, GetCurrentProcess, GetGuiResources,
                GetProcessHandleCount,
            },
        },
        UI::HiDpi::GetDpiForSystem,
    },
    core::w,
};

/// CF_UNICODETEXT.
const CF_UNICODETEXT: u32 = 13;

/// GDI and USER objects of this process, then of the whole session
/// (gdiObjects, userObjects, globalGdiObjects, globalUserObjects).
pub fn gui_resources() -> [u32; 4] {
    // GR_GLOBAL: the (HANDLE)-2 pseudo handle for the whole session.
    let global = HANDLE(-2isize as *mut c_void);
    // SAFETY: plain queries on the current-process pseudo handle and the
    // documented GR_GLOBAL pseudo handle.
    unsafe {
        let process = GetCurrentProcess();
        [
            GetGuiResources(process, GR_GDIOBJECTS),
            GetGuiResources(process, GR_USEROBJECTS),
            GetGuiResources(global, GR_GDIOBJECTS),
            GetGuiResources(global, GR_USEROBJECTS),
        ]
    }
}

/// The process's open handle count.
pub fn handle_count() -> u32 {
    let mut count = 0u32;
    // SAFETY: the pseudo handle is always valid; `count` outlives the call.
    let _ = unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) };
    count
}

/// GetDpiForSystem.
pub fn system_dpi() -> u32 {
    // SAFETY: a plain query.
    unsafe { GetDpiForSystem() }
}

/// SystemSettings.BuildLabel: HKLM's BuildLabEx.
pub fn build_label() -> String {
    let mut buffer = [0u16; 256];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: the buffer and its byte size are valid for the call.
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"Software\Microsoft\Windows NT\CurrentVersion"),
            w!("BuildLabEx"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .is_ok();
    if !ok {
        return "No BuildLabEx set".into();
    }
    let text = String::from_utf16_lossy(&buffer[..(size as usize / 2).min(buffer.len())]);
    text.trim_end_matches('\0').to_owned()
}

/// Puts `text` on the clipboard (ErrorReporter.CopyPathToClipboard).
pub fn copy_text(text: &str) -> bool {
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let bytes = wide.len() * 2;
    // SAFETY: the clipboard is opened and closed here; the global block is
    // allocated with the text's size, filled while locked, and handed to
    // the clipboard (which then owns it) or freed if that fails.
    unsafe {
        if OpenClipboard(None).is_err() {
            return false;
        }
        let mut done = false;
        if EmptyClipboard().is_ok() {
            if let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
                let target = GlobalLock(memory) as *mut u16;
                if !target.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
                    let _ = GlobalUnlock(memory);
                    done = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0))).is_ok();
                }
                if !done {
                    let _ = GlobalFree(Some(memory));
                }
            }
        }
        let _ = CloseClipboard();
        done
    }
}

/// LocalDataExporter.ShowInExplorer: a folder opens, a file is selected.
pub fn show_in_explorer(path: &std::path::Path) {
    let mut command = std::process::Command::new("explorer.exe");
    if path.is_dir() {
        command.arg(path);
    } else {
        use std::os::windows::process::CommandExt;
        command.raw_arg(format!("/select,\"{}\"", path.display()));
    }
    let _ = command.spawn();
}
