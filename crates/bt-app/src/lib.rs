//! Native adapters shared by the application and isolated Windows tests.
pub mod i18n;
#[cfg(windows)]
pub mod win32;
