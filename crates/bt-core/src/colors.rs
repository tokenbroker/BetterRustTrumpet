//! WPF `ColorConverter.ConvertFromString` and `Color.ToString()`, which the
//! original uses for every stored color setting, `.btsettings` color import
//! and theme brushes. Pinned by colors_reference.json (.NET 8 WPF).
//!
//! scRGB (`sc#a, r, g, b`) parsing and printing use the current culture; it
//! is supplied as a [`NumberCulture`]. ICC `ContextColor` strings are always
//! rejected because the port never loads color profiles (D32).
use crate::text;
use thiserror::Error;

mod known {
    include!("data/wpf_colors.rs");
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("invalid color text")]
pub struct Error;

/// The `NumberFormatInfo` members used by float parsing and printing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberCulture<'a> {
    pub decimal_separator: &'a str,
    pub group_separator: &'a str,
    pub positive_sign: &'a str,
    pub negative_sign: &'a str,
    pub nan_symbol: &'a str,
    pub positive_infinity_symbol: &'a str,
    pub negative_infinity_symbol: &'a str,
}
impl NumberCulture<'_> {
    pub const INVARIANT: Self = Self {
        decimal_separator: ".",
        group_separator: ",",
        positive_sign: "+",
        negative_sign: "-",
        nan_symbol: "NaN",
        positive_infinity_symbol: "Infinity",
        negative_infinity_symbol: "-Infinity",
    };
    /// `TokenizerHelper.GetNumericListSeparator`.
    pub fn list_separator(&self) -> char {
        if self.decimal_separator.starts_with(',') {
            ';'
        } else {
            ','
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Raw `FromScRgb` inputs (a, r, g, b); such colors print as `sc#`.
    pub sc_rgb: Option<[f32; 4]>,
}

impl Color {
    pub const TRANSPARENT: Self = Self::from_argb(0x00ff_ffff);

    pub const fn from_argb(argb: u32) -> Self {
        Self {
            a: (argb >> 24) as u8,
            r: (argb >> 16) as u8,
            g: (argb >> 8) as u8,
            b: argb as u8,
            sc_rgb: None,
        }
    }
    /// `Color.FromScRgb`: alpha is clamped, channels use the sRGB curve.
    pub fn from_sc_rgb(a: f32, r: f32, g: f32, b: f32) -> Self {
        let clamped = a.clamp(0.0, 1.0);
        Self {
            a: (clamped * 255.0 + 0.5) as u8,
            r: sc_to_srgb(r),
            g: sc_to_srgb(g),
            b: sc_to_srgb(b),
            sc_rgb: Some([a, r, g, b]),
        }
    }
    /// `Color.ToString()` in the given culture.
    pub fn to_text(&self, culture: &NumberCulture<'_>) -> String {
        match self.sc_rgb {
            None => format!("#{:02X}{:02X}{:02X}{:02X}", self.a, self.r, self.g, self.b),
            Some([a, r, g, b]) => {
                let sep = culture.list_separator();
                let f = |v: f32| format_float(v, culture);
                format!("sc#{}{sep} {}{sep} {}{sep} {}", f(a), f(r), f(g), f(b))
            }
        }
    }
}

fn sc_to_srgb(value: f32) -> u8 {
    // NaN and non-positive values map to 0, as `!(val > 0.0)` does.
    if value.is_nan() || value <= 0.0 {
        0
    } else if f64::from(value) <= 0.0031308 {
        (255.0_f32 * value * 12.92_f32 + 0.5) as u8
    } else if value < 1.0 {
        let curve = f64::from(value).powf(1.0 / 2.4) as f32;
        (255.0_f32 * (1.055_f32 * curve - 0.055_f32) + 0.5) as u8
    } else {
        255
    }
}

fn hex(c: char) -> Result<u8, Error> {
    c.to_digit(16)
        .filter(|_| c.is_ascii())
        .map(|d| d as u8)
        .ok_or(Error)
}

/// `ColorConverter.ConvertFromString`.
pub fn parse(text: &str, culture: &NumberCulture<'_>) -> Result<Color, Error> {
    let trimmed = text.trim();
    let units = trimmed.encode_utf16().count();
    if trimmed.starts_with('#') && matches!(units, 4 | 5 | 7 | 9) {
        let d: Vec<u8> = trimmed[1..].chars().map(hex).collect::<Result<_, _>>()?;
        let pair = |i: usize| d[i] * 16 + d[i + 1];
        let twice = |i: usize| d[i] * 17;
        let (a, r, g, b) = match units {
            9 => (pair(0), pair(2), pair(4), pair(6)),
            7 => (255, pair(0), pair(2), pair(4)),
            5 => (twice(0), twice(1), twice(2), twice(3)),
            _ => (255, twice(0), twice(1), twice(2)),
        };
        return Ok(Color {
            a,
            r,
            g,
            b,
            sc_rgb: None,
        });
    }
    if let Some(tokens) = trimmed.strip_prefix("sc#") {
        return parse_sc_rgb(tokens, culture);
    }
    if text::ordinal_starts_with(trimmed, "ContextColor ") {
        return Err(Error);
    }
    // KnownColors upper-cases with the invariant culture, then compares.
    let upper = text::invariant_upper(trimmed);
    known::KNOWN_COLORS
        .iter()
        .find(|(name, _)| name.to_ascii_uppercase() == upper)
        .map(|(_, argb)| Color::from_argb(*argb))
        .ok_or(Error)
}

/// `AppSettings.ParseColorSetting`: empty or invalid text is Transparent.
pub fn parse_setting(stored: Option<&str>, culture: &NumberCulture<'_>) -> Color {
    match stored {
        None | Some("") => Color::TRANSPARENT,
        Some(text) => parse(text, culture).unwrap_or(Color::TRANSPARENT),
    }
}

/// `TokenizerHelper` without quoting: tokens split by whitespace and at most
/// one separator; empty tokens and dangling separators are errors.
fn tokens(text: &str, separator: char) -> Result<Vec<&str>, Error> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let byte = |i: usize| chars.get(i).map_or(text.len(), |c| c.0);
    let mut i = 0;
    while i < chars.len() && chars[i].1.is_whitespace() {
        i += 1;
    }
    let mut out = vec![];
    while i < chars.len() {
        let start = i;
        while i < chars.len() && !chars[i].1.is_whitespace() && chars[i].1 != separator {
            i += 1;
        }
        let end = i;
        // ScanToNextToken
        let mut separators = 0;
        while i < chars.len() {
            match chars[i].1 {
                c if c == separator => {
                    separators += 1;
                    if separators > 1 {
                        return Err(Error);
                    }
                }
                c if c.is_whitespace() => {}
                _ => break,
            }
            i += 1;
        }
        if separators > 0 && i >= chars.len() || end == start {
            return Err(Error);
        }
        out.push(&text[byte(start)..byte(end)]);
    }
    Ok(out)
}

fn parse_sc_rgb(text: &str, culture: &NumberCulture<'_>) -> Result<Color, Error> {
    let tokens = tokens(text, culture.list_separator())?;
    let values: Vec<f32> = tokens
        .iter()
        .map(|t| parse_float(t, culture).ok_or(Error))
        .collect::<Result<_, _>>()?;
    match values[..] {
        [r, g, b] => Ok(Color::from_sc_rgb(1.0, r, g, b)),
        [a, r, g, b] => Ok(Color::from_sc_rgb(a, r, g, b)),
        _ => Err(Error),
    }
}

fn number_white(c: char) -> bool {
    c == ' ' || ('\t'..='\r').contains(&c)
}
fn allow_hyphen(culture: &NumberCulture<'_>) -> bool {
    matches!(
        culture.negative_sign,
        "\u{2012}" | "\u{207b}" | "\u{208b}" | "\u{2212}" | "\u{2796}" | "\u{fe63}" | "\u{ff0d}"
    )
}

/// `float.Parse(text, NumberStyles.Float | NumberStyles.AllowThousands, culture)`.
pub fn parse_float(text: &str, culture: &NumberCulture<'_>) -> Option<f32> {
    numeric_float(text, culture).or_else(|| symbol_float(text, culture))
}

fn numeric_float(text: &str, culture: &NumberCulture<'_>) -> Option<f32> {
    let mut rest = text.trim_start_matches(number_white);
    let mut negative = false;
    if !culture.positive_sign.is_empty() && rest.starts_with(culture.positive_sign) {
        rest = &rest[culture.positive_sign.len()..];
    } else if !culture.negative_sign.is_empty() && rest.starts_with(culture.negative_sign) {
        negative = true;
        rest = &rest[culture.negative_sign.len()..];
    } else if allow_hyphen(culture) && rest.starts_with('-') {
        negative = true;
        rest = &rest[1..];
    }
    let (mut integral, mut fraction) = (String::new(), String::new());
    let (mut digits, mut decimal) = (false, false);
    while let Some(c) = rest.chars().next() {
        if c.is_ascii_digit() {
            digits = true;
            if decimal {
                &mut fraction
            } else {
                &mut integral
            }
            .push(c);
            rest = &rest[1..];
        } else if !decimal
            && !culture.decimal_separator.is_empty()
            && rest.starts_with(culture.decimal_separator)
        {
            decimal = true;
            rest = &rest[culture.decimal_separator.len()..];
        } else if digits
            && !decimal
            && !culture.group_separator.is_empty()
            && rest.starts_with(culture.group_separator)
        {
            rest = &rest[culture.group_separator.len()..];
        } else {
            break;
        }
    }
    if !digits {
        return None;
    }
    let mut exponent = String::new();
    if let Some(after) = rest.strip_prefix(['e', 'E']) {
        let (sign, unsigned) = if !culture.positive_sign.is_empty()
            && after.starts_with(culture.positive_sign)
        {
            ("", &after[culture.positive_sign.len()..])
        } else if !culture.negative_sign.is_empty() && after.starts_with(culture.negative_sign) {
            ("-", &after[culture.negative_sign.len()..])
        } else if allow_hyphen(culture) && after.starts_with('-') {
            ("-", &after[1..])
        } else {
            ("", after)
        };
        let count = unsigned.bytes().take_while(u8::is_ascii_digit).count();
        if count > 0 {
            // .NET caps huge exponents; any cap beyond float range is equivalent.
            let value = &unsigned[..count];
            let capped = if value.len() > 5 { "99999" } else { value };
            exponent = format!("e{sign}{capped}");
            rest = &unsigned[count..];
        }
    }
    let rest = rest.trim_start_matches(number_white);
    if !rest.bytes().all(|b| b == 0) {
        return None;
    }
    let sign = if negative { "-" } else { "" };
    let fraction = if fraction.is_empty() {
        "0".into()
    } else {
        fraction
    };
    let integral = if integral.is_empty() {
        "0".into()
    } else {
        integral
    };
    format!("{sign}{integral}.{fraction}{exponent}")
        .parse()
        .ok()
}

fn symbol_float(text: &str, culture: &NumberCulture<'_>) -> Option<f32> {
    let value = text.trim();
    let eq = |s: &str, symbol: &str| text::ordinal_ignore_case(s, symbol);
    if eq(value, culture.positive_infinity_symbol) {
        return Some(f32::INFINITY);
    }
    if eq(value, culture.negative_infinity_symbol) {
        return Some(f32::NEG_INFINITY);
    }
    if eq(value, culture.nan_symbol) {
        return Some(f32::NAN);
    }
    if text::ordinal_starts_with(value, culture.positive_sign) {
        let rest = &value[culture.positive_sign.len()..];
        if eq(rest, culture.positive_infinity_symbol) {
            return Some(f32::INFINITY);
        }
        return eq(rest, culture.nan_symbol).then_some(f32::NAN);
    }
    let negative = if text::ordinal_starts_with(value, culture.negative_sign) {
        Some(&value[culture.negative_sign.len()..])
    } else if allow_hyphen(culture) {
        value.strip_prefix('-')
    } else {
        None
    };
    negative
        .filter(|rest| eq(rest, culture.nan_symbol))
        .map(|_| f32::NAN)
}

/// .NET Core 3.0+ `float.ToString(culture)`: shortest round-trip digits,
/// scientific beyond 9 significant positions or before -3.
pub fn format_float(value: f32, culture: &NumberCulture<'_>) -> String {
    if value.is_nan() {
        return culture.nan_symbol.into();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            culture.positive_infinity_symbol
        } else {
            culture.negative_infinity_symbol
        }
        .into();
    }
    let sign = if value.is_sign_negative() {
        culture.negative_sign
    } else {
        ""
    };
    if value == 0.0 {
        return format!("{sign}0");
    }
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let exponent: i32 = exponent.parse().expect("exponent digits");
    let position = exponent + 1;
    let dec = culture.decimal_separator;
    if !(-3..=9).contains(&position) {
        let (first, rest) = digits.split_at(1);
        let fraction = if rest.is_empty() {
            String::new()
        } else {
            format!("{dec}{rest}")
        };
        let exp_sign = if exponent < 0 {
            culture.negative_sign
        } else {
            culture.positive_sign
        };
        return format!("{sign}{first}{fraction}E{exp_sign}{:02}", exponent.abs());
    }
    let length = digits.len() as i32;
    if position <= 0 {
        format!(
            "{sign}0{dec}{}{digits}",
            "0".repeat(position.unsigned_abs() as usize)
        )
    } else if position >= length {
        format!("{sign}{digits}{}", "0".repeat((position - length) as usize))
    } else {
        let (whole, fraction) = digits.split_at(position as usize);
        format!("{sign}{whole}{dec}{fraction}")
    }
}
