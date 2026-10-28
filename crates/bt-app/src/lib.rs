//! Native adapters shared by the application and isolated Windows tests.
pub mod i18n;
pub mod log;
#[cfg(windows)]
pub mod win32;
