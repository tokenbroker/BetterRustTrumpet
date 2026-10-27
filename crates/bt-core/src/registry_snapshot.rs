//! Lossless registry migration input, before .NET RegistryKey.GetValue string
//! decoding/environment expansion. Unknown types and malformed UTF-16 survive.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawRegistryValue {
    /// UTF-16 code units, excluding the terminating NUL. Empty is the default value.
    pub name_utf16: Vec<u16>,
    pub kind: u32,
    /// Exact NtQueryValueKey data bytes, including any string terminators.
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrySnapshot {
    pub key_existed: bool,
    pub values: Vec<RawRegistryValue>,
}

impl RegistrySnapshot {
    pub fn is_valid(&self) -> bool {
        // Missing keys cannot have values. Reject exact duplicate names, but
        // retain every UTF-16 unit (even NUL/unpaired surrogates) for recovery.
        if !self.key_existed && !self.values.is_empty() {
            return false;
        }
        let mut seen = std::collections::HashSet::new();
        self.values.iter().all(|v| seen.insert(&v.name_utf16))
    }
}
