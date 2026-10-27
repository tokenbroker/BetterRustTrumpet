//! System colours the original flyout resolves from Windows: the accent
//! colour (its `SystemAccent`) and whether transparency effects are on.
use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    core::w,
};

/// Windows accent colour as `#rrggbb`, or Windows' default blue.
pub fn accent() -> String {
    UISettings::new()
        .and_then(|settings| settings.GetColorValue(UIColorType::Accent))
        .map(|c| format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B))
        .unwrap_or_else(|_| "#0078d7".into())
}

/// "Transparency effects" in Settings > Personalization > Colors (on when
/// the value is missing, as Windows treats it).
pub fn transparency() -> bool {
    let mut value = 1u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reads a DWORD into `value`, whose size is passed in `size`; the
    // key and value names are static NUL-terminated strings.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("EnableTransparency"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    status.is_err() || value != 0
}
