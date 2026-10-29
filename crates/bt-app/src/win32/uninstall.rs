//! The setup's entry in Settings > Apps (D57): after a self-update (D56)
//! its version is brought up to date, for the copy that setup installed.
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, w};

/// packaging/installer.iss's AppId, as Inno Setup names its key.
const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{4A0AB8A6-D8B7-4365-AADB-82565A5263F8}_is1";

fn read(name: windows::core::PCWSTR) -> Option<String> {
    let key = HSTRING::from(KEY);
    let mut size = 0u32;
    // SAFETY: a size query, then a read into a buffer of that size.
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..end]))
    }
}

/// Sets the entry's DisplayVersion when setup installed the app in `dir`.
pub fn record_version(dir: &std::path::Path, version: &str) {
    let Some(location) = read(w!("InstallLocation")) else {
        return;
    };
    let normalize = |p: &str| p.trim_end_matches('\\').to_lowercase();
    if normalize(&location) != normalize(&dir.to_string_lossy()) {
        return;
    }
    let value: Vec<u16> = version.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `value` is NUL-terminated and its byte length includes the
    // terminator, as REG_SZ requires.
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(KEY),
            w!("DisplayVersion"),
            REG_SZ.0,
            Some(value.as_ptr().cast()),
            (value.len() * 2) as u32,
        );
    }
}
