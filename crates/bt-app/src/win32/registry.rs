//! Read-only HKCU snapshotting. No production API creates or writes a key.
use bt_core::{
    legacy_store::REGISTRY_SETTINGS_SUBKEY,
    registry_snapshot::{RawRegistryValue, RegistrySnapshot},
};
use std::{io, path::Path};
use windows::{
    Wdk::System::Registry::{KeyValuePartialInformation, NtQueryValueKey},
    Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND,
            ERROR_SUCCESS, FILETIME, HANDLE, UNICODE_STRING, WIN32_ERROR,
        },
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, RegCloseKey, RegEnumValueW, RegOpenKeyExW,
            RegQueryInfoKeyW,
        },
    },
    core::{PCWSTR, PWSTR},
};

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: Key uniquely owns a successful RegOpenKeyExW handle. No
        // borrowed handle outlives this object; predefined handles are not owned.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
fn error(status: WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(status.0 as i32)
}
fn changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "registry source changed during capture; retry migration",
    )
}
fn open(subkey: &str) -> io::Result<Option<Key>> {
    if subkey.is_empty() || subkey.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty registry path or embedded NUL",
        ));
    }
    let path: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    let mut key = HKEY::default();
    // SAFETY: path is NUL-terminated and live for this synchronous call; key is
    // a valid output pointer. Only KEY_QUERY_VALUE is requested, never write/create.
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(Some(Key(key))),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
        status => Err(error(status)),
    }
}

#[derive(PartialEq, Eq)]
struct Metadata {
    count: u32,
    last_write: u64,
}
fn metadata(key: &Key) -> io::Result<Metadata> {
    let mut count = 0;
    let mut time = FILETIME::default();
    // SAFETY: the owned query handle is live; count/time are valid output
    // pointers. All omitted outputs are null; no registry mutation occurs.
    let status = unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&mut count),
            None,
            None,
            None,
            Some(&mut time),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(error(status));
    }
    Ok(Metadata {
        count,
        last_write: u64::from(time.dwLowDateTime) | (u64::from(time.dwHighDateTime) << 32),
    })
}
fn grow<T: Clone>(buffer: &mut Vec<T>, required: usize, zero: T) -> io::Result<()> {
    let size = buffer
        .len()
        .checked_mul(2)
        .map(|n| n.max(required))
        .filter(|n| *n <= u32::MAX as usize)
        .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "registry buffer too large"))?;
    buffer.try_reserve_exact(size - buffer.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "cannot allocate registry buffer",
        )
    })?;
    buffer.resize(size, zero);
    Ok(())
}
fn raw_value(key: &Key, name: &[u16]) -> io::Result<RawRegistryValue> {
    let length = u16::try_from(name.len() * 2)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "registry name too long"))?;
    let mut units = name.to_vec();
    let name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: if units.is_empty() {
            PWSTR::null()
        } else {
            PWSTR(units.as_mut_ptr())
        },
    };
    // KEY_VALUE_PARTIAL_INFORMATION has a 12-byte header of three ULONGs,
    // followed by raw Data bytes. Vec<u32> provides the required alignment.
    let mut words = vec![0u32; 64];
    for _ in 0..16 {
        let size = u32::try_from(words.len() * 4)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "registry value too large"))?;
        let mut needed = 0;
        // SAFETY: the query handle and counted UTF-16 name are live. words is
        // a writable ULONG-aligned buffer of exactly size bytes; needed is a
        // valid output pointer. NtQueryValueKey never mutates the registry.
        let status = unsafe {
            NtQueryValueKey(
                HANDLE(key.0.0),
                &name,
                KeyValuePartialInformation,
                Some(words.as_mut_ptr().cast()),
                size,
                &mut needed,
            )
        };
        match status.0 as u32 {
            0 => {
                if needed < 12 || needed > size || words[2] > needed - 12 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid registry value length",
                    ));
                }
                let bytes = words[3..]
                    .iter()
                    .flat_map(|word| word.to_le_bytes())
                    .take(words[2] as usize)
                    .collect();
                return Ok(RawRegistryValue {
                    name_utf16: units,
                    kind: words[1],
                    bytes,
                });
            }
            0x80000005 | 0xc0000023 => grow(&mut words, (needed as usize).div_ceil(4), 0)?, // overflow / buffer too small
            0xc0000034 | 0xc000003a => return Err(changed()), // value disappeared
            other => {
                return Err(io::Error::other(format!(
                    "NtQueryValueKey returned {other:#010x}"
                )));
            }
        }
    }
    Err(changed())
}

fn pass(key: &Key) -> io::Result<Vec<RawRegistryValue>> {
    let before = metadata(key)?;
    let mut values = Vec::new();
    let mut name = vec![0u16; 64];
    let mut index = 0;
    loop {
        let mut value = None;
        for _ in 0..16 {
            let mut name_len = name.len() as u32;
            // SAFETY: name is an initialized writable UTF-16 buffer with
            // its unit count supplied. The owned query handle remains live;
            // all data/type outputs are omitted, so this enumerates names only.
            let status = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            match status {
                ERROR_SUCCESS => {
                    if name_len as usize >= name.len() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid registry name length",
                        ));
                    }
                    value = Some(raw_value(key, &name[..name_len as usize])?);
                    break;
                }
                ERROR_MORE_DATA => {
                    // A useful required name length is not guaranteed.
                    grow(&mut name, name_len as usize + 1, 0)?;
                }
                ERROR_NO_MORE_ITEMS => {
                    if before != metadata(key)? || values.len() != before.count as usize {
                        return Err(changed());
                    }
                    values.sort_by(|a: &RawRegistryValue, b| a.name_utf16.cmp(&b.name_utf16));
                    return Ok(values);
                }
                status => return Err(error(status)),
            }
        }
        values.push(value.ok_or_else(changed)?);
        index = index.checked_add(1).ok_or_else(changed)?;
    }
}

/// Reads all values under one HKCU key, retaining default/unknown values and
/// arbitrary raw types. Subkeys are not settings in NamespacedSettingsBag.
pub fn read_hkcu_values(subkey: &str) -> io::Result<RegistrySnapshot> {
    let Some(key) = open(subkey)? else {
        return Ok(RegistrySnapshot::default());
    };
    for _ in 0..3 {
        let capture = || -> io::Result<Vec<RawRegistryValue>> {
            let first = pass(&key)?;
            if first != pass(&key)? {
                return Err(changed());
            }
            Ok(first)
        };
        match capture() {
            Ok(values) => {
                return Ok(RegistrySnapshot {
                    key_existed: true,
                    values,
                });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
    }
    Err(changed())
}

pub fn read_legacy_settings() -> io::Result<RegistrySnapshot> {
    read_hkcu_values(REGISTRY_SETTINGS_SUBKEY)
}

/// Caller selects the installed Rust destination; no source key is created
/// when absent, and an existing archive bypasses registry capture entirely.
pub fn migrate_legacy_settings(
    destination: &Path,
) -> Result<bt_storage::MigratedRegistry, bt_storage::Error> {
    bt_storage::registry(destination, read_legacy_settings)
}

/// OS boundaries for reading an archived snapshot. No access to the original
/// key is necessary; casing follows Windows registry UTF-16 upcasing.
pub struct RegistryEnvironment;
impl bt_core::registry_values::Environment for RegistryEnvironment {
    fn names_equal(&self, left: &[u16], right: &[u16]) -> bool {
        if left.len() != right.len() {
            return false;
        }
        left.iter().zip(right).all(|(&a, &b)| {
            // SAFETY: pure ntdll character conversions; scalar UTF-16 code
            // units require no pointers/handles and may include surrogates.
            unsafe {
                windows::Wdk::System::SystemServices::RtlUpcaseUnicodeChar(a)
                    == windows::Wdk::System::SystemServices::RtlUpcaseUnicodeChar(b)
            }
        })
    }
    fn expand(&self, text: &[u16]) -> Result<Vec<u16>, bt_core::registry_values::ExpansionError> {
        use bt_core::registry_values::ExpansionError;
        let source: Vec<_> = text.iter().copied().chain(Some(0)).collect();
        let mut buffer = vec![0u16; 128];
        for _ in 0..8 {
            // SAFETY: source is NUL-terminated; buffer is an initialized
            // writable UTF-16 slice. The API is synchronous and read-only.
            let length = unsafe {
                windows::Win32::System::Environment::ExpandEnvironmentStringsW(
                    PCWSTR(source.as_ptr()),
                    Some(&mut buffer),
                )
            } as usize;
            if length == 0 {
                return Err(ExpansionError);
            }
            if length <= buffer.len() {
                buffer.truncate(length - 1);
                return Ok(buffer);
            }
            buffer
                .try_reserve_exact(length - buffer.len())
                .map_err(|_| ExpansionError)?;
            buffer.resize(length, 0);
        }
        Err(ExpansionError)
    }
}

#[cfg(test)]
mod tests;
