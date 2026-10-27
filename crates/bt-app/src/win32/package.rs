//! Read-only packaged (MSIX) LocalSettings capture. The original MSIX build
//! stores settings in its package's LocalSettings; an unpackaged Rust build
//! reads them through ApplicationDataManager without writing anything.
use bt_core::package_snapshot::{PackageSnapshot, RawPackageEntry, RawPackageValue};
use std::{io, sync::OnceLock};
use windows::{
    Foundation::{Collections::IPropertySet, IPropertyValue, PropertyType},
    Management::Core::ApplicationDataManager,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER},
        Storage::Packaging::Appx::{PACKAGE_ID, PackageFamilyNameFromId},
        System::Com::CoIncrementMTAUsage,
    },
    core::{HSTRING, IInspectable, Interface, PWSTR},
};

/// Identity of the original Store/MSIX package (EarTrumpet.Package manifest).
pub const PACKAGE_NAME: &str = "xammen.Bettertrumpet";
pub const PACKAGE_PUBLISHER: &str = "CN=7EDFC72A-8780-4841-8F34-30B45D719EAF";

fn io_error(error: windows::core::Error) -> io::Error {
    io::Error::other(format!("{} ({:?})", error.message(), error.code()))
}
fn changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "packaged settings changed during capture",
    )
}

/// `PackageFamilyNameFromId` for a name and publisher.
pub fn family_name(name: &str, publisher: &str) -> io::Result<String> {
    let mut name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut publisher: Vec<u16> = publisher.encode_utf16().chain(Some(0)).collect();
    let id = PACKAGE_ID {
        name: PWSTR(name.as_mut_ptr()),
        publisher: PWSTR(publisher.as_mut_ptr()),
        ..Default::default()
    };
    let mut buffer = vec![0u16; 128];
    for _ in 0..2 {
        let mut length = buffer.len() as u32;
        // SAFETY: `id` points at NUL-terminated buffers that outlive the call;
        // `buffer` is writable for `length` UTF-16 units, as the API requires.
        let status =
            unsafe { PackageFamilyNameFromId(&id, &mut length, Some(PWSTR(buffer.as_mut_ptr()))) };
        if status == ERROR_INSUFFICIENT_BUFFER {
            buffer.resize(length as usize, 0);
            continue;
        }
        status.ok().map_err(io_error)?;
        let units = &buffer[..(length as usize).saturating_sub(1)];
        return String::from_utf16(units).map_err(io::Error::other);
    }
    Err(io::Error::other(
        "PackageFamilyNameFromId buffer size changed",
    ))
}

/// WinRT calls need a multithreaded apartment somewhere in the process; keep
/// one alive for the process lifetime without initializing calling threads.
fn ensure_mta() -> io::Result<()> {
    static MTA: OnceLock<Result<(), String>> = OnceLock::new();
    MTA.get_or_init(|| {
        // SAFETY: CoIncrementMTAUsage has no preconditions. The cookie is
        // deliberately never passed to CoDecrementMTAUsage, keeping the MTA
        // alive for the rest of the process.
        unsafe { CoIncrementMTAUsage() }
            .map(|_cookie| ())
            .map_err(|e| e.message().to_string())
    })
    .clone()
    .map_err(io::Error::other)
}

fn entry(key: &HSTRING, value: &IInspectable) -> windows::core::Result<RawPackageEntry> {
    let value = match value.cast::<IPropertyValue>() {
        Ok(property) => match property.Type()? {
            PropertyType::Boolean => RawPackageValue::Bool {
                value: property.GetBoolean()?,
            },
            PropertyType::String => RawPackageValue::String {
                utf16: property.GetString()?.to_vec(),
            },
            other => RawPackageValue::Other {
                property_type: Some(other.0),
            },
        },
        // Composite values and other non-property objects.
        Err(_) => RawPackageValue::Other {
            property_type: None,
        },
    };
    Ok(RawPackageEntry {
        key_utf16: key.to_vec(),
        value,
    })
}

fn pass(values: &IPropertySet) -> windows::core::Result<Vec<RawPackageEntry>> {
    let mut out = vec![];
    let iterator = values.First()?;
    while iterator.HasCurrent()? {
        let pair = iterator.Current()?;
        out.push(entry(&pair.Key()?, &pair.Value()?)?);
        iterator.MoveNext()?;
    }
    Ok(out)
}

/// Captures every LocalSettings value of `family` read-only. A family that is
/// not installed yields `container_existed: false` and creates nothing.
pub fn capture(family: &str) -> io::Result<PackageSnapshot> {
    ensure_mta()?;
    let data = match ApplicationDataManager::CreateForPackageFamily(&HSTRING::from(family)) {
        Ok(data) => data,
        Err(error) if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => {
            return Ok(PackageSnapshot {
                family: family.into(),
                container_existed: false,
                entries: vec![],
            });
        }
        Err(error) => return Err(io_error(error)),
    };
    let values = data
        .LocalSettings()
        .and_then(|settings| settings.Values())
        .map_err(io_error)?;
    for _ in 0..3 {
        let first = pass(&values).map_err(io_error)?;
        if first == pass(&values).map_err(io_error)? {
            return Ok(PackageSnapshot {
                family: family.into(),
                container_existed: true,
                entries: first,
            });
        }
    }
    Err(changed())
}

/// The original package's family for this machine.
pub fn legacy_family() -> io::Result<String> {
    family_name(PACKAGE_NAME, PACKAGE_PUBLISHER)
}

/// Caller selects the Rust destination; an existing archive bypasses the
/// capture entirely and the package's settings are never written.
pub fn migrate_legacy_settings(
    destination: &std::path::Path,
) -> Result<bt_storage::MigratedPackage, bt_storage::Error> {
    bt_storage::package(destination, || capture(&legacy_family()?))
}

#[cfg(test)]
mod tests;
