//! The common item dialogs (IFileOpenDialog, IFileSaveDialog): open or
//! save a file and pick a folder, modal to an owner window. Each returns
//! `None` when the user cancels or the dialog fails.
use std::{ffi::c_void, path::PathBuf};
use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree},
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, FOS_FORCEFILESYSTEM, FOS_OVERWRITEPROMPT, FOS_PICKFOLDERS,
            FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IFileSaveDialog,
            SIGDN_FILESYSPATH,
        },
    },
    core::{HSTRING, Interface, PCWSTR},
};

/// A file type: its name ("BetterTrumpet Profile (*.btprofile)") and
/// pattern ("*.btprofile").
pub type Filter<'a> = (&'a str, &'a str);

/// Title, file types and default extension (without the dot).
fn prepare(dialog: &IFileDialog, title: &str, filters: &[Filter<'_>], ext: &str) -> Option<()> {
    let names: Vec<HSTRING> = filters.iter().map(|(n, _)| HSTRING::from(*n)).collect();
    let specs: Vec<HSTRING> = filters.iter().map(|(_, s)| HSTRING::from(*s)).collect();
    let types: Vec<COMDLG_FILTERSPEC> = names
        .iter()
        .zip(&specs)
        .map(|(name, spec)| COMDLG_FILTERSPEC {
            pszName: PCWSTR(name.as_ptr()),
            pszSpec: PCWSTR(spec.as_ptr()),
        })
        .collect();
    // SAFETY: `dialog` is a live COM object; the strings behind `types`
    // outlive the calls, which copy them.
    unsafe {
        dialog.SetTitle(&HSTRING::from(title)).ok()?;
        if !types.is_empty() {
            dialog.SetFileTypes(&types).ok()?;
            dialog.SetFileTypeIndex(1).ok()?;
        }
        if !ext.is_empty() {
            dialog.SetDefaultExtension(&HSTRING::from(ext)).ok()?;
        }
    }
    Some(())
}

/// Shows the dialog and returns the chosen file system path.
fn show(dialog: &IFileDialog, owner: isize) -> Option<PathBuf> {
    // SAFETY: `dialog` is a live COM object and `owner` a window of this
    // thread (or 0); the returned string is freed with CoTaskMemFree.
    unsafe {
        dialog.Show(Some(HWND(owner as *mut c_void))).ok()?;
        let item = dialog.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const c_void));
        path.map(PathBuf::from)
    }
}

/// An Open dialog for one existing file.
pub fn open_file(owner: isize, title: &str, filters: &[Filter<'_>], ext: &str) -> Option<PathBuf> {
    // SAFETY: plain COM creation on the UI thread, which initialized COM.
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let dialog: IFileDialog = dialog.cast().ok()?;
    prepare(&dialog, title, filters, ext)?;
    // SAFETY: as above.
    unsafe {
        let options = dialog.GetOptions().ok()?;
        dialog.SetOptions(options | FOS_FORCEFILESYSTEM).ok()?;
    }
    show(&dialog, owner)
}

/// A Save dialog proposing `file_name`; asks before overwriting.
pub fn save_file(
    owner: isize,
    title: &str,
    filters: &[Filter<'_>],
    ext: &str,
    file_name: &str,
) -> Option<PathBuf> {
    // SAFETY: plain COM creation on the UI thread, which initialized COM.
    let dialog: IFileSaveDialog =
        unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let dialog: IFileDialog = dialog.cast().ok()?;
    prepare(&dialog, title, filters, ext)?;
    // SAFETY: as above; the name is copied by the call.
    unsafe {
        let options = dialog.GetOptions().ok()?;
        dialog
            .SetOptions(options | FOS_FORCEFILESYSTEM | FOS_OVERWRITEPROMPT)
            .ok()?;
        dialog.SetFileName(&HSTRING::from(file_name)).ok()?;
    }
    show(&dialog, owner)
}

/// A folder picker (the Open dialog in folder mode).
pub fn pick_folder(owner: isize, title: &str) -> Option<PathBuf> {
    // SAFETY: plain COM creation on the UI thread, which initialized COM.
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let dialog: IFileDialog = dialog.cast().ok()?;
    prepare(&dialog, title, &[], "")?;
    // SAFETY: as above.
    unsafe {
        let options = dialog.GetOptions().ok()?;
        dialog
            .SetOptions(options | FOS_FORCEFILESYSTEM | FOS_PICKFOLDERS)
            .ok()?;
    }
    show(&dialog, owner)
}
