//! System colours the original flyout resolves from Windows (its
//! `SystemSettings` and `SystemAccent*` brushes).
use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    core::{HSTRING, PCWSTR},
};

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
    let mut data = default;
    let mut size = std::mem::size_of::<u32>() as u32;
    let key = HSTRING::from("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
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
