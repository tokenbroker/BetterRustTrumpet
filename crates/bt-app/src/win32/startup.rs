//! "Start with Windows" via HKCU\...\Run, like the original's RunAtStartup.
//! Uses its own value name so the C# app's `BetterTrumpet` entry is never
//! changed while both builds are installed.
use std::io;
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR},
        System::Registry::{
            HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
            RegSetKeyValueW,
        },
    },
    core::{PCWSTR, w},
};

const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const VALUE: PCWSTR = w!("BetterTrumpet Rust Preview");

fn check(status: WIN32_ERROR) -> io::Result<()> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.0 as i32))
    }
}

pub fn is_enabled() -> bool {
    let mut size = 0u32;
    // SAFETY: constant NUL-terminated names; a null data pointer with a size
    // out-parameter only queries the value's existence and length.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    status.is_ok()
}

/// Enables startup for `executable`, or removes this build's entry.
pub fn set_enabled(enabled: bool, executable: &std::path::Path) -> io::Result<()> {
    if !enabled {
        // SAFETY: constant NUL-terminated key and value names.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE) };
        return if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            check(status)
        };
    }
    let command: Vec<u16> = format!("\"{}\"", executable.display())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: `command` is NUL-terminated and its byte length includes the
    // terminator, as REG_SZ requires; the buffer outlives the call.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE,
            REG_SZ.0,
            Some(command.as_ptr().cast()),
            (command.len() * 2) as u32,
        )
    };
    check(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trips this build's own Run entry and restores the prior state.
    #[test]
    fn startup_entry_round_trips_without_touching_other_entries() {
        let before = is_enabled();
        let exe = std::env::current_exe().unwrap();
        set_enabled(true, &exe).unwrap();
        assert!(is_enabled());
        set_enabled(false, &exe).unwrap();
        assert!(!is_enabled());
        // Removing an absent entry is not an error.
        set_enabled(false, &exe).unwrap();
        if before {
            set_enabled(true, &exe).unwrap();
        }
    }
}
