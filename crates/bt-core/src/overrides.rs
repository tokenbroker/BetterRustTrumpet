//! Editable Rust-owned settings layered over an immutable migration archive.
//! Unknown JSON fragments remain RawValue, avoiding float/text normalization.
use crate::{
    legacy_store::{self, PortableSnapshot},
    legacy_xml::{Kind, Value},
    registry_snapshot::RegistrySnapshot,
    registry_values::{self, Environment, Read, Request},
};
use serde_json::value::{RawValue, to_raw_value};
use std::collections::{BTreeMap, HashSet};
use thiserror::Error;

pub const FORMAT: &str = "BetterTrumpet.Rust.Settings";
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(transparent)]
struct Map(BTreeMap<String, Box<RawValue>>);
impl Map {
    fn new() -> Self {
        Self::default()
    }
}
impl std::ops::Deref for Map {
    type Target = BTreeMap<String, Box<RawValue>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Map {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl<'de> serde::Deserialize<'de> for Map {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Map;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an editable object with unique fields")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<Map, A::Error> {
                let mut map = Map::new();
                while let Some((key, value)) = a.next_entry::<String, Box<RawValue>>()? {
                    if map.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate editable JSON field"));
                    }
                }
                Ok(map)
            }
        }
        d.deserialize_map(Visitor)
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid editable settings JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid editable settings document")]
    InvalidDocument,
    #[error("unsupported editable settings format or version")]
    UnsupportedVersion,
    #[error("unsupported editable value encoding")]
    UnsupportedEncoding,
    #[error(transparent)]
    Expansion(#[from] registry_values::ExpansionError),
}

#[derive(Clone, Debug)]
struct Entry {
    key: Vec<u16>,
    fields: Map,
}
#[derive(Clone, Debug)]
pub struct Overrides {
    fields: Map,
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodedValue {
    Text(Vec<u16>),
    NativeBool(bool),
}

fn required<'a>(map: &'a Map, key: &str) -> Result<&'a RawValue, Error> {
    map.get(key)
        .map(AsRef::as_ref)
        .ok_or(Error::InvalidDocument)
}
fn payload(entry: &Entry) -> Result<Map, Error> {
    Ok(serde_json::from_str(
        required(&entry.fields, "value")?.get(),
    )?)
}
fn encoding(map: &Map) -> Result<String, Error> {
    Ok(serde_json::from_str(required(map, "encoding")?.get())?)
}
impl Default for Overrides {
    fn default() -> Self {
        Self::new()
    }
}
impl Overrides {
    pub fn new() -> Self {
        let mut fields = Map::new();
        fields.insert(
            "format".into(),
            to_raw_value(FORMAT).expect("format string"),
        );
        fields.insert(
            "version".into(),
            to_raw_value(&1u32).expect("version integer"),
        );
        Self {
            fields,
            entries: vec![],
        }
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut fields: Map = serde_json::from_slice(bytes)?;
        let format: String = serde_json::from_str(required(&fields, "format")?.get())?;
        let version: u32 = serde_json::from_str(required(&fields, "version")?.get())?;
        if format != FORMAT || version != 1 {
            return Err(Error::UnsupportedVersion);
        }
        let raw = fields.remove("entries").ok_or(Error::InvalidDocument)?;
        let entries: Vec<Map> = serde_json::from_str(raw.get())?;
        let mut seen = HashSet::new();
        let entries = entries
            .into_iter()
            .map(|fields| {
                let key: Vec<u16> = serde_json::from_str(required(&fields, "key")?.get())?;
                required(&fields, "value")?;
                if !seen.insert(key.clone()) {
                    return Err(Error::InvalidDocument);
                }
                Ok(Entry { key, fields })
            })
            .collect::<Result<_, Error>>()?;
        Ok(Self { fields, entries })
    }
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut fields = self.fields.clone();
        fields.insert(
            "entries".into(),
            to_raw_value(&self.entries.iter().map(|e| &e.fields).collect::<Vec<_>>())?,
        );
        Ok(serde_json::to_vec_pretty(&fields)?)
    }
    pub fn get(
        &self,
        key: &[u16],
        equals: impl Fn(&[u16], &[u16]) -> bool,
    ) -> Result<Option<EncodedValue>, Error> {
        let Some(entry) = self.entries.iter().find(|e| equals(&e.key, key)) else {
            return Ok(None);
        };
        let value = payload(entry)?;
        let data = required(&value, "data")?;
        Ok(Some(match encoding(&value)?.as_str() {
            "legacy-string" => EncodedValue::Text(serde_json::from_str(data.get())?),
            "native-bool" => EncodedValue::NativeBool(serde_json::from_str(data.get())?),
            _ => return Err(Error::UnsupportedEncoding),
        }))
    }
    pub fn contains(&self, key: &[u16], equals: impl Fn(&[u16], &[u16]) -> bool) -> bool {
        self.entries.iter().any(|e| equals(&e.key, key))
    }
    pub fn set(
        &mut self,
        key: &[u16],
        value: EncodedValue,
        equals: impl Fn(&[u16], &[u16]) -> bool,
    ) -> Result<(), Error> {
        let index = self.entries.iter().position(|e| equals(&e.key, key));
        let mut entry = match index {
            Some(i) => self.entries[i].clone(),
            None => {
                let mut fields = Map::new();
                fields.insert("key".into(), to_raw_value(key)?);
                Entry {
                    key: key.to_vec(),
                    fields,
                }
            }
        };
        let mut stored = if entry.fields.contains_key("value") {
            let stored = payload(&entry)?;
            if !matches!(encoding(&stored)?.as_str(), "legacy-string" | "native-bool") {
                return Err(Error::UnsupportedEncoding);
            }
            stored
        } else {
            Map::new()
        };
        let (tag, data) = match value {
            EncodedValue::Text(units) => ("legacy-string", to_raw_value(&units)?),
            EncodedValue::NativeBool(value) => ("native-bool", to_raw_value(&value)?),
        };
        stored.insert("encoding".into(), to_raw_value(tag)?);
        stored.insert("data".into(), data);
        entry.fields.insert("value".into(), to_raw_value(&stored)?);
        match index {
            Some(i) => self.entries[i] = entry,
            None => self.entries.push(entry),
        };
        Ok(())
    }
    pub fn remove_override(&mut self, key: &[u16], equals: impl Fn(&[u16], &[u16]) -> bool) {
        self.entries.retain(|e| !equals(&e.key, key));
    }
    pub fn registry_read(
        &self,
        archive: &RegistrySnapshot,
        key: Option<&str>,
        request: Request,
        env: &dyn Environment,
    ) -> Result<Read, Error> {
        let query: Vec<_> = key
            .unwrap_or_default()
            .encode_utf16()
            .take_while(|c| *c != 0)
            .collect();
        match self.get(&query, |a, b| env.names_equal(a, b))? {
            Some(EncodedValue::Text(units)) => Ok(registry_values::read_text(&units, request)),
            Some(EncodedValue::NativeBool(_)) => Err(Error::UnsupportedEncoding),
            None => Ok(archive.read(key, request, env)),
        }
    }
    pub fn registry_has_key(
        &self,
        archive: &RegistrySnapshot,
        key: Option<&str>,
        env: &dyn Environment,
    ) -> Result<bool, Error> {
        let query: Vec<_> = key
            .unwrap_or_default()
            .encode_utf16()
            .take_while(|c| *c != 0)
            .collect();
        match self.get(&query, |a, b| env.names_equal(a, b))? {
            Some(EncodedValue::Text(_)) => Ok(true),
            Some(EncodedValue::NativeBool(_)) => Err(Error::UnsupportedEncoding),
            None => Ok(archive.has_key(key, env)?),
        }
    }
    /// The caller supplies this session's portable dictionary comparer. The
    /// original switches from ignore-case to case-sensitive on successful Load.
    pub fn portable_read(
        &self,
        archive: &PortableSnapshot,
        key: &str,
        request: Request,
        equals: impl Fn(&[u16], &[u16]) -> bool,
    ) -> Result<Read, Error> {
        let key_units: Vec<_> = key.encode_utf16().collect();
        match self.get(&key_units, equals)? {
            Some(EncodedValue::Text(units)) => Ok(registry_values::read_text(
                &units,
                match request {
                    Request::NullDefaultString => Request::Value(Kind::String),
                    other => other,
                },
            )),
            Some(EncodedValue::NativeBool(_)) => Err(Error::UnsupportedEncoding),
            None => {
                let kind = match request {
                    Request::NullDefaultString => Kind::String,
                    Request::Value(kind) => kind,
                };
                Ok(match archive.store().read(key, kind) {
                    legacy_store::Read::Default => Read::Default,
                    legacy_store::Read::Throws => Read::Throws,
                    legacy_store::Read::Value(Value::String(Some(s))) => {
                        Read::StringUtf16(s.encode_utf16().collect())
                    }
                    legacy_store::Read::Value(v) => Read::Value(v),
                })
            }
        }
    }
    pub fn portable_has_key(
        &self,
        archive: &PortableSnapshot,
        key: &str,
        equals: impl Fn(&[u16], &[u16]) -> bool,
    ) -> bool {
        self.contains(&key.encode_utf16().collect::<Vec<_>>(), equals)
            || archive.store().has_key(key)
    }
}
