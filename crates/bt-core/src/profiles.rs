//! Pure persisted profile normalization and editing from VolumeProfileService.
//! ID allocation, local time formatting, storage and live audio stay at edges.
use crate::{
    models::*,
    text::{is_blank, ordinal_ignore_case, profile_slug},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedProfiles {
    pub profiles: Vec<VolumeProfile>,
    /// Mirrors assignedIds: only a new stable ID requests load-time persistence.
    pub assigned_ids: bool,
}

/// `allocate_id` is supplied by the app edge and must return a fresh GUID in
/// lowercase 32-hex-digit ("N") format, as Guid.NewGuid().ToString("N") does.
pub fn normalize(profile: &mut VolumeProfile, allocate_id: &mut impl FnMut() -> String) -> bool {
    let assigned_id = is_blank(profile.id.as_deref());
    if profile.schema_version <= 0 {
        profile.schema_version = 1;
    }
    if assigned_id {
        profile.id = Some(allocate_id());
    }
    if is_blank(profile.slug.as_deref()) {
        profile.slug = Some(profile_slug(profile.name.as_deref()));
    }
    if is_blank(profile.updated_at.as_deref()) {
        profile.updated_at.clone_from(&profile.created_at);
    }
    profile.hotkey.get_or_insert_default();
    profile.default_devices.get_or_insert_default();
    let devices = profile.devices.get_or_insert_default();
    devices.retain(Option::is_some);
    for device in devices.iter_mut().flatten() {
        let apps = device.apps.get_or_insert_default();
        apps.retain(Option::is_some);
        for app in apps.iter_mut().flatten() {
            if is_blank(app.device_id.as_deref()) {
                app.device_id.clone_from(&device.device_id);
            }
            if is_blank(app.device_display_name.as_deref()) {
                app.device_display_name.clone_from(&device.display_name);
            }
        }
    }
    assigned_id
}

/// Strict entry point retains the parse error for app diagnostics. XML nil,
/// blank strings and a JSON null list mean no presets in the C# implementation.
pub fn parse(
    json: &str,
    allocate_id: &mut impl FnMut() -> String,
) -> Result<ParsedProfiles, serde_json::Error> {
    if json.trim().is_empty() || json.trim_start().starts_with('<') {
        return Ok(ParsedProfiles::default());
    }
    let raw: Option<Vec<Option<VolumeProfile>>> = serde_json::from_str(json)?;
    let mut result = ParsedProfiles::default();
    for mut profile in raw.unwrap_or_default().into_iter().flatten() {
        result.assigned_ids |= normalize(&mut profile, allocate_id);
        result.profiles.push(profile);
    }
    Ok(result)
}

/// The public C# ParseProfiles facade catches all errors and returns empty.
pub fn parse_legacy(json: &str, allocate_id: &mut impl FnMut() -> String) -> Vec<VolumeProfile> {
    parse(json, allocate_id).unwrap_or_default().profiles
}

pub fn get_slug(profile: &VolumeProfile) -> String {
    if is_blank(profile.slug.as_deref()) {
        profile_slug(profile.name.as_deref())
    } else {
        profile.slug.clone().unwrap_or_default()
    }
}

pub fn app_key(app: &AppVolumeEntry) -> String {
    let identity = if is_blank(app.exe_name.as_deref()) {
        app.app_id.as_deref()
    } else {
        app.exe_name.as_deref()
    };
    format!(
        "{}|{}",
        app.device_id.as_deref().unwrap_or_default(),
        identity.unwrap_or_default()
    )
}

impl VolumeProfile {
    pub fn volume_devices(&self) -> impl Iterator<Item = &DeviceVolumeEntry> {
        self.devices
            .iter()
            .flatten()
            .flatten()
            .filter(|d| d.include_volume)
    }
    pub fn all_apps(&self) -> impl Iterator<Item = &AppVolumeEntry> {
        self.devices
            .iter()
            .flatten()
            .flatten()
            .flat_map(|d| d.apps.iter().flatten().flatten())
    }
    pub fn set_device_entry(
        &mut self,
        id: &str,
        volume: Option<i32>,
        muted: Option<bool>,
        now: &str,
    ) -> bool {
        let entry = self.devices.iter_mut().flatten().flatten().find(|d| {
            d.device_id
                .as_deref()
                .is_some_and(|key| ordinal_ignore_case(key, id))
        });
        let Some(entry) = entry else {
            return false;
        };
        if let Some(volume) = volume {
            entry.volume = volume.clamp(0, 100);
        }
        if let Some(muted) = muted {
            entry.is_muted = muted;
        }
        self.updated_at = Some(now.into());
        true
    }
    pub fn remove_device_entry(&mut self, id: &str, now: &str) -> bool {
        let Some(devices) = &mut self.devices else {
            return false;
        };
        let index = devices.iter().position(|d| {
            d.as_ref()
                .and_then(|d| d.device_id.as_deref())
                .is_some_and(|key| ordinal_ignore_case(key, id))
        });
        let Some(index) = index else {
            return false;
        };
        let device = devices[index].as_mut().expect("matched nonnull entry");
        if device.apps.as_ref().is_some_and(|apps| !apps.is_empty()) {
            device.include_volume = false;
        } else {
            devices.remove(index);
        }
        self.updated_at = Some(now.into());
        true
    }
    pub fn set_app_entry(
        &mut self,
        key: &str,
        volume: Option<i32>,
        muted: Option<bool>,
        now: &str,
    ) -> bool {
        let mut changed = false;
        for device in self.devices.iter_mut().flatten().flatten() {
            for app in device.apps.iter_mut().flatten().flatten() {
                if ordinal_ignore_case(&app_key(app), key) {
                    if let Some(volume) = volume {
                        app.volume = volume.clamp(0, 100);
                    }
                    if let Some(muted) = muted {
                        app.is_muted = muted;
                    }
                    changed = true;
                }
            }
        }
        if changed {
            self.updated_at = Some(now.into());
        }
        changed
    }
    pub fn remove_app_entry(&mut self, key: &str, now: &str) -> bool {
        let Some(devices) = &mut self.devices else {
            return false;
        };
        let mut removed = false;
        for device in devices.iter_mut().flatten() {
            if let Some(apps) = &mut device.apps {
                let before = apps.len();
                apps.retain(|app| {
                    app.as_ref()
                        .is_none_or(|app| !ordinal_ignore_case(&app_key(app), key))
                });
                removed |= apps.len() != before;
            }
        }
        // The source prunes empty app-only carriers even when no app matched,
        // but only touches/persists the profile if an app actually was removed.
        devices.retain(|d| {
            d.as_ref().is_none_or(|d| {
                d.include_volume || d.apps.as_ref().is_some_and(|apps| !apps.is_empty())
            })
        });
        if removed {
            self.updated_at = Some(now.into());
        }
        removed
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProfileCollection {
    profiles: Vec<VolumeProfile>,
}

impl ProfileCollection {
    pub fn new(profiles: Vec<VolumeProfile>) -> Self {
        Self { profiles }
    }
    pub fn profiles(&self) -> &[VolumeProfile] {
        &self.profiles
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut VolumeProfile> {
        self.profiles.get_mut(index)
    }
    pub fn find_by_id(&self, id: &str) -> Option<usize> {
        if is_blank(Some(id)) {
            return None;
        }
        self.profiles.iter().position(|p| {
            p.id.as_deref()
                .is_some_and(|key| ordinal_ignore_case(key, id))
        })
    }
    pub fn find(&self, name_or_slug: &str) -> Option<usize> {
        if is_blank(Some(name_or_slug)) {
            return None;
        }
        let slug = profile_slug(Some(name_or_slug));
        self.profiles
            .iter()
            .position(|p| ordinal_ignore_case(&get_slug(p), &slug))
            .or_else(|| {
                self.profiles.iter().position(|p| {
                    p.name
                        .as_deref()
                        .is_some_and(|name| ordinal_ignore_case(name, name_or_slug))
                })
            })
    }
    pub fn cycle_target(&self, last_applied: &str, direction: i32) -> Option<usize> {
        let count = self.profiles.len();
        if count == 0 {
            return None;
        }
        Some(match self.find_by_id(last_applied) {
            None => {
                if direction >= 0 {
                    0
                } else {
                    count - 1
                }
            }
            Some(index) => {
                if direction >= 0 {
                    (index + 1) % count
                } else {
                    (index + count - 1) % count
                }
            }
        })
    }
    /// Save a newly captured/replacement value. In-place edits should use
    /// get_mut + commit_edit, mirroring the source's ReferenceEquals branch.
    pub fn save_replacement(&mut self, mut profile: VolumeProfile, now: &str) -> usize {
        if let Some(index) = self.profiles.iter().position(|p| p.name == profile.name) {
            let existing = &self.profiles[index];
            if !is_blank(existing.id.as_deref()) {
                profile.id.clone_from(&existing.id);
            }
            if !is_blank(existing.created_at.as_deref()) {
                profile.created_at.clone_from(&existing.created_at);
            }
            if profile.hotkey.is_none_or(|h| h.is_empty()) && existing.hotkey.is_some() {
                profile.hotkey = existing.hotkey;
            }
            profile.updated_at = Some(now.into());
            self.profiles[index] = profile;
            index
        } else {
            self.profiles.push(profile);
            self.profiles.len() - 1
        }
    }
    pub fn commit_edit(&mut self, index: usize, now: &str) -> bool {
        let Some(profile) = self.profiles.get_mut(index) else {
            return false;
        };
        profile.updated_at = Some(now.into());
        true
    }
    /// Rename deliberately leaves Slug and UpdatedAt untouched in this layer.
    pub fn rename(&mut self, index: usize, name: Option<String>) -> bool {
        let Some(profile) = self.profiles.get_mut(index) else {
            return false;
        };
        profile.name = name;
        true
    }
    pub fn delete(&mut self, index: usize) -> bool {
        if index >= self.profiles.len() {
            return false;
        }
        self.profiles.remove(index);
        true
    }
    pub fn import_json(
        &mut self,
        json: &str,
        allocate_id: &mut impl FnMut() -> String,
    ) -> Result<Option<usize>, serde_json::Error> {
        let Some(mut profile): Option<VolumeProfile> = serde_json::from_str(json)? else {
            return Ok(None);
        };
        normalize(&mut profile, allocate_id);
        if profile
            .id
            .as_deref()
            .is_some_and(|id| self.find_by_id(id).is_some())
        {
            profile.id = Some(allocate_id());
        }
        if self.profiles.iter().any(|p| p.name == profile.name) {
            profile.name = Some(format!(
                "{} (Imported)",
                profile.name.as_deref().unwrap_or_default()
            ));
        }
        self.profiles.push(profile);
        Ok(Some(self.profiles.len() - 1))
    }
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.profiles)
    }
}
