//! The wall clock and local time zone the original's settings code reads
//! (`DateTime.UtcNow`, `DateTime.Today`, `TimeZoneInfo.Local`), as .NET ticks.
use bt_core::newtonsoft::LocalZone;
use windows::Win32::{
    Foundation::{FILETIME, SYSTEMTIME},
    System::Time::{
        FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTimeEx,
        TzSpecificLocalTimeToSystemTimeEx,
    },
};

/// Ticks between 0001-01-01 and the FILETIME epoch (1601-01-01).
const FILETIME_EPOCH: i64 = 504_911_232_000_000_000;
/// Ticks between 0001-01-01 and the Unix epoch.
const UNIX_EPOCH: i64 = 621_355_968_000_000_000;
const TICKS_PER_MINUTE: i64 = 600_000_000;
const TICKS_PER_DAY: i64 = 864_000_000_000;

fn to_system(ticks: i64) -> Option<SYSTEMTIME> {
    let file = u64::try_from(ticks.checked_sub(FILETIME_EPOCH)?).ok()?;
    let file = FILETIME {
        dwLowDateTime: file as u32,
        dwHighDateTime: (file >> 32) as u32,
    };
    let mut system = SYSTEMTIME::default();
    // SAFETY: both pointers are to initialized stack values for the call.
    unsafe { FileTimeToSystemTime(&file, &mut system) }.ok()?;
    Some(system)
}

fn to_ticks(system: &SYSTEMTIME) -> Option<i64> {
    let mut file = FILETIME::default();
    // SAFETY: both pointers are to initialized stack values for the call.
    unsafe { SystemTimeToFileTime(system, &mut file) }.ok()?;
    let file = (u64::from(file.dwHighDateTime) << 32) | u64::from(file.dwLowDateTime);
    i64::try_from(file).ok()?.checked_add(FILETIME_EPOCH)
}

/// The current user's time zone with its historic DST rules.
pub struct WindowsZone;

impl LocalZone for WindowsZone {
    fn offset_for_utc(&self, utc_ticks: i64) -> i32 {
        let local = to_system(utc_ticks).and_then(|utc| {
            let mut local = SYSTEMTIME::default();
            // SAFETY: `None` selects the current time zone; the pointers are
            // to initialized stack values for the call.
            unsafe { SystemTimeToTzSpecificLocalTimeEx(None, &utc, &mut local) }.ok()?;
            to_ticks(&local)
        });
        local.map_or(0, |l| ((l - utc_ticks) / TICKS_PER_MINUTE) as i32)
    }
    fn offset_for_local(&self, local_ticks: i64) -> i32 {
        let utc = to_system(local_ticks).and_then(|local| {
            let mut utc = SYSTEMTIME::default();
            // SAFETY: as above, converting the other way.
            unsafe { TzSpecificLocalTimeToSystemTimeEx(None, &local, &mut utc) }.ok()?;
            to_ticks(&utc)
        });
        utc.map_or(0, |u| ((local_ticks - u) / TICKS_PER_MINUTE) as i32)
    }
}

/// `DateTime.UtcNow` in ticks.
pub fn utc_now_ticks() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    UNIX_EPOCH + (since.as_nanos() / 100) as i64
}

/// `DateTime.Today` (local midnight) in ticks.
pub fn today_ticks() -> i64 {
    let utc = utc_now_ticks();
    let local = utc + i64::from(WindowsZone.offset_for_utc(utc)) * TICKS_PER_MINUTE;
    local - local.rem_euclid(TICKS_PER_DAY)
}

/// Milliseconds on the local wall clock, for undo coalescing (`DateTime.Now`).
pub fn local_now_ms() -> i64 {
    let utc = utc_now_ticks();
    (utc + i64::from(WindowsZone.offset_for_utc(utc)) * TICKS_PER_MINUTE) / 10_000
}
