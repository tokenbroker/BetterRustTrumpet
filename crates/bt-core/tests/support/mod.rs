//! Shared test boundaries. `Central` is the capture machine's time zone (US
//! Central, post-2007 rules); store_compatibility.rs checks it against every
//! offset Windows reported during capture.
#![allow(dead_code)]
use bt_core::newtonsoft::LocalZone;

pub const TICKS_PER_DAY: i64 = 864_000_000_000;
pub const TICKS_PER_HOUR: i64 = 36_000_000_000;

pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let leap = |y: i64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let y = year - 1;
    let mut days = y * 365 + y / 4 - y / 100 + y / 400;
    for m in 1..month {
        days += match m {
            2 if leap(year) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
    }
    days + day - 1
}
/// nth Sunday of a month as days since 0001-01-01 (a Monday).
fn sunday(year: i64, month: i64, nth: i64) -> i64 {
    let first = days_from_civil(year, month, 1);
    first + (7 - (first + 1) % 7) % 7 + (nth - 1) * 7
}
fn year_of(ticks: i64) -> i64 {
    let mut year = ticks / (TICKS_PER_DAY * 365) + 1;
    while days_from_civil(year, 1, 1) * TICKS_PER_DAY > ticks {
        year -= 1;
    }
    while days_from_civil(year + 1, 1, 1) * TICKS_PER_DAY <= ticks {
        year += 1;
    }
    year
}

pub struct Central;
impl LocalZone for Central {
    fn offset_for_utc(&self, utc: i64) -> i32 {
        let year = year_of(utc);
        let start = sunday(year, 3, 2) * TICKS_PER_DAY + 8 * TICKS_PER_HOUR;
        let end = sunday(year, 11, 1) * TICKS_PER_DAY + 7 * TICKS_PER_HOUR;
        if (start..end).contains(&utc) {
            -300
        } else {
            -360
        }
    }
    fn offset_for_local(&self, wall: i64) -> i32 {
        // Invalid and ambiguous wall times use the standard offset.
        let year = year_of(wall);
        let start = sunday(year, 3, 2) * TICKS_PER_DAY + 3 * TICKS_PER_HOUR;
        let end = sunday(year, 11, 1) * TICKS_PER_DAY + TICKS_PER_HOUR;
        if (start..end).contains(&wall) {
            -300
        } else {
            -360
        }
    }
}
