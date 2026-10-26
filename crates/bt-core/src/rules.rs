//! Persisted rule/hidden-list cleanup from `AppSettings.cs`.
//! These functions never write the source settings store.
use std::collections::HashSet;

use crate::models::{
    AppRuleEntry, HiddenAppEntry, HiddenDeviceEntry, LegacyHardMutedAppEntry, VolumeRuleMode,
};
use crate::text::invariant_lower;

pub fn normalize_key(value: Option<&str>) -> String {
    invariant_lower(value.unwrap_or_default().trim())
}

fn display(value: Option<String>) -> Option<String> {
    Some(value.unwrap_or_default().trim().to_owned())
}

pub fn normalize_apps(
    entries: impl IntoIterator<Item = Option<AppRuleEntry>>,
) -> Vec<AppRuleEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .flatten()
        .filter_map(|mut entry| {
            let key = normalize_key(entry.exe_name.as_deref());
            if key.is_empty() || !seen.insert(key.clone()) {
                return None;
            }
            entry.exe_name = Some(key);
            entry.display_name = display(entry.display_name);
            entry.icon_path = display(entry.icon_path);
            entry.is_pinned = true;
            if !matches!(
                entry.volume_mode,
                VolumeRuleMode::NONE | VolumeRuleMode::LAUNCH | VolumeRuleMode::LOCK
            ) {
                entry.volume_mode = VolumeRuleMode::NONE;
            }
            entry.volume_percent = entry.volume_percent.clamp(0, 100);
            Some(entry)
        })
        .collect()
}

pub fn normalize_hidden_apps(
    entries: impl IntoIterator<Item = Option<HiddenAppEntry>>,
) -> Vec<HiddenAppEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .flatten()
        .filter_map(|mut entry| {
            let device = normalize_key(entry.device_id.as_deref());
            let app = normalize_key(entry.app_id.as_deref());
            let exe = normalize_key(entry.exe_name.as_deref());
            // Preserve the legacy unescaped delimiter, including its collisions.
            if device.is_empty()
                || (app.is_empty() && exe.is_empty())
                || !seen.insert(format!("{device}|{app}|{exe}"))
            {
                return None;
            }
            entry.device_id = Some(device);
            entry.app_id = Some(app);
            entry.exe_name = Some(exe);
            entry.display_name = display(entry.display_name);
            Some(entry)
        })
        .collect()
}

pub fn normalize_hidden_devices(
    entries: impl IntoIterator<Item = Option<HiddenDeviceEntry>>,
) -> Vec<HiddenDeviceEntry> {
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .flatten()
        .filter_map(|mut entry| {
            let key = normalize_key(entry.device_id.as_deref());
            if key.is_empty() || !seen.insert(key.clone()) {
                return None;
            }
            entry.device_id = Some(key);
            entry.display_name = display(entry.display_name);
            Some(entry)
        })
        .collect()
}

#[derive(Debug, PartialEq)]
pub struct LoadedRules {
    pub entries: Vec<AppRuleEntry>,
    /// The original loader persists even an empty migration if the legacy list
    /// contained entries. A future store adapter must write only the new store.
    pub save_migration: bool,
}

/// Parsed-input half of EnsureAppRulesLoaded/MigrateLegacyHardMutedAppsUnsafe.
/// Store/JSON adapters must map their caught read errors to empty input lists.
pub fn load_apps(
    current: Vec<Option<AppRuleEntry>>,
    legacy: Option<Vec<Option<LegacyHardMutedAppEntry>>>,
) -> LoadedRules {
    let entries = normalize_apps(current);
    if !entries.is_empty() {
        return LoadedRules {
            entries,
            save_migration: false,
        };
    }
    let legacy = legacy.unwrap_or_default();
    let save_migration = !legacy.is_empty();
    let entries = normalize_apps(legacy.into_iter().flatten().map(|entry| {
        Some(AppRuleEntry {
            exe_name: entry.exe_name,
            display_name: entry.display_name,
            hard_muted: true,
            created_at_utc: entry.hard_muted_at_utc,
            ..AppRuleEntry::default()
        })
    }));
    LoadedRules {
        entries,
        save_migration,
    }
}
