//! Atomic editable destination persistence. In-memory mutation and observer
//! timing belong to the caller, independently of whether disk saving succeeds.
use bt_core::overrides::Overrides;
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

pub const FILE_NAME: &str = "settings.v1.json";
#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot access editable settings: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Format(#[from] bt_core::overrides::Error),
    #[error("editable settings changed on disk; reload before saving")]
    Conflict,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Saved,
    Unchanged,
}

struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
pub struct EditableFile {
    pub document: Overrides,
    path: PathBuf,
    baseline: Option<Vec<u8>>,
    existed_on_open: bool,
}
impl EditableFile {
    /// Opening is read-only and does not create even an empty destination.
    pub fn open(directory: &Path) -> Result<Self, Error> {
        let path = directory.join(FILE_NAME);
        let baseline = read(&path)?;
        let document = match &baseline {
            Some(bytes) => Overrides::decode(bytes)?,
            None => Overrides::new(),
        };
        let existed_on_open = baseline.is_some();
        Ok(Self {
            document,
            path,
            baseline,
            existed_on_open,
        })
    }
    pub fn existed_on_open(&self) -> bool {
        self.existed_on_open
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Serialize cooperating writers with a stable sidecar lock, compare the
    /// last loaded/saved bytes, then replace only this Rust-owned destination.
    /// A failed save never rolls back the caller's in-memory document.
    pub fn save(&mut self) -> Result<Outcome, Error> {
        let encoded = self.document.encode()?;
        let directory = self.path.parent().expect("fixed settings path has parent");
        fs::create_dir_all(directory)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("settings.v1.lock"))?;
        FileExt::lock_exclusive(&file)?;
        let _lock = Lock(file);
        let current = read(&self.path)?;
        if current != self.baseline {
            return Err(Error::Conflict);
        }
        if current.as_deref() == Some(encoded.as_slice()) {
            return Ok(Outcome::Unchanged);
        }
        let mut staged = tempfile::Builder::new()
            .prefix(".bt-settings-")
            .tempfile_in(directory)?;
        staged.write_all(&encoded)?;
        staged.flush()?;
        staged.as_file().sync_all()?;
        staged.persist(&self.path).map_err(|e| e.error)?;
        self.baseline = Some(encoded);
        Ok(Outcome::Saved)
    }
}
