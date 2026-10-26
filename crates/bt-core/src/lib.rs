//! Portable BetterTrumpet behavior. Keep Windows APIs and UI dependencies out.
#![forbid(unsafe_code)]

pub mod cli;
pub mod cli_responses;
pub mod cli_rules;
pub mod cli_validation;
pub mod focus;
pub mod folders;
pub mod identity;
pub mod legacy_xml;
pub mod models;
pub mod numbers;
pub mod profiles;
pub mod protocol;
pub mod rules;
pub mod settings;
pub mod text;
pub mod themes;
pub mod undo;
pub mod window_policy;
