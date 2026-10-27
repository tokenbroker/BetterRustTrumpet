//! Packaged (MSIX) LocalSettings migration input and the original
//! WindowsStorageSettingsBag read semantics over it.
//!
//! The original only ever writes booleans and strings. Values of any other
//! WinRT type (written by other builds or tools) are preserved by type and
//! read as the caller's default, exactly as the original's casts do.
use crate::{
    legacy_store::{self, PackageValue, Read},
    legacy_xml::Kind,
    text,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RawPackageValue {
    Bool {
        value: bool,
    },
    /// UTF-16 code units, unpaired surrogates included.
    String {
        utf16: Vec<u16>,
    },
    /// `Windows.Foundation.PropertyType` of a non-bool/string value, or
    /// `None` for values that are not property values (composite values).
    Other {
        property_type: Option<i32>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawPackageEntry {
    pub key_utf16: Vec<u16>,
    pub value: RawPackageValue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSnapshot {
    /// Package family whose LocalSettings were captured.
    pub family: String,
    /// False when the family is not installed for this user.
    pub container_existed: bool,
    pub entries: Vec<RawPackageEntry>,
}

/// LocalSettings keys are matched case-insensitively (verified read-only on
/// installed packages); ordinal ignore-case is the working comparer.
fn same_key(left: &[u16], right: &str) -> bool {
    text::ordinal_ignore_case(&String::from_utf16_lossy(left), right)
}

impl PackageSnapshot {
    pub fn is_valid(&self) -> bool {
        if !self.container_existed && !self.entries.is_empty() {
            return false;
        }
        // The container already guarantees case-insensitive uniqueness; reject
        // only exact duplicates so unusual (e.g. unpaired-surrogate) keys
        // never make a genuine capture fail.
        let mut seen = std::collections::HashSet::new();
        self.entries.iter().all(|e| seen.insert(&e.key_utf16))
    }
    fn find(&self, key: &str) -> Option<&RawPackageEntry> {
        self.entries.iter().find(|e| same_key(&e.key_utf16, key))
    }
    /// `WindowsStorageSettingsBag.HasKey`.
    pub fn has_key(&self, key: &str) -> bool {
        self.find(key).is_some()
    }
    /// `WindowsStorageSettingsBag.Get<T>`.
    pub fn read(&self, key: &str, kind: Kind) -> Read {
        let raw = self.find(key).map(|e| match &e.value {
            RawPackageValue::Bool { value } => PackageValue::Bool(*value),
            RawPackageValue::String { utf16 } => {
                PackageValue::String(String::from_utf16_lossy(utf16))
            }
            RawPackageValue::Other { .. } => PackageValue::Other,
        });
        legacy_store::read_package(raw.as_ref(), kind)
    }
}
