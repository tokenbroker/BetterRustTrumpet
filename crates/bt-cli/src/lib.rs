//! The `bt` client: sends one command line to the running BetterTrumpet
//! Rust app over its named pipe and returns the JSON line it answers.
#![deny(unsafe_code)]

#[cfg(windows)]
pub mod client;
#[cfg(windows)]
#[allow(unsafe_code)]
mod win32;
