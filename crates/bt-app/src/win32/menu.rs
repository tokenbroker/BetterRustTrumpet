//! Native popup menus. `popup` runs a modal loop, so call it from a task
//! (`wnd::post_task`), never from inside an event handler.
use super::wnd::Window;
use windows::{
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP,
            MF_SEPARATOR, MF_STRING, PostMessageW, SetForegroundWindow, TPM_BOTTOMALIGN,
            TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TPM_TOPALIGN,
            TrackPopupMenuEx, WM_NULL,
        },
    },
    core::HSTRING,
};

pub enum Item {
    /// Text, command id (non-zero), checked, enabled.
    Command(String, u32, bool, bool),
    Submenu(String, Vec<Item>),
    Separator,
}

impl Item {
    pub fn command(text: impl Into<String>, id: u32) -> Self {
        Self::Command(text.into(), id, false, true)
    }
    pub fn check(text: impl Into<String>, id: u32, checked: bool) -> Self {
        Self::Command(text.into(), id, checked, true)
    }
    pub fn disabled(text: impl Into<String>) -> Self {
        Self::Command(text.into(), 0, false, false)
    }
}

fn build(items: &[Item]) -> Option<HMENU> {
    // SAFETY: builds a menu tree; submenus are owned by their parent and
    // destroyed with it.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for item in items {
            let _ = match item {
                Item::Command(text, id, checked, enabled) => {
                    let mut flags = MF_STRING;
                    if *checked {
                        flags |= MF_CHECKED;
                    }
                    if !enabled {
                        flags |= MF_GRAYED;
                    }
                    AppendMenuW(menu, flags, *id as usize, &HSTRING::from(text.as_str()))
                }
                Item::Submenu(text, children) => match build(children) {
                    Some(sub) => AppendMenuW(
                        menu,
                        MF_STRING | MF_POPUP,
                        sub.0 as usize,
                        &HSTRING::from(text.as_str()),
                    ),
                    None => Ok(()),
                },
                Item::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, None),
            };
        }
        Some(menu)
    }
}

/// Shows a menu at screen point (`x`, `y`) owned by `owner` and returns the
/// chosen command id. `above` opens it upwards (for the taskbar).
pub fn popup(owner: Window, items: &[Item], x: i32, y: i32, above: bool) -> Option<u32> {
    let menu = build(items)?;
    let hwnd = HWND(owner.raw() as *mut _);
    let vertical = if above { TPM_BOTTOMALIGN } else { TPM_TOPALIGN };
    // SAFETY: the documented pattern: the owner becomes foreground so the
    // menu closes on outside clicks, and WM_NULL is posted after; the menu
    // is destroyed once.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_LEFTALIGN | vertical).0,
            x,
            y,
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(menu);
        (chosen.0 > 0).then_some(chosen.0 as u32)
    }
}
