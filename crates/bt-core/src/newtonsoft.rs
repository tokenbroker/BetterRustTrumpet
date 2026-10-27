//! Newtonsoft.Json 13 reader and conversion semantics needed to read values
//! the original wrote or a user hand-edited. Pinned by store_reference.json,
//! which executes the original JsonFileSettingsBag on .NET 8.
//!
//! The reader is lenient in exactly the ways `JsonTextReader` is: comments,
//! single quotes, unquoted names, octal/hex integers, `NaN`/`Infinity`,
//! `undefined`, empty array elements and trailing commas. String values that
//! look like dates become [`Date`] tokens, as `DateParseHandling.DateTime`
//! does. Local-time conversion is a native boundary supplied by [`LocalZone`].
use crate::{
    legacy_xml::{Point, Rect, WindowPlacement},
    models::HotkeyData,
    numbers::IntegerFormat,
    text,
};
use thiserror::Error;

mod keys {
    include!("data/dotnet_keys.rs");
}

pub(crate) const TICKS_PER_MINUTE: i64 = 600_000_000;
pub(crate) const TICKS_PER_DAY: i64 = 864_000_000_000;
pub(crate) const MAX_TICKS: i64 = 3_155_378_975_999_999_999;
const JAVASCRIPT_EPOCH_TICKS: i64 = 621_355_968_000_000_000;
/// `double.NaN` on .NET (sign bit set), as distinct from Rust's `f64::NAN`.
pub const DOTNET_NAN: f64 = f64::from_bits(0xfff8_0000_0000_0000);
/// `JsonTextReader` refuses integers longer than this many characters.
const MAX_INTEGER_LENGTH: usize = 380;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("invalid Newtonsoft JSON or conversion")]
pub struct Error;

/// Local time zone rules: the offset in minutes at a UTC instant, and the
/// `TimeZoneInfo.Local.GetUtcOffset` answer for an unspecified wall time.
pub trait LocalZone {
    fn offset_for_utc(&self, utc_ticks: i64) -> i32;
    fn offset_for_local(&self, local_ticks: i64) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateKind {
    Unspecified,
    Utc,
    /// Local time with the offset its serialization will print.
    Local {
        offset_minutes: i32,
    },
}
/// A .NET `DateTime`: 100ns ticks since 0001-01-01 plus its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub ticks: i64,
    pub kind: DateKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Null,
    Undefined,
    Bool(bool),
    Integer(i64),
    /// Canonical decimal text of an integer outside the Int64 range.
    BigInteger(String),
    Float(f64),
    String(String),
    Date(Date),
    /// A JavaScript constructor (`new Name(...)`), kept only for skipping.
    Constructor,
    /// Typed-mode number text, converted later by the `ReadAs*` rules.
    RawNumber(String),
    Object(Vec<(String, Token)>),
    Array(Vec<Token>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Root,
    Object,
    Array,
    Constructor,
}

struct Reader<'a, Z: ?Sized> {
    chars: Vec<char>,
    pos: usize,
    zone: &'a Z,
    /// Typed mode (`DeserializeObject<T>`): strings and numbers stay raw for
    /// the property's `ReadAs*` method instead of `ReadType.Read` parsing.
    raw: bool,
}

fn whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\0'
}

impl<Z: LocalZone + ?Sized> Reader<'_, Z> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }
    /// `EatWhitespace`: whitespace and embedded NULs, but not comments.
    fn eat_whitespace(&mut self) {
        while self.peek().is_some_and(whitespace) {
            self.pos += 1;
        }
    }
    fn comment(&mut self) -> Result<(), Error> {
        match self.at(1) {
            Some('*') => {
                self.pos += 2;
                loop {
                    match self.peek() {
                        None => return Err(Error),
                        Some('*') if self.at(1) == Some('/') => {
                            self.pos += 2;
                            return Ok(());
                        }
                        Some(_) => self.pos += 1,
                    }
                }
            }
            Some('/') => {
                self.pos += 2;
                while self.peek().is_some_and(|c| c != '\r' && c != '\n') {
                    self.pos += 1;
                }
                Ok(())
            }
            _ => Err(Error),
        }
    }
    /// Whitespace and comments, as skipped between tokens.
    fn skip(&mut self) -> Result<bool, Error> {
        let mut commented = false;
        loop {
            self.eat_whitespace();
            if self.peek() != Some('/') {
                return Ok(commented);
            }
            self.comment()?;
            commented = true;
        }
    }
    fn separator(&self, context: Context) -> bool {
        match self.peek() {
            None => true,
            Some('}' | ']' | ',') => true,
            Some(')') => context == Context::Constructor,
            Some('/') => matches!(self.at(1), Some('*' | '/')),
            Some(c) => c.is_whitespace(),
        }
    }
    fn literal(&mut self, word: &str, context: Context) -> Result<(), Error> {
        for expected in word.chars() {
            if self.peek() != Some(expected) {
                return Err(Error);
            }
            self.pos += 1;
        }
        if self.separator(context) {
            Ok(())
        } else {
            Err(Error)
        }
    }
    fn hex4(&mut self) -> Result<u32, Error> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self.peek().and_then(|c| c.to_digit(16)).ok_or(Error)?;
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }
    fn string(&mut self) -> Result<String, Error> {
        let quote = self.peek().ok_or(Error)?;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or(Error)?;
            self.pos += 1;
            match c {
                c if c == quote => return Ok(out),
                '\\' => {
                    let escape = self.peek().ok_or(Error)?;
                    self.pos += 1;
                    out.push(match escape {
                        'b' => '\u{8}',
                        't' => '\t',
                        'n' => '\n',
                        'f' => '\u{c}',
                        'r' => '\r',
                        '\\' | '"' | '\'' | '/' => escape,
                        'u' => {
                            let unit = self.hex4()?;
                            if (0xd800..0xdc00).contains(&unit)
                                && self.peek() == Some('\\')
                                && self.at(1) == Some('u')
                            {
                                let save = self.pos;
                                self.pos += 2;
                                let low = self.hex4()?;
                                if (0xdc00..0xe000).contains(&low) {
                                    let code = 0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00);
                                    out.push(char::from_u32(code).ok_or(Error)?);
                                    continue;
                                }
                                self.pos = save;
                            }
                            // Lone surrogates cannot be represented in UTF-8.
                            char::from_u32(unit).unwrap_or('\u{fffd}')
                        }
                        _ => return Err(Error),
                    });
                }
                c => out.push(c),
            }
        }
    }
    fn property_name(&mut self) -> Result<String, Error> {
        let name = match self.peek().ok_or(Error)? {
            '"' | '\'' => self.string()?,
            c if text::bmp_letter_or_digit(c) || c == '_' || c == '$' => {
                let mut name = String::new();
                loop {
                    match self.peek().ok_or(Error)? {
                        c if text::bmp_letter_or_digit(c) || c == '_' || c == '$' => {
                            name.push(c);
                            self.pos += 1;
                        }
                        c if whitespace(c) || c == ':' => break,
                        _ => return Err(Error),
                    }
                }
                name
            }
            _ => return Err(Error),
        };
        self.eat_whitespace();
        if self.peek() != Some(':') {
            return Err(Error);
        }
        self.pos += 1;
        Ok(name)
    }
    fn number(&mut self) -> Result<Token, Error> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_hexdigit() || matches!(c, '-' | '.' | 'x' | 'X' | '+'))
        {
            self.pos += 1;
        }
        match self.peek() {
            None | Some('\0' | ',' | '}' | ']' | ')' | '/') => {}
            Some(c) if c.is_whitespace() => {}
            Some(_) => return Err(Error),
        }
        let number: String = self.chars[start..self.pos].iter().collect();
        if self.raw {
            return Ok(Token::RawNumber(number));
        }
        classify(&number)
    }
}

/// `ParseReadNumber` with `ReadType.Read`.
fn classify(number: &str) -> Result<Token, Error> {
    match number {
        "NaN" => return Ok(Token::Float(DOTNET_NAN)),
        "Infinity" => return Ok(Token::Float(f64::INFINITY)),
        "-Infinity" => return Ok(Token::Float(f64::NEG_INFINITY)),
        _ => {}
    }
    {
        let bytes = number.as_bytes();
        if bytes.len() > 1 && bytes[0] == b'0' && !matches!(bytes[1], b'.' | b'e' | b'E') {
            let parsed = match number.get(..2) {
                Some("0x" | "0X") => radix(&number[2..], 16),
                _ => radix(number, 8),
            };
            return parsed.map(Token::Integer);
        }
        let (negative, digits) = match number.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, number),
        };
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(value) = number.parse::<i64>() {
                return Ok(Token::Integer(value));
            }
            if number.len() > MAX_INTEGER_LENGTH {
                return Err(Error);
            }
            let digits = digits.trim_start_matches('0');
            return Ok(Token::BigInteger(if negative {
                format!("-{digits}")
            } else {
                digits.to_owned()
            }));
        }
        let value: f64 = number.parse().map_err(|_| Error)?;
        // The captured reader yields positive zero for "-0.0" and underflow.
        Ok(Token::Float(if value == 0.0 { 0.0 } else { value }))
    }
}

impl<Z: LocalZone + ?Sized> Reader<'_, Z> {
    fn value(&mut self, context: Context) -> Result<Token, Error> {
        self.skip()?;
        let c = self.peek().ok_or(Error)?;
        Ok(match c {
            '"' | '\'' => {
                let value = self.string()?;
                if self.raw {
                    return Ok(Token::String(value));
                }
                match date(&value, self.zone)? {
                    Some(date) => Token::Date(date),
                    None => Token::String(value),
                }
            }
            't' => {
                self.literal("true", context)?;
                Token::Bool(true)
            }
            'f' => {
                self.literal("false", context)?;
                Token::Bool(false)
            }
            'n' if self.at(1) == Some('e') => {
                if context == Context::Root {
                    return Err(Error);
                }
                self.constructor()?
            }
            'n' => {
                self.literal("null", context)?;
                Token::Null
            }
            'u' => {
                self.literal("undefined", context)?;
                Token::Undefined
            }
            'N' => {
                self.literal("NaN", context)?;
                if self.raw {
                    Token::RawNumber("NaN".into())
                } else {
                    Token::Float(DOTNET_NAN)
                }
            }
            'I' => {
                self.literal("Infinity", context)?;
                if self.raw {
                    Token::RawNumber("Infinity".into())
                } else {
                    Token::Float(f64::INFINITY)
                }
            }
            '-' if self.at(1) == Some('I') => {
                self.literal("-Infinity", context)?;
                if self.raw {
                    Token::RawNumber("-Infinity".into())
                } else {
                    Token::Float(f64::NEG_INFINITY)
                }
            }
            '{' => {
                self.pos += 1;
                Token::Object(self.object(false)?)
            }
            '[' => {
                self.pos += 1;
                Token::Array(self.items(']', Context::Array)?.0)
            }
            '-' | '.' | '0'..='9' => self.number()?,
            _ => return Err(Error),
        })
    }
    /// Array elements or constructor arguments. A comma in value position is
    /// an undefined element; the closing bracket may follow a comma.
    fn items(&mut self, close: char, context: Context) -> Result<(Vec<Token>, bool), Error> {
        let mut items = vec![];
        let mut commented = false;
        loop {
            commented |= self.skip()?;
            match self.peek().ok_or(Error)? {
                c if c == close => {
                    self.pos += 1;
                    return Ok((items, commented));
                }
                ',' => items.push(Token::Undefined),
                ']' | ')' | '}' => return Err(Error),
                _ => items.push(self.value(context)?),
            }
            commented |= self.skip()?;
            match self.peek().ok_or(Error)? {
                ',' => self.pos += 1,
                c if c == close => {
                    self.pos += 1;
                    return Ok((items, commented));
                }
                _ => return Err(Error),
            }
        }
    }
    fn object(&mut self, root: bool) -> Result<Vec<(String, Token)>, Error> {
        let mut members = vec![];
        let context = if root { Context::Root } else { Context::Object };
        loop {
            self.skip()?;
            if self.peek().ok_or(Error)? == '}' {
                self.pos += 1;
                return Ok(members);
            }
            let name = self.property_name()?;
            self.skip()?;
            let value = match self.peek().ok_or(Error)? {
                ',' => Token::Undefined,
                _ => self.value(context)?,
            };
            // JTokenWriter: "last property name wins" by remove-then-append.
            // The root is a dictionary, where only the final value matters.
            if !root && !self.raw {
                members.retain(|(existing, _)| *existing != name);
            }
            members.push((name, value));
            self.skip()?;
            match self.peek().ok_or(Error)? {
                ',' => self.pos += 1,
                '}' => {
                    self.pos += 1;
                    return Ok(members);
                }
                _ => return Err(Error),
            }
        }
    }
    /// `new Name(args)`. JTokenWriter converts constructors named exactly
    /// `Date` while loading, so their arguments are validated here.
    fn constructor(&mut self) -> Result<Token, Error> {
        self.literal("new", Context::Constructor)?;
        self.eat_whitespace();
        let start = self.pos;
        loop {
            match self.peek().ok_or(Error)? {
                c if text::bmp_letter_or_digit(c) => self.pos += 1,
                '(' => break,
                c if c.is_whitespace() => break,
                _ => return Err(Error),
            }
        }
        let name: String = self.chars[start..self.pos].iter().collect();
        self.eat_whitespace();
        if self.peek() != Some('(') {
            return Err(Error);
        }
        self.pos += 1;
        let (args, commented) = self.items(')', Context::Constructor)?;
        if self.raw {
            // Skipped by Read: no JTokenWriter date conversion, but every
            // argument is still read with ReadType.Read.
            for arg in &args {
                check_read(arg, self.zone)?;
            }
        } else if name == "Date" {
            constructor_date(&args, commented)?;
        }
        Ok(Token::Constructor)
    }
}

/// `Convert.ToInt64(text, radix)`: two's complement over 64 bits.
fn radix(digits: &str, radix: u32) -> Result<i64, Error> {
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return Err(Error);
    }
    u64::from_str_radix(digits, radix)
        .map(|v| v as i64)
        .map_err(|_| Error)
}

fn constructor_date(args: &[Token], commented: bool) -> Result<(), Error> {
    if commented || args.is_empty() || args.len() > 7 {
        return Err(Error);
    }
    let mut values = [0_i64; 7];
    for (slot, arg) in values.iter_mut().zip(args) {
        let Token::Integer(value) = arg else {
            return Err(Error);
        };
        *slot = *value;
    }
    if args.len() == 1 {
        return javascript_ticks(values[0]).map(|_| ());
    }
    let part = |i: usize, default: i32| {
        if i < args.len() {
            values[i] as i32
        } else {
            default
        }
    };
    let (year, month, day) = (part(0, 0), part(1, 0).wrapping_add(1), part(2, 1));
    let (hour, minute, second, millisecond) = (part(3, 0), part(4, 0), part(5, 0), part(6, 0));
    if !(1..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
        || !(0..1000).contains(&millisecond)
    {
        return Err(Error);
    }
    Ok(())
}

/// Parses a settings document (`Dictionary<string, object>`). `Ok(None)` is a
/// null or empty document; trailing content other than comments is an error.
pub fn parse_object_document(
    text: &str,
    zone: &(impl LocalZone + ?Sized),
) -> Result<Option<Vec<(String, Token)>>, Error> {
    let mut reader = Reader {
        chars: text.chars().collect(),
        pos: 0,
        zone,
        raw: false,
    };
    reader.skip()?;
    let result = match reader.peek() {
        None => return Ok(None),
        Some('{') => {
            reader.pos += 1;
            Some(reader.object(true)?)
        }
        Some('n') => {
            reader.literal("null", Context::Root)?;
            None
        }
        Some('u') => {
            reader.literal("undefined", Context::Root)?;
            None
        }
        Some(_) => return Err(Error),
    };
    reader.skip()?;
    if reader.peek().is_some() {
        return Err(Error);
    }
    Ok(result)
}

// ---- typed mode (DeserializeObject<T>) -----------------------------------------

/// Parses a `DeserializeObject<T>` document in typed mode. `Ok(None)` is a
/// null or empty document; callers reject roots of the wrong shape.
pub fn parse_typed_document(
    text: &str,
    zone: &(impl LocalZone + ?Sized),
) -> Result<Option<Token>, Error> {
    let mut reader = Reader {
        chars: text.chars().collect(),
        pos: 0,
        zone,
        raw: true,
    };
    reader.skip()?;
    let result = match reader.peek() {
        None => return Ok(None),
        Some('n') if reader.at(1) == Some('u') => {
            reader.literal("null", Context::Root)?;
            None
        }
        Some(_) => Some(reader.value(Context::Root)?),
    };
    reader.skip()?;
    if reader.peek().is_some() {
        return Err(Error);
    }
    Ok(result)
}

/// `JsonReader.Skip` over a typed-mode value: every token is read with
/// `ReadType.Read`, so dates and numbers are parsed (and may throw).
pub fn check_read(token: &Token, zone: &(impl LocalZone + ?Sized)) -> Result<(), Error> {
    match token {
        Token::String(s) => date(s, zone).map(|_| ()),
        Token::RawNumber(n) => classify(n).map(|_| ()),
        Token::Object(members) => members.iter().try_for_each(|(_, v)| check_read(v, zone)),
        Token::Array(items) => items.iter().try_for_each(|v| check_read(v, zone)),
        _ => Ok(()),
    }
}

fn non_base10(number: &str) -> Option<(&str, u32)> {
    let bytes = number.as_bytes();
    if bytes.len() > 1 && bytes[0] == b'0' && !matches!(bytes[1], b'.' | b'e' | b'E') {
        Some(match number.get(..2) {
            Some("0x" | "0X") => (&number[2..], 16),
            _ => (number, 8),
        })
    } else {
        None
    }
}

/// `ReadAsString` in typed mode: number text is validated but kept verbatim.
pub fn typed_string(token: &Token) -> Result<Option<String>, Error> {
    match token {
        Token::String(s) => Ok(Some(s.clone())),
        Token::RawNumber(n) => {
            let valid = match (n.as_str(), non_base10(n)) {
                ("NaN" | "Infinity" | "-Infinity", _) => true,
                (_, Some((digits, base))) => radix(digits, base).is_ok(),
                _ => n.parse::<f64>().is_ok(),
            };
            if valid {
                Ok(Some(n.clone()))
            } else {
                Err(Error)
            }
        }
        Token::Bool(b) => Ok(Some(b.to_string())),
        Token::Null | Token::Undefined => Ok(None),
        _ => Err(Error),
    }
}

/// `ReadAsInt32` in typed mode (no date parsing of strings).
pub fn typed_int32(token: &Token) -> Result<Option<i32>, Error> {
    match token {
        Token::String(s) if s.is_empty() => Ok(None),
        Token::String(s) => IntegerFormat::INVARIANT.parse_i32(s).map(Some).ok_or(Error),
        Token::RawNumber(n) => {
            if let Some((digits, base)) = non_base10(n) {
                // Convert.ToInt32(text, 16 or 8): 32-bit two's complement.
                if digits.is_empty() || !digits.chars().all(|c| c.is_digit(base)) {
                    return Err(Error);
                }
                return u32::from_str_radix(digits, base)
                    .map(|v| Some(v as i32))
                    .map_err(|_| Error);
            }
            let digits = n.strip_prefix('-').unwrap_or(n);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error);
            }
            n.parse::<i32>().map(Some).map_err(|_| Error)
        }
        Token::Null | Token::Undefined => Ok(None),
        _ => Err(Error),
    }
}

/// `ReadAsDateTime` in typed mode. `fallback` is `DateTime.TryParse` with the
/// invariant culture and `DateTimeStyles.RoundtripKind`.
pub fn typed_datetime(
    token: &Token,
    zone: &(impl LocalZone + ?Sized),
    fallback: impl FnOnce(&str) -> Option<Date>,
) -> Result<Option<Date>, Error> {
    match token {
        Token::String(s) if s.is_empty() => Ok(None),
        Token::String(s) => match date(s, zone)? {
            Some(date) => Ok(Some(date)),
            None => fallback(s).map(Some).ok_or(Error),
        },
        Token::Null | Token::Undefined => Ok(None),
        _ => Err(Error),
    }
}

/// Typed property lookup: exact name first, then ordinal ignore-case.
pub fn member_index(names: &[&str], name: &str) -> Option<usize> {
    names.iter().position(|n| *n == name).or_else(|| {
        names
            .iter()
            .position(|n| text::ordinal_ignore_case(n, name))
    })
}

/// `JsonConvert.ToString(string)` quoting with the default escape handling.
pub fn quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}') => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---- dates -----------------------------------------------------------------

fn leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}
pub(crate) fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
/// Days from 0001-01-01 (proleptic Gregorian).
pub(crate) fn days_from_civil(year: i32, month: i32, day: i32) -> i64 {
    let y = i64::from(year) - 1;
    let mut days = y * 365 + y / 4 - y / 100 + y / 400;
    for m in 1..month {
        days += i64::from(days_in_month(year, m));
    }
    days + i64::from(day) - 1
}
pub(crate) fn civil_from_days(mut days: i64) -> (i32, i32, i32) {
    let mut year = (days / 146_097 * 400 + 1) as i32;
    days %= 146_097;
    let centuries = (days / 36_524).min(3);
    year += (centuries * 100) as i32;
    days -= centuries * 36_524;
    year += (days / 1461 * 4) as i32;
    days %= 1461;
    let years = (days / 365).min(3);
    year += years as i32;
    days -= years * 365;
    let mut month = 1;
    while days >= i64::from(days_in_month(year, month)) {
        days -= i64::from(days_in_month(year, month));
        month += 1;
    }
    (year, month, days as i32 + 1)
}
fn javascript_ticks(milliseconds: i64) -> Result<i64, Error> {
    let ticks = milliseconds
        .wrapping_mul(10_000)
        .wrapping_add(JAVASCRIPT_EPOCH_TICKS);
    if (0..=MAX_TICKS).contains(&ticks) {
        Ok(ticks)
    } else {
        Err(Error)
    }
}
/// `DateTime.ToLocalTime`, clamped to the representable range.
fn to_local(utc: i64, zone: &(impl LocalZone + ?Sized)) -> Date {
    let offset = zone.offset_for_utc(utc);
    let local = utc + i64::from(offset) * TICKS_PER_MINUTE;
    let clamped = local.clamp(0, MAX_TICKS);
    let offset_minutes = if clamped == local {
        offset
    } else {
        zone.offset_for_local(clamped)
    };
    Date {
        ticks: clamped,
        kind: DateKind::Local { offset_minutes },
    }
}
pub(crate) fn local_date(ticks: i64, zone: &(impl LocalZone + ?Sized)) -> Date {
    Date {
        ticks,
        kind: DateKind::Local {
            offset_minutes: zone.offset_for_local(ticks),
        },
    }
}

fn digits(chars: &[char], start: usize, count: usize) -> Option<i32> {
    let mut value = 0;
    for i in start..start + count {
        value = value * 10 + chars.get(i)?.to_digit(10).filter(|_| chars[i].is_ascii())? as i32;
    }
    Some(value)
}

/// `DateTimeUtils.TryParseDateTime`: `Ok(None)` when the string is not a
/// date, `Err` where the original throws while loading.
fn date(value: &str, zone: &(impl LocalZone + ?Sized)) -> Result<Option<Date>, Error> {
    let length = value.encode_utf16().count();
    if let Some(inner) = value
        .strip_prefix("/Date(")
        .and_then(|v| v.strip_suffix(")/"))
        .filter(|_| length >= 9)
    {
        return microsoft_date(inner, zone);
    }
    let chars: Vec<char> = value.chars().collect();
    if (19..=40).contains(&length) && chars.get(10) == Some(&'T') {
        return iso_date(&chars, zone);
    }
    Ok(None)
}

fn microsoft_date(inner: &str, zone: &(impl LocalZone + ?Sized)) -> Result<Option<Date>, Error> {
    let sign = inner
        .get(1..)
        .and_then(|rest| rest.find('+').or_else(|| rest.find('-')))
        .map(|i| i + 1);
    let ticks_text = sign.map_or(inner, |i| &inner[..i]);
    if let Some(i) = sign {
        let hours = inner[i + 1..].chars().take(2).collect::<String>();
        let valid = hours.chars().count() == 2
            && match hours.strip_prefix('-') {
                Some(rest) => rest.bytes().all(|b| b.is_ascii_digit()),
                None => hours.bytes().all(|b| b.is_ascii_digit()),
            }
            && hours != "-";
        if !valid {
            return Ok(None);
        }
    }
    let unsigned = ticks_text.strip_prefix('-').unwrap_or(ticks_text);
    if unsigned.is_empty() || !unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let Ok(milliseconds) = ticks_text.parse::<i64>() else {
        return Ok(None);
    };
    let utc = javascript_ticks(milliseconds)?;
    Ok(Some(match sign {
        Some(_) => to_local(utc, zone),
        None => Date {
            ticks: utc,
            kind: DateKind::Utc,
        },
    }))
}

/// `DateTimeParser.Parse` plus `CreateDateTime` and the zone handling of
/// `TryParseDateTimeIso` under `DateTimeZoneHandling.RoundtripKind`.
fn iso_date(c: &[char], zone: &(impl LocalZone + ?Sized)) -> Result<Option<Date>, Error> {
    let end = c.len();
    let ch = |i: usize, expected: char| c.get(i) == Some(&expected);
    let (Some(year), Some(month), Some(day)) = (digits(c, 0, 4), digits(c, 5, 2), digits(c, 8, 2))
    else {
        return Ok(None);
    };
    if year < 1
        || !ch(4, '-')
        || !(1..=12).contains(&month)
        || !ch(7, '-')
        || day < 1
        || day > days_in_month(year, month)
        || !ch(10, 'T')
    {
        return Ok(None);
    }
    let (Some(hour), Some(minute), Some(second)) =
        (digits(c, 11, 2), digits(c, 14, 2), digits(c, 17, 2))
    else {
        return Ok(None);
    };
    if hour > 24
        || !ch(13, ':')
        || minute >= 60
        || !ch(16, ':')
        || second >= 60
        || hour == 24 && (minute != 0 || second != 0)
    {
        return Ok(None);
    }
    let mut start = 19;
    let mut fraction = 0_i64;
    if ch(start, '.') {
        let mut count = 0;
        loop {
            start += 1;
            if start >= end || count >= 7 {
                break;
            }
            let Some(digit) = c[start].to_digit(10).filter(|_| c[start].is_ascii()) else {
                break;
            };
            fraction = fraction * 10 + i64::from(digit);
            count += 1;
        }
        if count == 0 {
            return Ok(None);
        }
        fraction *= 10_i64.pow(7 - count);
        if hour == 24 && fraction != 0 {
            return Ok(None);
        }
    }
    // ParseZone
    #[derive(PartialEq)]
    enum Zone {
        None,
        Utc,
        West,
        East,
    }
    let mut kind = Zone::None;
    let (mut zone_hour, mut zone_minute) = (0, 0);
    if start < end {
        if matches!(c[start], 'z' | 'Z') {
            kind = Zone::Utc;
            start += 1;
        } else {
            if start + 2 < end {
                if let Some(h) = digits(c, start + 1, 2) {
                    match c[start] {
                        '-' => {
                            kind = Zone::West;
                            zone_hour = h;
                            start += 3;
                        }
                        '+' => {
                            kind = Zone::East;
                            zone_hour = h;
                            start += 3;
                        }
                        _ => {}
                    }
                }
            }
            if start < end {
                if ch(start, ':') {
                    start += 1;
                    if start + 1 < end {
                        if let Some(m) = digits(c, start, 2) {
                            zone_minute = m;
                            start += 2;
                        }
                    }
                } else if start + 1 < end {
                    if let Some(m) = digits(c, start, 2) {
                        zone_minute = m;
                        start += 2;
                    }
                }
            }
        }
    }
    if start != end {
        return Ok(None);
    }
    let base_hour = if hour == 24 { 0 } else { hour };
    let mut ticks = days_from_civil(year, month, day) * TICKS_PER_DAY
        + i64::from(base_hour * 3600 + minute * 60 + second) * 10_000_000
        + fraction;
    if hour == 24 {
        ticks += TICKS_PER_DAY;
        if ticks > MAX_TICKS {
            return Err(Error);
        }
    }
    let offset = (i64::from(zone_hour) * 60 + i64::from(zone_minute)) * TICKS_PER_MINUTE;
    Ok(Some(match kind {
        Zone::None => Date {
            ticks,
            kind: DateKind::Unspecified,
        },
        Zone::Utc => Date {
            ticks,
            kind: DateKind::Utc,
        },
        Zone::West if ticks + offset <= MAX_TICKS => to_local(ticks + offset, zone),
        Zone::West => {
            let local = ticks + offset + i64::from(zone.offset_for_local(ticks)) * TICKS_PER_MINUTE;
            local_date(local.min(MAX_TICKS), zone)
        }
        Zone::East if ticks - offset >= 0 => to_local(ticks - offset, zone),
        Zone::East => {
            let local = ticks - offset + i64::from(zone.offset_for_local(ticks)) * TICKS_PER_MINUTE;
            local_date(local.max(0), zone)
        }
    }))
}

/// Newtonsoft's ISO round-trip format (`yyyy-MM-ddTHH:mm:ss.FFFFFFFK`).
pub fn format_date(date: &Date) -> String {
    let (year, month, day) = civil_from_days(date.ticks / TICKS_PER_DAY);
    let time = date.ticks % TICKS_PER_DAY;
    let seconds = time / 10_000_000;
    let mut out = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    let mut fraction = time % 10_000_000;
    if fraction != 0 {
        let mut width = 7;
        while fraction % 10 == 0 {
            width -= 1;
            fraction /= 10;
        }
        out.push_str(&format!(".{fraction:0width$}"));
    }
    match date.kind {
        DateKind::Unspecified => {}
        DateKind::Utc => out.push('Z'),
        DateKind::Local { offset_minutes } => {
            let sign = if offset_minutes >= 0 { '+' } else { '-' };
            let abs = offset_minutes.unsigned_abs();
            out.push_str(&format!("{sign}{:02}:{:02}", abs / 60, abs % 60));
        }
    }
    out
}

// ---- formatting --------------------------------------------------------------

/// `JsonConvert.ToString(double)`: .NET round-trip text with a decimal place.
pub fn format_double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let text = round_trip(value);
    if text.contains(['.', 'E']) {
        text
    } else {
        text + ".0"
    }
}
/// .NET Core 3.0+ `double.ToString("R")`: shortest digits, scientific when the
/// decimal point position is beyond 17 digits or before -3.
fn round_trip(value: f64) -> String {
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0");
    }
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let exponent: i32 = exponent.parse().expect("exponent digits");
    let position = exponent + 1;
    if !(-3..=17).contains(&position) {
        let (first, rest) = digits.split_at(1);
        let fraction = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let exp_sign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{first}{fraction}E{exp_sign}{:02}", exponent.abs());
    }
    let length = digits.len() as i32;
    if position <= 0 {
        format!(
            "{sign}0.{}{digits}",
            "0".repeat(position.unsigned_abs() as usize)
        )
    } else if position >= length {
        format!("{sign}{digits}{}", "0".repeat((position - length) as usize))
    } else {
        let (whole, fraction) = digits.split_at(position as usize);
        format!("{sign}{whole}.{fraction}")
    }
}

// ---- conversions (SerializeObject then DeserializeObject<T>) ----------------

/// The text a token serializes to where a reader sees a string token.
fn serialized_string(token: &Token) -> Option<String> {
    match token {
        Token::String(s) => Some(s.clone()),
        Token::Date(d) => Some(format_date(d)),
        Token::Float(f) if !f.is_finite() => Some(format_double(*f)),
        _ => None,
    }
}
/// `bool.TryParse`: trims whitespace and NULs, ordinal ignore-case.
fn parse_bool(value: &str) -> Option<bool> {
    let value = value.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if text::ordinal_ignore_case(value, "True") {
        Some(true)
    } else if text::ordinal_ignore_case(value, "False") {
        Some(false)
    } else {
        None
    }
}
/// `JsonReader.ReadAsBoolean`.
pub fn read_as_boolean(token: &Token) -> Result<Option<bool>, Error> {
    if let Some(s) = serialized_string(token) {
        return if s.is_empty() {
            Ok(None)
        } else {
            parse_bool(&s).map(Some).ok_or(Error)
        };
    }
    match token {
        Token::Null | Token::Undefined => Ok(None),
        Token::Bool(b) => Ok(Some(*b)),
        Token::Integer(i) => Ok(Some(*i != 0)),
        Token::BigInteger(_) => Ok(Some(true)),
        Token::Float(f) => Ok(Some(*f != 0.0)),
        _ => Err(Error),
    }
}
/// `JsonReader.ReadAsInt32`.
pub fn read_as_int32(token: &Token) -> Result<Option<i32>, Error> {
    if let Some(s) = serialized_string(token) {
        return if s.is_empty() {
            Ok(None)
        } else {
            IntegerFormat::INVARIANT
                .parse_i32(&s)
                .map(Some)
                .ok_or(Error)
        };
    }
    match token {
        Token::Null | Token::Undefined => Ok(None),
        Token::Integer(i) => i32::try_from(*i).map(Some).map_err(|_| Error),
        _ => Err(Error),
    }
}
/// `JsonReader.ReadAsDouble` for the non-string tokens a bag can hold.
pub fn read_as_double(token: &Token) -> Result<f64, Error> {
    match token {
        Token::Integer(i) => Ok(*i as f64),
        Token::BigInteger(s) => s.parse().map_err(|_| Error),
        Token::Float(f) if f.is_finite() => Ok(*f),
        _ => Err(Error),
    }
}
/// `JsonReader.ReadAsString` for the non-string tokens a bag can hold.
pub fn read_as_string(token: &Token) -> Result<Option<String>, Error> {
    if let Some(s) = serialized_string(token) {
        return Ok(Some(s));
    }
    match token {
        Token::Null | Token::Undefined => Ok(None),
        Token::Bool(b) => Ok(Some(b.to_string())),
        Token::Integer(i) => Ok(Some(i.to_string())),
        Token::BigInteger(s) => Ok(Some(s.clone())),
        Token::Float(f) => Ok(Some(format_double(*f))),
        _ => Err(Error),
    }
}

/// `EnumUtils.ParseEnum` over the Windows Forms `Keys` names.
pub fn parse_keys(value: &str) -> Result<i32, Error> {
    parse_enum(value, keys::KEY_NAMES)
}

/// `EnumUtils.ParseEnum`: whole-name match, numeric text, then a
/// comma-separated list of exact-or-ignore-case names OR-ed together.
pub fn parse_enum(value: &str, names: &[(&str, i32)]) -> Result<i32, Error> {
    let exact = |name: &str| names.iter().find(|(n, _)| *n == name).map(|e| e.1);
    let folded = |name: &str| {
        names
            .iter()
            .find(|(n, _)| text::ordinal_ignore_case(n, name))
            .map(|e| e.1)
    };
    if let Some(v) = exact(value) {
        return Ok(v);
    }
    let first = value.chars().find(|c| !c.is_whitespace()).ok_or(Error)?;
    if first.is_numeric() || first == '-' || first == '+' {
        // Numeric text either converts or cannot match any name.
        return IntegerFormat::INVARIANT
            .parse_i32(value.trim())
            .ok_or(Error);
    }
    let mut result = 0;
    for part in value.trim_start().split(',') {
        let part = part.trim();
        match exact(part).or_else(|| folded(part)) {
            Some(v) => result |= v,
            None => return folded(value).ok_or(Error),
        }
    }
    Ok(result)
}
/// Enum properties read with `ReadType.Read` and `EnsureType`.
fn read_enum(token: &Token) -> Result<i32, Error> {
    if let Some(s) = serialized_string(token) {
        return if matches!(token, Token::Date(_)) {
            Err(Error)
        } else {
            parse_keys(&s)
        };
    }
    match token {
        Token::Integer(i) => Ok(*i as i32),
        Token::Bool(b) => Ok(i32::from(*b)),
        // Captured: any floating-point value becomes the zero enum value.
        Token::Float(_) => Ok(0),
        _ => Err(Error),
    }
}
/// `Convert.ToUInt32(double)`: banker's rounding with range checks.
fn double_to_u32(value: f64) -> Result<u32, Error> {
    if !(-0.5..4_294_967_295.5).contains(&value) {
        return Err(Error);
    }
    let truncated = value as u32;
    let difference = value - f64::from(truncated);
    Ok(
        if difference > 0.5 || difference == 0.5 && truncated & 1 != 0 {
            truncated + 1
        } else {
            truncated
        },
    )
}
/// uint fields read with `ReadType.Read` and `Convert.ChangeType`.
fn read_u32(token: &Token) -> Result<u32, Error> {
    if let Some(s) = serialized_string(token) {
        return if matches!(token, Token::Date(_)) {
            Err(Error)
        } else {
            IntegerFormat::INVARIANT.parse_u32(&s).ok_or(Error)
        };
    }
    match token {
        Token::Integer(i) => u32::try_from(*i).map_err(|_| Error),
        Token::Float(f) => double_to_u32(*f),
        Token::Bool(b) => Ok(u32::from(*b)),
        _ => Err(Error),
    }
}
fn int_field(token: &Token) -> Result<i32, Error> {
    read_as_int32(token)?.ok_or(Error)
}
/// Members match by exact name first, then ordinal ignore-case; later
/// duplicates overwrite earlier values and unknown members are skipped.
fn populate(
    token: &Token,
    names: &[&str],
    mut member: impl FnMut(usize, &Token) -> Result<(), Error>,
) -> Result<(), Error> {
    let Token::Object(members) = token else {
        return Err(Error);
    };
    for (name, value) in members {
        let index = names.iter().position(|n| n == name).or_else(|| {
            names
                .iter()
                .position(|n| text::ordinal_ignore_case(n, name))
        });
        if let Some(i) = index {
            member(i, value)?;
        }
    }
    Ok(())
}
pub fn to_hotkey(token: &Token) -> Result<HotkeyData, Error> {
    let mut value = HotkeyData::default();
    populate(token, &["Modifiers", "Key", "IsEmpty"], |i, v| {
        match i {
            0 => value.modifiers = read_enum(v)?,
            1 => value.key = read_enum(v)?,
            // Read-only: validated by ReadAsBoolean, then discarded.
            _ => {
                read_as_boolean(v)?;
            }
        }
        Ok(())
    })?;
    Ok(value)
}
fn to_point(token: &Token) -> Result<Point, Error> {
    let mut point = Point::default();
    populate(token, &["x", "y"], |i, v| {
        *[&mut point.x, &mut point.y][i] = int_field(v)?;
        Ok(())
    })?;
    Ok(point)
}
fn to_rect(token: &Token) -> Result<Rect, Error> {
    let mut rect = Rect::default();
    populate(token, &["Left", "Top", "Right", "Bottom"], |i, v| {
        *[
            &mut rect.left,
            &mut rect.top,
            &mut rect.right,
            &mut rect.bottom,
        ][i] = int_field(v)?;
        Ok(())
    })?;
    Ok(rect)
}
pub fn to_placement(token: &Token) -> Result<WindowPlacement, Error> {
    let mut value = WindowPlacement::default();
    let names = [
        "length",
        "flags",
        "showCmd",
        "ptMinPosition",
        "ptMaxPosition",
        "rcNormalPosition",
    ];
    populate(token, &names, |i, v| {
        match i {
            0 => value.length = read_u32(v)?,
            1 => value.flags = read_u32(v)?,
            2 => value.show_cmd = read_u32(v)?,
            3 => value.pt_min_position = to_point(v)?,
            4 => value.pt_max_position = to_point(v)?,
            _ => value.rc_normal_position = to_rect(v)?,
        }
        Ok(())
    })?;
    Ok(value)
}
pub fn to_u32(token: &Token) -> Result<u32, Error> {
    read_u32(token)
}

/// `ReadAsBoolean` in typed mode: no date parsing; NaN/Infinity literals are
/// rejected, other numbers are true when non-zero.
pub fn typed_bool(token: &Token) -> Result<Option<bool>, Error> {
    match token {
        Token::String(s) if s.is_empty() => Ok(None),
        Token::String(s) => parse_bool(s).map(Some).ok_or(Error),
        Token::RawNumber(n) if matches!(n.as_str(), "NaN" | "Infinity" | "-Infinity") => Err(Error),
        Token::RawNumber(n) => match classify(n)? {
            Token::Integer(i) => Ok(Some(i != 0)),
            Token::BigInteger(_) => Ok(Some(true)),
            Token::Float(f) => Ok(Some(f != 0.0)),
            _ => Err(Error),
        },
        Token::Bool(b) => Ok(Some(*b)),
        Token::Null | Token::Undefined => Ok(None),
        _ => Err(Error),
    }
}

/// An enum property in typed mode (`ReadType.Read` then `EnsureType`).
pub fn typed_enum(
    token: &Token,
    names: &[(&str, i32)],
    zone: &(impl LocalZone + ?Sized),
) -> Result<i32, Error> {
    match token {
        Token::String(s) => match date(s, zone)? {
            Some(_) => Err(Error),
            None => parse_enum(s, names),
        },
        Token::RawNumber(n) => match classify(n)? {
            Token::Integer(i) => Ok(i as i32),
            Token::Float(_) => Ok(0),
            _ => Err(Error),
        },
        Token::Bool(b) => Ok(i32::from(*b)),
        _ => Err(Error),
    }
}

/// Windows Forms Keys property conversion in typed Json.NET records.
pub fn typed_keys(token: &Token, zone: &(impl LocalZone + ?Sized)) -> Result<i32, Error> {
    typed_enum(token, keys::KEY_NAMES, zone)
}
