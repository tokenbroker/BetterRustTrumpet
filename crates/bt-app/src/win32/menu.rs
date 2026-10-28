//! Menu items, and native popup menus: the fallback for the app's own
//! drawn menus (`ui::menu`). `popup` runs a modal loop, so call it from a
//! task (`wnd::post_task`), never from inside an event handler.
use super::wnd::Window;
use windows::{
    Win32::{
        Foundation::HWND,
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
        UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MF_CHECKED, MF_GRAYED, MF_POPUP,
            MF_SEPARATOR, MF_STRING, PostMessageW, SetForegroundWindow, TPM_BOTTOMALIGN,
            TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TPM_TOPALIGN,
            TrackPopupMenuEx, WM_NULL,
        },
    },
    core::{BOOL, HSTRING, PCSTR, w},
};

pub enum Item {
    /// Text, command id (non-zero), checked, enabled.
    Command(String, u32, bool, bool),
    /// Text, command id and a vector icon: SVG path data (any viewBox; it
    /// is fitted by its bounds) and its scale (the original's IconScale).
    Icon(String, u32, &'static str, f32),
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
    pub fn icon(text: impl Into<String>, id: u32, path: &'static str, scale: f32) -> Self {
        Self::Icon(text.into(), id, path, scale)
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
                Item::Icon(text, id, ..) => {
                    AppendMenuW(menu, MF_STRING, *id as usize, &HSTRING::from(text.as_str()))
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

/// Draws this process's menus dark or light. Win32 has no documented
/// switch, so this uses the uxtheme exports File Explorer and many apps
/// use (Windows 10 1903+): SetPreferredAppMode (ordinal 135, ForceDark or
/// ForceLight), AllowDarkModeForWindow (133) on the owner and
/// FlushMenuThemes (136). Missing exports leave the menus as they are.
fn set_theme(owner: HWND, dark: bool) {
    type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
    type AllowDarkModeForWindow = unsafe extern "system" fn(HWND, BOOL) -> BOOL;
    type FlushMenuThemes = unsafe extern "system" fn();
    const FORCE_DARK: i32 = 2;
    const FORCE_LIGHT: i32 = 3;
    // SAFETY: uxtheme is a system DLL that stays loaded once loaded; the
    // exports are looked up by ordinal and called with the signatures they
    // have had since Windows 10 1903 (on 1809 ordinal 135 takes a BOOL,
    // which any non-zero mode satisfies). `owner` is our window.
    unsafe {
        let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) else {
            return;
        };
        let ordinal = |n: usize| GetProcAddress(uxtheme, PCSTR(std::ptr::without_provenance(n)));
        if let Some(f) = ordinal(135) {
            let f: SetPreferredAppMode = std::mem::transmute(f);
            f(if dark { FORCE_DARK } else { FORCE_LIGHT });
        }
        if let Some(f) = ordinal(133) {
            let f: AllowDarkModeForWindow = std::mem::transmute(f);
            let _ = f(owner, BOOL::from(dark));
        }
        if let Some(f) = ordinal(136) {
            let f: FlushMenuThemes = std::mem::transmute(f);
            f();
        }
    }
}

/// Shows a menu at screen point (`x`, `y`) owned by `owner` and returns the
/// chosen command id. `above` opens it upwards (for the taskbar); `dark`
/// draws it in the dark theme.
pub fn popup(
    owner: Window,
    items: &[Item],
    x: i32,
    y: i32,
    above: bool,
    dark: bool,
) -> Option<u32> {
    let menu = build(items)?;
    let hwnd = HWND(owner.raw() as *mut _);
    set_theme(hwnd, dark);
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
