//! A process's main window title (`Process.MainWindowTitle`), which the
//! original's legacy media-player fallback shows as the track title.
use super::foreground::process_path;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GW_OWNER, GetWindow, GetWindowTextLengthW, GetWindowTextW,
            GetWindowThreadProcessId, IsWindowVisible,
        },
    },
    core::BOOL,
};

struct Search<'a> {
    /// Lower-case executable file stem ("vlc").
    stem: &'a str,
    found: Option<String>,
}

fn stem_of(path: &str) -> String {
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the `Search` passed by `title`, alive for the whole
    // EnumWindows call, and only touched on this thread.
    let search = unsafe { &mut *(data.0 as *mut Search) };
    // SAFETY: plain queries on a window handle EnumWindows just gave us;
    // the text buffer is sized from GetWindowTextLengthW.
    unsafe {
        // The main window: visible, top-level, unowned, with a title.
        if !IsWindowVisible(hwnd).as_bool()
            || GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid())
        {
            return true.into();
        }
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return true.into();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if !process_path(pid).is_some_and(|path| stem_of(&path) == search.stem) {
            return true.into();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        let text = String::from_utf16_lossy(&buffer[..copied.max(0) as usize]);
        if text.trim().is_empty() {
            return true.into();
        }
        search.found = Some(text);
    }
    false.into()
}

/// The title of the first visible, unowned top-level window of a process
/// whose executable is `executable` (a path or file name).
pub fn title(executable: &str) -> Option<String> {
    let stem = stem_of(executable);
    let mut search = Search {
        stem: &stem,
        found: None,
    };
    // SAFETY: the callback only reads windows and writes `search`, which
    // outlives the call; EnumWindows "fails" when the callback stops it.
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&raw mut search as isize));
    }
    search.found
}
