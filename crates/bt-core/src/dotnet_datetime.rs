//! `DateTime.TryParse(text, CultureInfo.InvariantCulture,
//! DateTimeStyles.RoundtripKind)`, which Newtonsoft's `ReadAsDateTime` falls
//! back to when its own ISO/Microsoft formats do not match.
//!
//! .NET's lexer-driven parser accepts an open-ended set of forms. This is an
//! explicit grammar covering every form captured from the runtime in
//! folder_rules_reference.json (D33): numeric Y-M-D and M/D/Y orders, month
//! and weekday names, 12/24-hour times, fractions rounded to ticks, and Z/GMT
//! or numeric offsets. Inputs outside the grammar are rejected.
use crate::newtonsoft::{
    Date, DateKind, LocalZone, MAX_TICKS, TICKS_PER_DAY, TICKS_PER_MINUTE, civil_from_days,
    days_from_civil, days_in_month, local_date,
};

#[derive(Clone, Debug, PartialEq)]
enum Lex {
    Num(String),
    Word(String),
    Sym(char),
}

fn tokenize(text: &str) -> Vec<(Lex, bool)> {
    let chars: Vec<char> = text.trim().chars().collect();
    let mut out = vec![];
    let mut i = 0;
    let mut space = false;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            space = true;
            i += 1;
            continue;
        }
        let start = i;
        let lex = if c.is_ascii_digit() {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            Lex::Num(chars[start..i].iter().collect())
        } else if c.is_alphabetic() {
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            Lex::Word(chars[start..i].iter().collect::<String>().to_lowercase())
        } else {
            i += 1;
            Lex::Sym(c)
        };
        out.push((lex, space));
        space = false;
    }
    out
}

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const DAYS: [&str; 7] = [
    "sunday",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
];

fn named(word: &str, names: &[&str]) -> Option<usize> {
    names
        .iter()
        .position(|n| *n == word || (word.len() == 3 && n.starts_with(word)))
}

#[derive(Clone, Copy)]
enum Zone {
    None,
    Utc,
    Offset(i64),
}

struct Cursor<'a> {
    tokens: &'a [(Lex, bool)],
    i: usize,
}

impl Cursor<'_> {
    fn at(&self, offset: usize) -> Option<&Lex> {
        self.tokens.get(self.i + offset).map(|t| &t.0)
    }
    fn spaced(&self, offset: usize) -> bool {
        self.tokens.get(self.i + offset).is_some_and(|t| t.1)
    }
    /// A number of `min..=max` digits at `offset`.
    fn num(&self, offset: usize, min: usize, max: usize) -> Option<u32> {
        match self.at(offset) {
            Some(Lex::Num(n)) if (min..=max).contains(&n.len()) => n.parse().ok(),
            _ => None,
        }
    }
    fn sym(&self, offset: usize, c: char) -> bool {
        self.at(offset) == Some(&Lex::Sym(c))
    }
    fn word(&self, offset: usize) -> Option<&str> {
        match self.at(offset) {
            Some(Lex::Word(w)) => Some(w),
            _ => None,
        }
    }
    fn month(&self, offset: usize) -> Option<u32> {
        named(self.word(offset)?, &MONTHS).map(|m| m as u32 + 1)
    }
    fn weekday(&mut self) -> Option<Option<usize>> {
        let day = named(self.word(0)?, &DAYS)?;
        self.i += 1;
        if self.sym(0, ',') {
            self.i += 1;
        }
        Some(Some(day))
    }
}

fn two_digit_year(year: u32, digits: usize) -> u32 {
    match digits {
        4 => year,
        _ if year <= 49 => 2000 + year,
        _ => 1900 + year,
    }
}

/// (year, month, day, tokens used, ISO-like); a `0` year means "this year".
fn date(c: &Cursor<'_>) -> Option<(u32, u32, u32, usize, bool)> {
    let len = |o: usize| match c.at(o) {
        Some(Lex::Num(n)) => n.len(),
        _ => 0,
    };
    // yyyy S M S d | yyyy S M
    if let Some(year) = c.num(0, 4, 4) {
        for s in ['-', '/', '.'] {
            if c.sym(1, s) {
                if let Some(month) = c.num(2, 1, 2) {
                    if c.sym(3, s) {
                        let day = c.num(4, 1, 2)?;
                        return Some((year, month, day, 5, s == '-'));
                    }
                    if s != '.' {
                        return Some((year, month, 1, 3, false));
                    }
                }
            }
        }
        if let Some(month) = c.month(1) {
            return Some(match c.num(2, 1, 2) {
                Some(day) => (year, month, day, 3, false),
                None => (year, month, 1, 2, false),
            });
        }
        return None;
    }
    // M S d S y | M / yyyy | M / d
    if let Some(month) = c.num(0, 1, 2) {
        for s in ['/', '-'] {
            if !c.sym(1, s) {
                continue;
            }
            // d - Mon - yyyy
            if let Some(name) = c.month(2) {
                if c.sym(3, s) {
                    let year = c.num(4, 4, 4)?;
                    return Some((year, name, month, 5, false));
                }
                return None;
            }
            if c.sym(3, s) {
                let day = c.num(2, 1, 2)?;
                let digits = len(4);
                let year = c.num(4, 1, 4).filter(|_| digits != 3)?;
                return Some((two_digit_year(year, digits), month, day, 5, false));
            }
            if s == '/' {
                if let Some(year) = c.num(2, 4, 4) {
                    return Some((year, month, 1, 3, false));
                }
                if let Some(day) = c.num(2, 1, 2) {
                    return Some((0, month, day, 3, false));
                }
            }
            return None;
        }
        // d Mon yyyy | n Mon (n is a year)
        if let Some(name) = c.month(1) {
            return Some(match c.num(2, 4, 4) {
                Some(year) => (year, name, month, 3, false),
                None => (two_digit_year(month, len(0)), name, 1, 2, false),
            });
        }
        // M d yyyy | M d
        if let Some(day) = c.num(1, 1, 2) {
            return Some(match c.num(2, 4, 4) {
                Some(year) => (year, month, day, 3, false),
                None => (0, month, day, 2, false),
            });
        }
        return None;
    }
    // Mon [-] d [,] [-] yyyy | Mon yyyy | Mon d
    let month = c.month(0)?;
    let mut o = 1;
    if c.sym(o, '-') {
        o += 1;
    }
    if let Some(year) = c.num(o, 4, 4) {
        return Some((year, month, 1, o + 1, false));
    }
    let day = c.num(o, 1, 2)?;
    let mut y = o + 1;
    if c.sym(y, ',') || c.sym(y, '-') {
        y += 1;
    }
    match c.num(y, 4, 4) {
        Some(year) => Some((year, month, day, y + 1, false)),
        None => Some((0, month, day, o + 1, false)),
    }
}

/// (hour, minute, second, fraction ticks, tokens used).
fn time(c: &Cursor<'_>, strict: bool) -> Option<(u32, u32, u32, i64, usize)> {
    let (field_min, field_max) = if strict { (2, 2) } else { (1, 2) };
    let hour = c.num(0, field_min, field_max)?;
    let (mut minute, mut second, mut fraction, mut o) = (0, 0, 0_i64, 1);
    if c.sym(1, ':') {
        minute = c.num(2, field_min, field_max)?;
        o = 3;
        if c.sym(3, ':') {
            second = c.num(4, field_min, field_max)?;
            o = 5;
            if c.sym(5, '.') {
                o = 6;
                if let Some(Lex::Num(digits)) = c.at(6) {
                    let value: f64 = format!("0.{digits}").parse().ok()?;
                    fraction = (value * 10_000_000.0).round_ties_even() as i64;
                    o = 7;
                }
            }
        }
    }
    let designator = c.word(o).filter(|w| matches!(*w, "am" | "pm"));
    let hour = match designator {
        _ if strict && designator.is_some() => return None,
        Some("am") if hour > 12 => return None,
        Some("am") if hour == 12 => 0,
        Some("pm") if hour < 12 => hour + 12,
        None if o == 1 => return None,
        _ => hour,
    };
    if designator.is_some() {
        o += 1;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some((hour, minute, second, fraction, o))
}

fn zone_suffix(c: &Cursor<'_>) -> Option<(Zone, usize)> {
    match c.at(0) {
        Some(Lex::Word(w)) if w == "z" || w == "gmt" && c.spaced(0) => Some((Zone::Utc, 1)),
        Some(Lex::Sym(sign @ ('+' | '-'))) => {
            let (hours, minutes, used) =
                if let (Some(h), true, Some(m)) = (c.num(1, 2, 2), c.sym(2, ':'), c.num(3, 2, 2)) {
                    (h, m, 4)
                } else if let Some(hm) = c.num(1, 4, 4) {
                    (hm / 100, hm % 100, 2)
                } else {
                    (c.num(1, 1, 2)?, 0, 2)
                };
            if hours > 23 || minutes > 59 {
                return None;
            }
            let minutes = i64::from(hours * 60 + minutes);
            Some((
                Zone::Offset(if *sign == '-' { -minutes } else { minutes }),
                used,
            ))
        }
        _ => None,
    }
}

/// `DateTimeParse.AdjustTimeZoneToLocal` for an explicit offset.
fn adjust_to_local(ticks: i64, offset: i64, zone: &(impl LocalZone + ?Sized)) -> Option<Date> {
    let offset = offset * TICKS_PER_MINUTE;
    let mut local = ticks - offset;
    if ticks < TICKS_PER_DAY {
        local += i64::from(zone.offset_for_local(ticks)) * TICKS_PER_MINUTE;
        if local < 0 {
            local += TICKS_PER_DAY;
        }
    } else if !(0..=MAX_TICKS).contains(&local) {
        local += i64::from(zone.offset_for_local(ticks)) * TICKS_PER_MINUTE;
    } else {
        let offset_minutes = zone.offset_for_utc(local);
        let converted = local + i64::from(offset_minutes) * TICKS_PER_MINUTE;
        return (0..=MAX_TICKS).contains(&converted).then_some(Date {
            ticks: converted,
            kind: DateKind::Local { offset_minutes },
        });
    }
    (0..=MAX_TICKS)
        .contains(&local)
        .then(|| local_date(local, zone))
}

/// `today_ticks` is the local date (midnight) used for time-only input and
/// for forms without a year.
pub fn try_parse(text: &str, today_ticks: i64, zone: &(impl LocalZone + ?Sized)) -> Option<Date> {
    let tokens = tokenize(text);
    let mut c = Cursor {
        tokens: &tokens,
        i: 0,
    };
    let mut weekday = c.weekday().flatten();
    let parsed = date(&c);
    let mut iso = false;
    let (today_year, _, _) = civil_from_days(today_ticks / TICKS_PER_DAY);
    let mut ymd = None;
    if let Some((year, month, day, used, iso_like)) = parsed {
        c.i += used;
        iso = iso_like;
        ymd = Some((if year == 0 { today_year as u32 } else { year }, month, day));
        if weekday.is_none() {
            weekday = c.weekday().flatten();
        }
    }
    let mut strict = false;
    if ymd.is_some() {
        if c.word(0) == Some("t") && !c.spaced(0) {
            if !iso {
                return None;
            }
            strict = true;
            c.i += 1;
        } else if c.sym(0, ',') {
            c.i += 1;
        }
    }
    let mut clock = None;
    if let Some((h, m, s, fraction, used)) = time(&c, strict) {
        clock = Some((h, m, s, fraction));
        c.i += used;
    } else if strict {
        return None;
    }
    if ymd.is_none() && clock.is_none() {
        return None;
    }
    let mut kind = Zone::None;
    if clock.is_some() {
        if let Some((z, used)) = zone_suffix(&c) {
            kind = z;
            c.i += used;
        }
    }
    if weekday.is_none() {
        weekday = c.weekday().flatten();
    }
    if c.i != tokens.len() {
        return None;
    }
    let days = match ymd {
        Some((year, month, day)) => {
            let (year, month, day) = (year as i32, month as i32, day as i32);
            if !(1..=9999).contains(&year)
                || !(1..=12).contains(&month)
                || day < 1
                || day > days_in_month(year, month)
            {
                return None;
            }
            days_from_civil(year, month, day)
        }
        None => today_ticks / TICKS_PER_DAY,
    };
    if weekday.is_some_and(|w| (days + 1) % 7 != w as i64) {
        return None;
    }
    let (h, m, s, fraction) = clock.unwrap_or_default();
    let ticks = days * TICKS_PER_DAY + i64::from(h * 3600 + m * 60 + s) * 10_000_000 + fraction;
    if ticks > MAX_TICKS {
        return None;
    }
    match kind {
        Zone::None => Some(Date {
            ticks,
            kind: DateKind::Unspecified,
        }),
        Zone::Utc => Some(Date {
            ticks,
            kind: DateKind::Utc,
        }),
        Zone::Offset(offset) => adjust_to_local(ticks, offset, zone),
    }
}
