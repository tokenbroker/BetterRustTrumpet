//! The Windows user's display language, which .NET uses as
//! `CultureInfo.CurrentUICulture`.
use windows::Win32::Globalization::{
    GetUserDefaultUILanguage, GetUserPreferredUILanguages, LCIDToLocaleName, MUI_LANGUAGE_NAME,
};

/// The first of the user's preferred UI languages ("de-DE"), as .NET's
/// GetUserDefaultUICulture picks it, else the default UI language's name.
pub fn ui_language() -> Option<String> {
    let (mut count, mut len) = (0u32, 0u32);
    // SAFETY: a size query with no buffer; both out-pointers are ours.
    let sized =
        unsafe { GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len) };
    if sized.is_ok() && len > 0 {
        let mut buffer = vec![0u16; len as usize];
        // SAFETY: `buffer` holds the `len` characters just asked for.
        let filled = unsafe {
            GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut count,
                Some(windows::core::PWSTR(buffer.as_mut_ptr())),
                &mut len,
            )
        };
        if filled.is_ok() {
            // A double-NUL-terminated list; the first entry is the language.
            let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            if end > 0 {
                return Some(String::from_utf16_lossy(&buffer[..end]));
            }
        }
    }
    // SAFETY: no arguments.
    let lang = unsafe { GetUserDefaultUILanguage() };
    let mut name = [0u16; 85];
    // SAFETY: `name` is LOCALE_NAME_MAX_LENGTH characters.
    let n = unsafe { LCIDToLocaleName(u32::from(lang), Some(&mut name), 0) };
    (n > 1).then(|| String::from_utf16_lossy(&name[..n as usize - 1]))
}
