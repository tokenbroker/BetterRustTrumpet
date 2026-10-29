//! .NET 8 RegistryKey.GetValue projection from lossless NT registry snapshots.
//! Keep UTF-16 strings intact; native casing/expansion are explicit boundaries.
use crate::{
    legacy_xml::{self, Kind, Value},
    registry_snapshot::{RawRegistryValue, RegistrySnapshot},
    text,
};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("registry environment expansion failed")]
pub struct ExpansionError;

pub trait Environment {
    fn names_equal(&self, left: &[u16], right: &[u16]) -> bool;
    fn expand(&self, text: &[u16]) -> Result<Vec<u16>, ExpansionError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Object {
    String(Vec<u16>),
    DWord(u32),
    QWord(u64),
    Binary(Vec<u8>),
    MultiString(Vec<Vec<u16>>),
}

#[derive(Clone, Copy, Debug)]
pub enum Request {
    Value(Kind),
    /// RegistrySettingsBag tests the runtime default value, so a null string
    /// default takes the XML path instead of returning a stored string directly.
    NullDefaultString,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Read {
    Value(Value),
    StringUtf16(Vec<u16>),
    Default,
    Throws,
}

fn units(bytes: &[u8]) -> Vec<u16> {
    let mut units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    if bytes.len() % 2 == 1 {
        // Single-byte and one-byte-short-of-buffer cases survive Win32's
        // normalization. .NET starts with 512 bytes, then uses pooled
        // power-of-two buffers. Captured at 511/513 and 1023/1025 bytes.
        let preserves = bytes.len() == 1
            || (bytes.len() >= 511 && bytes.len().wrapping_add(1).is_power_of_two());
        units.push(if preserves {
            u16::from(bytes[bytes.len() - 1])
        } else {
            0
        });
    }
    units
}

pub fn project(
    raw: &RawRegistryValue,
    expand: bool,
    env: &dyn Environment,
) -> Result<Option<Object>, ExpansionError> {
    Ok(Some(match raw.kind {
        0 | 3 | 5 => Object::Binary(raw.bytes.clone()),
        4 | 11 => match raw.bytes.as_slice() {
            bytes if bytes.len() == 4 => {
                Object::DWord(u32::from_le_bytes(bytes.try_into().unwrap()))
            }
            bytes if bytes.len() == 8 => {
                Object::QWord(u64::from_le_bytes(bytes.try_into().unwrap()))
            }
            _ => Object::Binary(raw.bytes.clone()),
        },
        1 | 2 => {
            let mut text = units(&raw.bytes);
            if text.last() == Some(&0) {
                text.pop();
            }
            Object::String(if raw.kind == 2 && expand {
                env.expand(&text)?
            } else {
                text
            })
        }
        7 => {
            let text = units(&raw.bytes);
            let mut rest = text.as_slice();
            let mut strings = Vec::new();
            while rest.len() > 1 || (rest.len() == 1 && rest[0] != 0) {
                if let Some(end) = rest.iter().position(|c| *c == 0) {
                    strings.push(rest[..end].to_vec());
                    rest = &rest[end + 1..];
                } else {
                    strings.push(rest.to_vec());
                    break;
                }
            }
            Object::MultiString(strings)
        }
        _ => return Ok(None),
    }))
}

impl RegistrySnapshot {
    pub fn find_value(
        &self,
        name: Option<&str>,
        env: &dyn Environment,
    ) -> Option<&RawRegistryValue> {
        if !self.key_existed {
            return None;
        }
        let query: Vec<_> = name
            .unwrap_or_default()
            .encode_utf16()
            .take_while(|c| *c != 0)
            .collect();
        self.values
            .iter()
            .find(|v| env.names_equal(&v.name_utf16, &query))
    }
    /// Original HasKey calls GetValue, so an existing unsupported type can
    /// still be false; expansion failures escape HasKey rather than defaulting.
    pub fn has_key(
        &self,
        name: Option<&str>,
        env: &dyn Environment,
    ) -> Result<bool, ExpansionError> {
        self.find_value(name, env)
            .map_or(Ok(false), |v| project(v, true, env).map(|v| v.is_some()))
    }
    pub fn read(&self, name: Option<&str>, request: Request, env: &dyn Environment) -> Read {
        let Some(raw) = self.find_value(name, env) else {
            return Read::Default;
        };
        // ReadSetting<T> catches native errors/type-cast failures. XML errors
        // happen after that helper returns, so those escape the original getter.
        let Ok(Some(Object::String(units))) = project(raw, true, env) else {
            return Read::Default;
        };
        read_text(&units, request)
    }
}

/// Read an already stored string using the registry bag's runtime-default route.
pub fn read_text(units: &[u16], request: Request) -> Read {
    if matches!(request, Request::Value(Kind::String)) {
        return Read::StringUtf16(units.to_vec());
    }
    // Invalid surrogate units are illegal XML chars. Use an illegal XML
    // sentinel rather than U+FFFD, so the streaming reader fails only if
    // it actually reaches them (it can stop before unread trailing data).
    let xml: String = char::decode_utf16(units.iter().copied())
        .map(|c| c.unwrap_or('\u{1}'))
        .collect();
    if text::is_blank(Some(&xml)) {
        return Read::Default;
    }
    let kind = match request {
        Request::Value(kind) => kind,
        Request::NullDefaultString => Kind::String,
    };
    legacy_xml::decode(&xml, kind).map_or(Read::Throws, Read::Value)
}
