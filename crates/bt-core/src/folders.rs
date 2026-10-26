//! FolderVolumeRuleMatcher.cs logical matching. The app supplies Windows
//! full-path resolution for context-dependent relative/drive-relative inputs.
use crate::{
    identity::{windows_is_fully_qualified, windows_root_length},
    text::{bmp_letter, is_blank, ordinal_ignore_case, ordinal_starts_with},
};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PathError {
    #[error("invalid Windows path")]
    InvalidArgument,
    #[error("unsupported Windows path")]
    NotSupported,
    #[error("Windows path resolution failed: {0}")]
    Other(String),
}
pub trait WindowsPathResolver {
    fn full_path(&self, value: &str) -> Result<String, PathError>;
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub folder_path: Option<String>,
    pub volume_percent: i32,
    pub created_at_ticks: i64,
}

fn windows_path(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(bmp_letter)
        && chars.next() == Some(':')
        && matches!(chars.next(), Some('\\' | '/'))
}
fn unc_path(value: &str) -> bool {
    value.starts_with("\\\\") || value.starts_with("//")
}
fn unix_path(value: &str) -> bool {
    value.starts_with('/') && !unc_path(value)
}
fn trim_path(value: Option<&str>) -> &str {
    value.unwrap_or_default().trim().trim_matches('"')
}
fn trim_separators(value: &str) -> &str {
    value.trim_end_matches(['\\', '/'])
}

pub fn is_usable_executable(value: Option<&str>) -> bool {
    if is_blank(value) {
        return false;
    }
    let value = value.unwrap_or_default();
    if windows_path(value) || unc_path(value) || unix_path(value) {
        return true;
    }
    windows_is_fully_qualified(value)
}

fn normalize(
    value: Option<&str>,
    folder: bool,
    resolver: &impl WindowsPathResolver,
) -> Result<String, PathError> {
    let value = trim_path(value);
    if value.is_empty() {
        return Ok(String::new());
    }
    if windows_path(value) || unc_path(value) {
        let normalized = value.replace('/', "\\");
        return Ok(if folder {
            trim_separators(&normalized).into()
        } else {
            normalized
        });
    }
    if unix_path(value) {
        let normalized = value.replace('\\', "/");
        let trimmed = normalized.trim_end_matches('/');
        return Ok(if trimmed.is_empty() {
            "/".into()
        } else {
            trimmed.into()
        });
    }
    let normalized = match resolver.full_path(value) {
        Ok(path) => path,
        Err(PathError::InvalidArgument | PathError::NotSupported) => return Ok(String::new()),
        Err(error) => return Err(error),
    };
    if folder
        && normalized.ends_with(['\\', '/'])
        && normalized.encode_utf16().count()
            > windows_root_length(&normalized.encode_utf16().collect::<Vec<_>>())
    {
        Ok(normalized[..normalized.len() - 1].into())
    } else {
        Ok(normalized)
    }
}

pub fn normalize_folder(
    value: Option<&str>,
    resolver: &impl WindowsPathResolver,
) -> Result<String, PathError> {
    normalize(value, true, resolver)
}
pub fn normalize_executable(
    value: Option<&str>,
    resolver: &impl WindowsPathResolver,
) -> Result<String, PathError> {
    normalize(value, false, resolver)
}

pub fn is_executable_under_folder(
    executable: Option<&str>,
    folder: Option<&str>,
    resolver: &impl WindowsPathResolver,
) -> Result<bool, PathError> {
    let folder = normalize_folder(folder, resolver)?;
    let executable = normalize_executable(executable, resolver)?;
    if folder.is_empty() || executable.is_empty() {
        return Ok(false);
    }
    let separator = if folder.contains('\\') || executable.contains('\\') {
        '\\'
    } else {
        '/'
    };
    let prefix = if folder == "/" || folder.ends_with(['\\', '/']) {
        folder.clone()
    } else {
        format!("{folder}{separator}")
    };
    if ordinal_ignore_case(&executable, &folder)
        || ordinal_ignore_case(&executable, trim_separators(&folder))
        || ordinal_starts_with(&executable, &prefix)
    {
        return Ok(true);
    }
    let alternative = if separator == '\\' { '/' } else { '\\' };
    Ok(ordinal_starts_with(
        &executable,
        &prefix.replace(separator, &alternative.to_string()),
    ))
}

pub fn try_match(
    executable: Option<&str>,
    rules: &[Rule],
    resolver: &impl WindowsPathResolver,
) -> Result<Option<i32>, PathError> {
    if !is_usable_executable(executable) {
        return Ok(None);
    }
    let mut best: Option<&Rule> = None;
    for rule in rules {
        if is_blank(rule.folder_path.as_deref())
            || !is_executable_under_folder(executable, rule.folder_path.as_deref(), resolver)?
        {
            continue;
        }
        let Some(current) = best else {
            best = Some(rule);
            continue;
        };
        let length = normalize_folder(rule.folder_path.as_deref(), resolver)?
            .encode_utf16()
            .count();
        let current_length = normalize_folder(current.folder_path.as_deref(), resolver)?
            .encode_utf16()
            .count();
        if length > current_length
            || (length == current_length && rule.created_at_ticks < current.created_at_ticks)
        {
            best = Some(rule);
        }
    }
    Ok(best.map(|rule| rule.volume_percent.clamp(0, 100)))
}
