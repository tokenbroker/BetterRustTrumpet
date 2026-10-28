//! System colours the original flyout resolves from Windows (its
//! `SystemSettings` and `SystemAccent*` brushes).
use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::{
        Graphics::Gdi::{
            COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT,
            COLOR_MENU, COLOR_MENUTEXT, COLOR_WINDOW, COLOR_WINDOWFRAME, COLOR_WINDOWTEXT,
            GetSysColor, SYS_COLOR_INDEX,
        },
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::{
                SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
            },
        },
    },
    core::{HSTRING, PCWSTR},
};

/// The system colours high-contrast themes define (WPF's SystemColors),
/// as RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemColors {
    pub window: [u8; 3],
    pub window_text: [u8; 3],
    pub highlight: [u8; 3],
    pub highlight_text: [u8; 3],
    /// ControlText (COLOR_BTNTEXT).
    pub control_text: [u8; 3],
    pub gray_text: [u8; 3],
    /// HotTrack (COLOR_HOTLIGHT).
    pub hot_track: [u8; 3],
    pub menu: [u8; 3],
    pub menu_text: [u8; 3],
    pub window_frame: [u8; 3],
}

fn sys_color(index: SYS_COLOR_INDEX) -> [u8; 3] {
    // SAFETY: a plain query; the value is 0x00BBGGRR.
    let c = unsafe { GetSysColor(index) };
    [c as u8, (c >> 8) as u8, (c >> 16) as u8]
}

/// SPI_GETHIGHCONTRAST: whether Windows' high-contrast mode is on
/// (`SystemParameters.HighContrast`).
pub fn high_contrast_on() -> bool {
    let mut info = HIGHCONTRASTW {
        cbSize: size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a HIGHCONTRASTW whose size is passed in uiParam
    // and in cbSize; it outlives the call.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            info.cbSize,
            Some((&raw mut info).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok();
    ok && info.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
}

/// The high-contrast system colours, when high contrast is on.
pub fn high_contrast() -> Option<SystemColors> {
    high_contrast_on().then(|| SystemColors {
        window: sys_color(COLOR_WINDOW),
        window_text: sys_color(COLOR_WINDOWTEXT),
        highlight: sys_color(COLOR_HIGHLIGHT),
        highlight_text: sys_color(COLOR_HIGHLIGHTTEXT),
        control_text: sys_color(COLOR_BTNTEXT),
        gray_text: sys_color(COLOR_GRAYTEXT),
        hot_track: sys_color(COLOR_HOTLIGHT),
        menu: sys_color(COLOR_MENU),
        menu_text: sys_color(COLOR_MENUTEXT),
        window_frame: sys_color(COLOR_WINDOWFRAME),
    })
}

fn color(kind: UIColorType, fallback: &str) -> String {
    UISettings::new()
        .and_then(|settings| settings.GetColorValue(kind))
        .map(|c| format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B))
        .unwrap_or_else(|_| fallback.into())
}

/// Windows accent colour as `#rrggbb`, or Windows' default blue.
pub fn accent() -> String {
    color(UIColorType::Accent, "#0078d7")
}

/// `SystemAccentDark1`, for the accent-coloured flyout and popups.
pub fn accent_dark1() -> String {
    color(UIColorType::AccentDark1, "#005a9e")
}

/// A DWORD under HKCU, as the original's `ReadDword` (`> 0`).
fn personalize(value: &str, default: u32) -> bool {
    dword(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
        value,
        default,
    )
}

fn dword(path: &str, value: &str, default: u32) -> bool {
    let mut data = default;
    let mut size = std::mem::size_of::<u32>() as u32;
    let key = HSTRING::from(path);
    let name = HSTRING::from(value);
    // SAFETY: reads a DWORD into `data`, whose size is passed in `size`; the
    // key and value names are NUL-terminated HSTRINGs that outlive the call.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        data = default;
    }
    data > 0
}

/// "Transparency effects" in Settings > Personalization > Colors (on when
/// the value is missing, as Windows treats it).
pub fn transparency() -> bool {
    personalize("EnableTransparency", 1)
}

/// The taskbar/system theme the flyout follows (`IsSystemLightTheme`).
pub fn system_light() -> bool {
    personalize("SystemUsesLightTheme", 0)
}

/// "Show accent colour on Start and taskbar" (`UseAccentColor`).
pub fn accent_on_taskbar() -> bool {
    personalize("ColorPrevalence", 0)
}

/// The app theme ("Choose your default app mode": `AppsUseLightTheme`),
/// which app windows such as settings follow.
pub fn apps_light() -> bool {
    personalize("AppsUseLightTheme", 1)
}

/// "Show accent colour on title bars and window borders"
/// (`UseAccentColorOnWindowBorders`, DWM ColorPrevalence).
pub fn accent_on_title_bars() -> bool {
    dword("Software\\Microsoft\\Windows\\DWM", "ColorPrevalence", 0)
}
