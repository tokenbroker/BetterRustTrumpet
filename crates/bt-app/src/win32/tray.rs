//! The notification-area icon (Shell_NotifyIcon, NOTIFYICON_VERSION_4).
use std::rc::Rc;

use super::{
    trayicon::Icon,
    wnd::{WM_TRAY, Window},
};
use windows::{
    Win32::{
        Foundation::{HANDLE, HWND},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::GetSystemMetricsForDpi,
            Shell::{
                NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO,
                NIIF_RESPECT_QUIET_TIME, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
                NOTIFYICON_VERSION_4, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER,
                Shell_NotifyIconGetRect, Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                DestroyIcon, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXSMICON,
                SM_CYSMICON,
            },
        },
    },
    core::PCWSTR,
};

const ID: u32 = 1;

pub struct Tray {
    window: Window,
    icon: HICON,
    /// The drawn icon that replaces the application icon once set.
    drawn: Option<Rc<Icon>>,
    tip: String,
}

fn copy_tip(text: &str, out: &mut [u16]) {
    let wide: Vec<u16> = text.encode_utf16().take(out.len() - 1).collect();
    out.fill(0);
    out[..wide.len()].copy_from_slice(&wide);
}

/// The application icon (resource 1) at the small-icon size for `dpi`.
fn load_icon(dpi: u32) -> HICON {
    // SAFETY: loads icon resource 1 from our own module; the handle is
    // destroyed in Drop.
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return HICON::default();
        };
        let cx = GetSystemMetricsForDpi(SM_CXSMICON, dpi);
        let cy = GetSystemMetricsForDpi(SM_CYSMICON, dpi);
        LoadImageW(
            Some(module.into()),
            // MAKEINTRESOURCE(1).
            PCWSTR(std::ptr::without_provenance(1)),
            IMAGE_ICON,
            cx,
            cy,
            LR_DEFAULTCOLOR,
        )
        .map(|h: HANDLE| HICON(h.0))
        .unwrap_or_default()
    }
}

impl Tray {
    pub fn new(window: Window, tip: &str) -> Self {
        let dpi = (super::wnd::monitor_at(0, 0).scale * 96.0) as u32;
        let tray = Self {
            window,
            icon: load_icon(dpi),
            drawn: None,
            tip: tip.into(),
        };
        tray.add();
        tray
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: HWND(self.window.raw() as *mut _),
            uID: ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.drawn.as_ref().map_or(self.icon, |icon| icon.handle()),
            ..Default::default()
        };
        copy_tip(&self.tip, &mut data.szTip);
        data
    }

    /// Adds the icon (again, after the taskbar restarts).
    pub fn add(&self) {
        let mut data = self.data();
        // SAFETY: `data` is a valid NOTIFYICONDATAW for our window.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }

    /// The native tooltip text (empty hides it).
    pub fn set_tip(&mut self, tip: &str) {
        if self.tip == tip {
            return;
        }
        self.tip = tip.into();
        let data = self.data();
        // SAFETY: as above.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    /// Replaces the icon (the previous drawn one is destroyed after the
    /// shell has the new one).
    pub fn set_icon(&mut self, icon: Rc<Icon>) {
        let _previous = self.drawn.replace(icon);
        let mut data = self.data();
        // Only the image changed (the animation sends one per pose).
        data.uFlags = NIF_ICON;
        // SAFETY: as above; the new icon outlives the call.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    /// A notification from the icon (title and text), shown by the shell.
    pub fn notify(&self, title: &str, text: &str) {
        let mut data = self.data();
        data.uFlags |= NIF_INFO;
        data.dwInfoFlags = NIIF_INFO | NIIF_RESPECT_QUIET_TIME;
        copy_tip(title, &mut data.szInfoTitle);
        copy_tip(text, &mut data.szInfo);
        // SAFETY: as above.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    /// The icon's screen rectangle (x, y, width, height), physical pixels.
    pub fn rect(&self) -> Option<(i32, i32, i32, i32)> {
        let id = NOTIFYICONIDENTIFIER {
            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: HWND(self.window.raw() as *mut _),
            uID: ID,
            ..Default::default()
        };
        // SAFETY: `id` identifies our icon.
        let r = unsafe { Shell_NotifyIconGetRect(&id) }.ok()?;
        Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let data = self.data();
        // SAFETY: removes our icon and destroys the icon handle we loaded.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            if !self.icon.is_invalid() {
                let _ = DestroyIcon(self.icon);
            }
        }
    }
}
