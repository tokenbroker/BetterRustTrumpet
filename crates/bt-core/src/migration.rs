//! Pure migration archive format. File discovery and persistence live in
//! bt-storage so the portable models acquire no OS adapter dependencies.
use crate::{legacy_store::PortableSnapshot, newtonsoft::LocalZone};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PORTABLE_FORMAT: &str = "BetterTrumpet.Rust.PortableMigration";
pub const REGISTRY_FORMAT: &str = "BetterTrumpet.Rust.RegistryMigration";
const VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid migration archive: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported migration archive format or version")]
    UnsupportedArchive,
    #[error("invalid raw registry snapshot")]
    InvalidRegistrySnapshot,
}

#[derive(Serialize)]
struct ArchiveRef<'a> {
    format: &'a str,
    version: u32,
    original_bytes: Option<&'a [u8]>,
}
#[derive(Deserialize)]
struct Archive {
    format: String,
    version: u32,
    // This field must be present even when its value is null. A truncated or
    // hand-edited archive cannot masquerade as a missing legacy source file.
    #[serde(deserialize_with = "required_bytes")]
    original_bytes: Option<Vec<u8>>,
}
fn required_bytes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
    Option::deserialize(d)
}

pub fn encode_portable(bytes: Option<&[u8]>) -> Result<Vec<u8>, Error> {
    Ok(serde_json::to_vec_pretty(&ArchiveRef {
        format: PORTABLE_FORMAT,
        version: VERSION,
        original_bytes: bytes,
    })?)
}

pub fn decode_portable(bytes: &[u8], zone: &dyn LocalZone) -> Result<PortableSnapshot, Error> {
    let archive: Archive = serde_json::from_slice(bytes)?;
    if archive.format != PORTABLE_FORMAT || archive.version != VERSION {
        return Err(Error::UnsupportedArchive);
    }
    Ok(PortableSnapshot::capture(archive.original_bytes, zone))
}

#[derive(Deserialize)]
struct RegistryArchive {
    format: String,
    version: u32,
    snapshot: crate::registry_snapshot::RegistrySnapshot,
}

#[derive(Serialize)]
struct RegistryArchiveRef<'a> {
    format: &'a str,
    version: u32,
    snapshot: &'a crate::registry_snapshot::RegistrySnapshot,
}

pub fn encode_registry(
    snapshot: &crate::registry_snapshot::RegistrySnapshot,
) -> Result<Vec<u8>, Error> {
    if !snapshot.is_valid() {
        return Err(Error::InvalidRegistrySnapshot);
    }
    Ok(serde_json::to_vec_pretty(&RegistryArchiveRef {
        format: REGISTRY_FORMAT,
        version: VERSION,
        snapshot,
    })?)
}

pub fn decode_registry(bytes: &[u8]) -> Result<crate::registry_snapshot::RegistrySnapshot, Error> {
    let archive: RegistryArchive = serde_json::from_slice(bytes)?;
    if archive.format != REGISTRY_FORMAT || archive.version != VERSION {
        return Err(Error::UnsupportedArchive);
    }
    if !archive.snapshot.is_valid() {
        return Err(Error::InvalidRegistrySnapshot);
    }
    Ok(archive.snapshot)
}
