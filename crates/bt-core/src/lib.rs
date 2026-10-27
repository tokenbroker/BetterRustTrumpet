//! Portable BetterTrumpet behavior. Keep Windows APIs and UI dependencies out.
#![forbid(unsafe_code)]

pub mod app_rules;

pub mod cli;
pub mod cli_responses;
pub mod cli_rules;
pub mod cli_validation;
pub mod colors;
pub mod dotnet_datetime;
pub mod focus;
pub mod folder_rules;
pub mod folders;
pub mod hidden;
pub mod identity;
pub mod legacy_store;
pub mod legacy_xml;
pub mod migration;
pub mod models;
pub mod newtonsoft;
pub mod numbers;
pub mod overrides;
pub mod profiles;
pub mod protocol;
pub mod registry_snapshot;
pub mod registry_values;
pub mod rules;
pub mod settings;
pub mod text;
pub mod themes;
pub mod typed_lists;
pub mod typed_profiles;
pub mod undo;
pub mod window_policy;
