//! What the original settings bags return from `Get<T>(key, default)` for a
//! raw stored value. Reading here never opens HKCU, package storage or files:
//! store adapters hand over raw values and keep them for recovery.
//!
//! Registry and package bags let XML errors escape `Get` ([`Read::Throws`]);
//! the portable JSON bag catches every error and returns the default.
use crate::{
    legacy_xml::{self, Kind, Value},
    newtonsoft::{self, LocalZone, Token},
    text::is_blank,
};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    Value(Value),
    /// The caller's default value.
    Default,
    /// The original getter throws (`InvalidOperationException` from XML).
    Throws,
}

/// `RegistryKey.GetValue` results. REG_EXPAND_SZ arrives already expanded.
#[derive(Clone, Debug, PartialEq)]
pub enum RegistryValue {
    String(String),
    DWord(u32),
    QWord(u64),
    Binary(Vec<u8>),
    MultiString(Vec<String>),
}

/// Boxed WinRT values in `ApplicationData.LocalSettings`; only booleans and
/// strings are ever cast by the original.
#[derive(Clone, Debug, PartialEq)]
pub enum PackageValue {
    Bool(bool),
    String(String),
    Other,
}

fn xml(text: &str, kind: Kind) -> Read {
    legacy_xml::decode(text, kind).map_or(Read::Throws, Read::Value)
}

/// `RegistrySettingsBag.Get`: strings are cast directly, everything else is
/// XML from a REG_SZ value; non-string registry types read as the default.
pub fn read_registry(raw: Option<&RegistryValue>, kind: Kind) -> Read {
    let text = match raw {
        Some(RegistryValue::String(text)) => Some(text.as_str()),
        _ => None,
    };
    match (kind, text) {
        (Kind::String, Some(text)) => Read::Value(Value::String(Some(text.to_owned()))),
        (Kind::String, None) => Read::Default,
        (_, Some(text)) if !is_blank(Some(text)) => xml(text, kind),
        _ => Read::Default,
    }
}

/// `WindowsStorageSettingsBag.Get`: booleans and strings are native values,
/// other types are XML strings.
pub fn read_package(raw: Option<&PackageValue>, kind: Kind) -> Read {
    match (kind, raw) {
        (_, None) => Read::Default,
        (Kind::Bool, Some(PackageValue::Bool(b))) => Read::Value(Value::Bool(*b)),
        (Kind::String, Some(PackageValue::String(s))) => {
            Read::Value(Value::String(Some(s.clone())))
        }
        (Kind::Bool | Kind::String, _) => Read::Default,
        (_, Some(PackageValue::String(text))) if !is_blank(Some(text)) => xml(text, kind),
        _ => Read::Default,
    }
}

/// `NamespacedSettingsBag` key mapping for addon settings.
pub fn namespaced_key(namespace: &str, key: &str) -> String {
    format!("{namespace}.{key}")
}

/// `File.ReadAllText` decoding: byte-order-mark detection, otherwise UTF-8
/// with replacement characters.
pub fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |bytes: &[u8], little: bool| {
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|c| match (c, little) {
                ([a, b], true) => u16::from_le_bytes([*a, *b]),
                ([a, b], false) => u16::from_be_bytes([*a, *b]),
                _ => 0xfffd,
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    let utf32 = |bytes: &[u8], little: bool| {
        bytes
            .chunks(4)
            .map(|c| match c {
                [a, b, c, d] => {
                    let raw = [*a, *b, *c, *d];
                    let code = if little {
                        u32::from_le_bytes(raw)
                    } else {
                        u32::from_be_bytes(raw)
                    };
                    char::from_u32(code).unwrap_or('\u{fffd}')
                }
                _ => '\u{fffd}',
            })
            .collect()
    };
    match bytes {
        [0xff, 0xfe, 0, 0, rest @ ..] => utf32(rest, true),
        [0, 0, 0xfe, 0xff, rest @ ..] => utf32(rest, false),
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0xff, 0xfe, rest @ ..] => utf16(rest, true),
        [0xfe, 0xff, rest @ ..] => utf16(rest, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// The portable `config/settings.json` bag after `Load`. Keys are
/// case-sensitive (Newtonsoft's dictionary replaces the constructor's
/// ignore-case one) and the last duplicate wins. Any load error yields an
/// empty bag, as the original catches it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PortableStore {
    values: HashMap<String, Token>,
}

impl PortableStore {
    pub fn parse(text: &str, zone: &(impl LocalZone + ?Sized)) -> Self {
        let values = newtonsoft::parse_object_document(text, zone)
            .ok()
            .flatten()
            .unwrap_or_default()
            .into_iter()
            .collect();
        Self { values }
    }
    /// `bytes` is `None` when the file does not exist.
    pub fn load(bytes: Option<&[u8]>, zone: &(impl LocalZone + ?Sized)) -> Self {
        bytes.map_or_else(Self::default, |b| Self::parse(&decode_text(b), zone))
    }
    pub fn has_key(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }
    pub fn raw(&self, key: &str) -> Option<&Token> {
        self.values.get(key)
    }
    /// `JsonFileSettingsBag.Get`.
    pub fn read(&self, key: &str, kind: Kind) -> Read {
        let Some(raw) = self.values.get(key) else {
            return Read::Default;
        };
        let value = match (kind, raw) {
            (_, Token::Null | Token::Undefined) => return Read::Default,
            (Kind::Bool, Token::Bool(b)) => Ok(Value::Bool(*b)),
            (Kind::I32, Token::Integer(i)) => Ok(Value::I32(*i as i32)),
            (Kind::F64, Token::Float(f)) => Ok(Value::F64(*f)),
            (Kind::F32, Token::Float(f)) => Ok(Value::F32(*f as f32)),
            (Kind::String, Token::String(s)) => Ok(Value::String(Some(s.clone()))),
            (_, Token::String(s)) => legacy_xml::decode(s, kind).map_err(|_| newtonsoft::Error),
            (_, token) => convert(token, kind),
        };
        value.map_or(Read::Default, Read::Value)
    }
}

/// The "last resort" `DeserializeObject<T>(SerializeObject(raw))`.
fn convert(token: &Token, kind: Kind) -> Result<Value, newtonsoft::Error> {
    Ok(match kind {
        Kind::Bool => Value::Bool(newtonsoft::read_as_boolean(token)?.ok_or(newtonsoft::Error)?),
        Kind::I32 => Value::I32(newtonsoft::read_as_int32(token)?.ok_or(newtonsoft::Error)?),
        Kind::U32 => Value::U32(newtonsoft::to_u32(token)?),
        Kind::F64 => Value::F64(newtonsoft::read_as_double(token)?),
        Kind::F32 => Value::F32(newtonsoft::read_as_double(token)? as f32),
        Kind::String => Value::String(newtonsoft::read_as_string(token)?),
        Kind::Hotkey => Value::Hotkey(Some(newtonsoft::to_hotkey(token)?)),
        Kind::Placement | Kind::OptionalPlacement => {
            Value::Placement(Some(newtonsoft::to_placement(token)?))
        }
    })
}
