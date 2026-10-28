//! Tray icon images: HICONs made from BGRA pixels drawn by the app, the
//! legacy SndVolSSO.dll icons Windows' own volume icon uses (UseLegacyIcon)
//! and the small-icon size for a DPI.
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HANDLE},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
            DeleteObject, HGDIOBJ,
        },
        System::{
            LibraryLoader::{
                LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE, LoadLibraryExW,
            },
            Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
        },
        UI::{
            HiDpi::GetSystemMetricsForDpi,
            WindowsAndMessaging::{
                CreateIconIndirect, DestroyIcon, HICON, ICONINFO, IMAGE_ICON, LR_DEFAULTCOLOR,
                LoadImageW, SM_CXSMICON,
            },
        },
    },
    core::{HSTRING, PCWSTR, w},
};

/// An icon handle owned by the app, destroyed on drop.
pub struct Icon(HICON);

impl Icon {
    pub fn handle(&self) -> HICON {
        self.0
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: the handle was created by CreateIconIndirect and is
        // destroyed exactly once.
        unsafe {
            let _ = DestroyIcon(self.0);
        }
    }
}

/// SM_CXSMICON for `dpi` (the notification-area icon size).
pub fn small_icon_size(dpi: u32) -> u32 {
    // SAFETY: a plain metrics query.
    let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) };
    u32::try_from(size).ok().filter(|&s| s > 0).unwrap_or(16)
}

/// A `size` x `size` icon from straight-alpha BGRA pixels (top-down).
pub fn from_bgra(size: u32, pixels: &[u8]) -> Option<Icon> {
    let length = (size * size * 4) as usize;
    if pixels.len() != length {
        return None;
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: the DIB's bits are `length` bytes, written once while the
    // bitmap is alive; the colour and mask bitmaps are copied into the icon
    // by CreateIconIndirect and deleted afterwards.
    unsafe {
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, length);
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: true.into(),
            hbmMask: mask,
            hbmColor: color,
            ..Default::default()
        });
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        icon.ok().map(Icon)
    }
}

/// The SndVolSSO.dll icons (SndVolSSO.IconId).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SndVol {
    Muted = 120,
    SpeakerZeroBars = 121,
    SpeakerOneBar = 122,
    SpeakerTwoBars = 123,
    SpeakerThreeBars = 124,
    NoDevice = 125,
}

/// Straight-alpha BGRA pixels of a SndVolSSO.dll icon at `size`, loaded as
/// a resource-only module (IconHelper.LoadIconResource).
pub fn sndvol_bgra(id: SndVol, size: u32) -> Option<Vec<u8>> {
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let path = HSTRING::from(format!(r"{system}\System32\SndVolSSO.dll"));
    // SAFETY: the module is mapped as data only, used for one icon lookup
    // and freed; the icon is copied to pixels and destroyed.
    unsafe {
        let module = LoadLibraryExW(
            &path,
            None,
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
        .ok()?;
        let icon = LoadImageW(
            Some(module.into()),
            // MAKEINTRESOURCE(id).
            PCWSTR(std::ptr::without_provenance(id as usize)),
            IMAGE_ICON,
            size as i32,
            size as i32,
            LR_DEFAULTCOLOR,
        )
        .map(|h: HANDLE| HICON(h.0));
        let _ = FreeLibrary(module);
        let icon = icon.ok()?;
        let pixels = super::icons::render(icon, size, 0);
        let _ = DestroyIcon(icon);
        pixels
    }
}

/// Windows 11 (build 22000 or later), from CurrentBuildNumber; the
/// original's volume-bar thresholds differ between 10 and 11.
pub fn is_windows_11() -> bool {
    let mut buffer = [0u16; 16];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: the buffer and its byte size are valid for the call.
    let ok = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .is_ok();
    let text = String::from_utf16_lossy(&buffer);
    ok && text
        .trim_end_matches('\0')
        .parse::<u32>()
        .is_ok_and(|build| build >= 22000)
}
