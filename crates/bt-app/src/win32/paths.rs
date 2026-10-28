//! Path resolution and ID allocation for folder volume rules: .NET 8's
//! `Path.GetFullPath` on Windows and `Guid.NewGuid().ToString("N")`.
use bt_core::folders::{PathError, WindowsPathResolver};
use windows::{
    Win32::{Storage::FileSystem::GetFullPathNameW, System::Com::CoCreateGuid},
    core::PCWSTR,
};

/// `Path.GetFullPath`: empty or NUL-containing input is an ArgumentException,
/// `\\?\` paths are returned unchanged, everything else goes through
/// GetFullPathNameW against the process's current directory.
pub struct FullPath;

impl WindowsPathResolver for FullPath {
    fn full_path(&self, value: &str) -> Result<String, PathError> {
        if value.is_empty() || value.contains('\0') {
            return Err(PathError::InvalidArgument);
        }
        if value.starts_with(r"\\?\") {
            return Ok(value.into());
        }
        let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
        let mut buffer = vec![0u16; 260];
        loop {
            // SAFETY: `wide` is NUL-terminated and `buffer` is a writable
            // slice whose length the binding passes; both outlive the call.
            let length =
                unsafe { GetFullPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buffer), None) };
            if length == 0 {
                return Err(PathError::Other(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
            let length = length as usize;
            if length < buffer.len() {
                return Ok(String::from_utf16_lossy(&buffer[..length]));
            }
            // Too small: `length` is the size needed, terminator included.
            buffer.resize(length, 0);
        }
    }
}

/// A new GUID as 32 lowercase hex digits (`ToString("N")`).
pub fn new_id() -> String {
    // SAFETY: CoCreateGuid has no preconditions (it needs no COM apartment).
    let guid = unsafe { CoCreateGuid() }.unwrap_or_default();
    format!(
        "{:08x}{:04x}{:04x}{}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_32_lowercase_hex_digits() {
        let (a, b) = (new_id(), new_id());
        assert_eq!(a.len(), 32);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_ne!(a, b);
    }

    #[test]
    fn full_path_matches_get_full_path_basics() {
        assert_eq!(FullPath.full_path(""), Err(PathError::InvalidArgument));
        assert_eq!(FullPath.full_path("a\0b"), Err(PathError::InvalidArgument));
        assert_eq!(FullPath.full_path(r"\\?\C:\x\..").unwrap(), r"\\?\C:\x\..");
        assert_eq!(FullPath.full_path(r"C:\a\.\b\..\c").unwrap(), r"C:\a\c");
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            FullPath.full_path("child").unwrap(),
            cwd.join("child").to_string_lossy()
        );
    }
}
