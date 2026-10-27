//! First-run portable migration. Preserve the complete source bytes in an
//! immutable, versioned archive; decoded reads retain legacy bag semantics.
//! The app's editable settings will be a separate file, never this archive.
#![forbid(unsafe_code)]
pub mod editable;
use bt_core::{
    legacy_store::{PORTABLE_MARKERS, PortableSnapshot},
    migration,
    newtonsoft::LocalZone,
    registry_snapshot::RegistrySnapshot,
};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const ARCHIVE_COMPONENTS: [&str; 3] = ["config", "rust", "migration.v1.json"];

#[derive(Debug, Error)]
pub enum Error {
    #[error("the executable directory has no portable marker")]
    NotPortable,
    #[error("cannot access migration data: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Archive(#[from] migration::Error),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Existing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MigratedPortable {
    pub outcome: Outcome,
    pub archive_path: PathBuf,
    pub snapshot: PortableSnapshot,
}

fn existing(path: PathBuf, file: File, zone: &dyn LocalZone) -> Result<MigratedPortable, Error> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let mut file = file;
    file.read_to_end(&mut bytes)?;
    let snapshot = migration::decode_portable(&bytes, zone)?;
    Ok(MigratedPortable {
        outcome: Outcome::Existing,
        archive_path: path,
        snapshot,
    })
}

/// Only the dedicated Rust archive directory is writable. The source is
/// `config/settings.json` and is opened solely through fs::read. A marker is
/// checked with File.Exists semantics (directories do not count).
///
/// An existing archive wins over a later source edit. A corrupt/unsupported
/// archive returns an error without overwriting it or silently reimporting.
/// Native source discovery for registry/package modes is a separate adapter.
pub fn portable(
    executable_directory: &Path,
    zone: &dyn LocalZone,
) -> Result<MigratedPortable, Error> {
    if !PORTABLE_MARKERS
        .iter()
        .any(|name| executable_directory.join(name).is_file())
    {
        return Err(Error::NotPortable);
    }
    let archive_path: PathBuf = ARCHIVE_COMPONENTS
        .iter()
        .fold(executable_directory.to_owned(), |path, component| {
            path.join(component)
        });
    match File::open(&archive_path) {
        Ok(file) => return existing(archive_path, file, zone),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let source = executable_directory.join("config").join("settings.json");
    let bytes = match fs::read(source) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let encoded = migration::encode_portable(bytes.as_deref())?;
    match install(&archive_path, &encoded)? {
        Outcome::Created => Ok(MigratedPortable {
            outcome: Outcome::Created,
            archive_path,
            snapshot: PortableSnapshot::capture(bytes, zone),
        }),
        Outcome::Existing => {
            let file = File::open(&archive_path)?;
            existing(archive_path, file, zone)
        }
    }
}

fn install(archive_path: &Path, encoded: &[u8]) -> Result<Outcome, Error> {
    let parent = archive_path
        .parent()
        .expect("fixed archive path has a parent");
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::Builder::new()
        .prefix(".bt-migration-")
        .tempfile_in(parent)?;
    staged.write_all(encoded)?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    // tempfile uses a platform no-clobber installation. Ordinary rename would
    // replace an existing archive on Unix, losing a concurrent winner's data.
    match staged.persist_noclobber(archive_path) {
        Ok(_) => Ok(Outcome::Created),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => Ok(Outcome::Existing),
        Err(error) => Err(error.error.into()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigratedRegistry {
    pub outcome: Outcome,
    pub archive_path: PathBuf,
    pub snapshot: RegistrySnapshot,
}

/// Installed migration. The app supplies its Rust-owned directory and a
/// read-only native capture callback. Existing archives bypass that callback.
pub fn registry(
    destination_directory: &Path,
    capture: impl FnOnce() -> io::Result<RegistrySnapshot>,
) -> Result<MigratedRegistry, Error> {
    use std::io::Read;
    fn load(path: &Path, mut file: File) -> Result<MigratedRegistry, Error> {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(MigratedRegistry {
            outcome: Outcome::Existing,
            archive_path: path.into(),
            snapshot: migration::decode_registry(&bytes)?,
        })
    }
    let path = destination_directory.join("migration.v1.json");
    match File::open(&path) {
        Ok(file) => return load(&path, file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let snapshot = capture()?;
    let encoded = migration::encode_registry(&snapshot)?;
    match install(&path, &encoded)? {
        Outcome::Created => Ok(MigratedRegistry {
            outcome: Outcome::Created,
            archive_path: path,
            snapshot,
        }),
        Outcome::Existing => {
            let file = File::open(&path)?;
            load(&path, file)
        }
    }
}
