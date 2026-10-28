//! QuickTrumpet volume presets (VolumeProfileService): capture, apply,
//! edit, rename, delete, import and export, stored in VolumeProfilesJson in
//! the original's Newtonsoft shape through the Store, and the confirmation
//! text (QuickTrumpetSummary).
//!
//! Like the original, which builds a new service for every use, each
//! [`Service`] loads the stored list when opened and persists after every
//! change; the pure list logic is bt-core's ProfileCollection.
//!
//! The tray, the hotkeys and the palette only apply presets; capture and
//! editing are here for the settings window's QuickTrumpet page.
#![allow(dead_code)]
use crate::{core::Core, rules, store::Store};
use bt_app::i18n::{tr, trf};
use bt_app::win32::clock::{self, WindowsZone};
use bt_audio::{Role, Snapshot};
use bt_core::{
    models::{
        AppVolumeEntry, CaptureScope, DefaultDeviceEntry, DefaultDeviceRole, DefaultDeviceSet,
        DeviceVolumeEntry, VolumeProfile,
    },
    profiles::{self, ProfileCollection},
    settings::{Key, Value},
    text::{is_blank, ordinal_ignore_case, profile_slug},
};

/// The original's CaptureOptions, with its defaults.
#[derive(Clone, Debug)]
pub struct CaptureOptions {
    pub include_device_volumes: bool,
    /// Explicit volume devices; `None` means the default output.
    pub device_ids: Option<Vec<String>>,
    pub all_devices: bool,
    pub include_app_volumes: bool,
    /// Apps from every output, or only from the volume devices.
    pub apps_from_all_devices: bool,
    pub route_apps: bool,
    pub default_roles: Option<Vec<Role>>,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            include_device_volumes: true,
            device_ids: None,
            all_devices: false,
            include_app_volumes: true,
            apps_from_all_devices: true,
            route_apps: false,
            default_roles: None,
        }
    }
}

/// ApplyProfileResult.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApplyResult {
    pub name: Option<String>,
    pub devices_applied: usize,
    pub apps_applied: usize,
    pub apps_missing: usize,
    pub apps_routed: usize,
    /// Role, device id, display name.
    pub defaults_applied: Vec<(Role, String, Option<String>)>,
    pub devices_missing: Vec<String>,
    pub warnings: Vec<String>,
    /// Apps that were routed: their state is applied again three times,
    /// 250 ms apart, once the move has settled.
    pub settle: Vec<AppVolumeEntry>,
}

pub fn role_of(role: Role) -> DefaultDeviceRole {
    match role {
        Role::Playback => DefaultDeviceRole::PLAYBACK,
        Role::PlaybackCommunications => DefaultDeviceRole::PLAYBACK_COMMUNICATIONS,
        Role::Recording => DefaultDeviceRole::RECORDING,
        Role::RecordingCommunications => DefaultDeviceRole::RECORDING_COMMUNICATIONS,
    }
}

fn slot(set: &mut DefaultDeviceSet, role: Role) -> &mut Option<DefaultDeviceEntry> {
    match role {
        Role::Playback => &mut set.playback,
        Role::PlaybackCommunications => &mut set.playback_communications,
        Role::Recording => &mut set.recording,
        Role::RecordingCommunications => &mut set.recording_communications,
    }
}

/// DefaultDeviceSet.Set: entries without a device id clear the slot.
fn set_slot(set: &mut DefaultDeviceSet, role: Role, entry: Option<DefaultDeviceEntry>) {
    *slot(set, role) = entry.filter(|e| !is_blank(e.device_id.as_deref()));
}

/// `DateTime.Now.ToString("yyyy-MM-dd HH:mm")`.
pub fn now_text() -> String {
    // Milliseconds since 0001-01-01 on the local wall clock.
    let ms = clock::local_now_ms();
    let minutes = ms.div_euclid(60_000);
    let days = minutes.div_euclid(1440);
    let minute = minutes.rem_euclid(1440);
    let (y, m, d) = civil(days - 719_162);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", minute / 60, minute % 60)
}

/// Year, month, day of a day count from 1970-01-01 (Howard Hinnant).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `Guid.NewGuid().ToString("N")`.
pub fn new_id() -> String {
    let guid = windows::core::GUID::new().unwrap_or_default();
    format!("{:032x}", guid.to_u128())
}

fn percent(volume: f32) -> i32 {
    (volume * 100.0).round() as i32
}

/// The stored list, without persisting (ParseProfiles(json): hotkey
/// registration and listings only read).
pub fn read(store: &Store) -> Vec<VolumeProfile> {
    let json = store.text(Key::VolumeProfilesJson.storage_key());
    profiles::parse_legacy(json.as_deref().unwrap_or("[]"), &WindowsZone, &mut new_id)
}

pub struct Service<'a> {
    pub core: &'a mut Core,
    list: ProfileCollection,
}

impl<'a> Service<'a> {
    /// LoadProfiles: presets without an id get one, persisted once.
    pub fn open(core: &'a mut Core) -> Self {
        let json = core.store.text(Key::VolumeProfilesJson.storage_key());
        let parsed = profiles::parse(json.as_deref().unwrap_or("[]"), &WindowsZone, &mut new_id)
            .unwrap_or_default();
        let mut service = Self {
            core,
            list: ProfileCollection::new(parsed.profiles),
        };
        if parsed.assigned_ids {
            service.persist();
        }
        service
    }

    pub fn profiles(&self) -> &[VolumeProfile] {
        self.list.profiles()
    }

    fn persist(&mut self) {
        if let Ok(json) = self.list.to_json() {
            self.core
                .store
                .set(Key::VolumeProfilesJson, Value::Text(Some(json)));
        }
    }

    // ---------- capture ----------

    /// CaptureCurrentState: the legacy capture (CLI, classic settings): the
    /// default or every device with the apps on it, routed back on apply.
    pub fn capture_current_state(&mut self, name: &str, scope: CaptureScope) -> VolumeProfile {
        let all = scope == CaptureScope::ALL_DEVICES;
        let mut profile = self.capture(
            name,
            &CaptureOptions {
                all_devices: all,
                apps_from_all_devices: all,
                route_apps: true,
                ..CaptureOptions::default()
            },
        );
        profile.capture_scope = scope;
        profile
    }

    pub fn capture(&mut self, name: &str, options: &CaptureOptions) -> VolumeProfile {
        let now = now_text();
        let mut profile = VolumeProfile {
            id: Some(new_id()),
            name: Some(name.into()),
            slug: Some(profile_slug(Some(name))),
            created_at: Some(now.clone()),
            updated_at: Some(now),
            capture_scope: if options.all_devices {
                CaptureScope::ALL_DEVICES
            } else {
                CaptureScope::CURRENT_DEVICE
            },
            include_device_volumes: options.include_device_volumes,
            include_app_volumes: options.include_app_volumes,
            route_apps: options.route_apps,
            ..VolumeProfile::default()
        };
        let snapshot = self.core.audio.snapshot().unwrap_or_default();
        let ids = volume_ids(&snapshot, options);
        fill_devices(
            &mut profile,
            &snapshot,
            &ids,
            options.include_app_volumes,
            options.apps_from_all_devices,
        );
        if let Some(roles) = &options.default_roles {
            let set = profile.default_devices.get_or_insert_default();
            let mut seen = vec![];
            for &role in roles {
                if !seen.contains(&role) {
                    seen.push(role);
                    set_slot(set, role, self.read_default(role));
                }
            }
        }
        profile
    }

    /// `DefaultDevices.Set(role, ReadCurrentDefault(role))` (save --defaults).
    pub fn capture_default(&self, profile: &mut VolumeProfile, role: Role) {
        let current = self.read_default(role);
        set_slot(
            profile.default_devices.get_or_insert_default(),
            role,
            current,
        );
    }

    fn read_default(&self, role: Role) -> Option<DefaultDeviceEntry> {
        let endpoint = self.core.audio.default_endpoint(role).ok().flatten()?;
        Some(DefaultDeviceEntry {
            device_id: Some(endpoint.id),
            display_name: Some(endpoint.name),
        })
    }

    /// UpdateFromCurrentState: new values, same volume devices, every open
    /// app and the same default-device slots; missing devices keep theirs.
    pub fn update_from_current_state(&mut self, index: usize) -> bool {
        let snapshot = self.core.audio.snapshot().unwrap_or_default();
        let defaults: Vec<(Role, Option<DefaultDeviceEntry>)> = Role::ALL
            .iter()
            .map(|&role| (role, self.read_default(role)))
            .collect();
        let Some(profile) = self.list.get_mut(index) else {
            return false;
        };
        let ids: Vec<String> = profile
            .volume_devices()
            .filter_map(|d| d.device_id.clone())
            .filter(|id| !is_blank(Some(id)))
            .collect();
        let stale: Vec<DeviceVolumeEntry> = profile
            .volume_devices()
            .filter(|d| find_device(&snapshot, d).is_none())
            .cloned()
            .collect();
        let capture_apps = profile.include_app_volumes || profile.all_apps().next().is_some();
        profile.devices = Some(vec![]);
        fill_devices(profile, &snapshot, &ids, capture_apps, true);
        let devices = profile.devices.get_or_insert_default();
        for mut missing in stale {
            missing.apps = Some(vec![]);
            devices.push(Some(missing));
        }
        let set = profile.default_devices.get_or_insert_default();
        for (role, current) in defaults {
            if set.get(role_of(role)).is_some() {
                if let Some(current) = current {
                    set_slot(set, role, Some(current));
                }
            }
        }
        profile.schema_version = bt_core::models::CURRENT_PROFILE_SCHEMA;
        self.list.commit_edit(index, &now_text());
        self.persist();
        true
    }

    // ---------- apply ----------

    pub fn apply(&mut self, index: usize) -> ApplyResult {
        let Some(profile) = self.list.profiles().get(index).cloned() else {
            return ApplyResult::default();
        };
        let result = apply(self.core, &profile);
        if let Some(id) = profile.id.filter(|id| !is_blank(Some(id))) {
            self.core
                .store
                .set(Key::QuickTrumpetLastAppliedId, Value::Text(Some(id)));
        }
        result
    }

    // ---------- lookup ----------

    pub fn find(&self, name_or_slug: &str) -> Option<usize> {
        self.list.find(name_or_slug)
    }
    pub fn find_by_id(&self, id: &str) -> Option<usize> {
        self.list.find_by_id(id)
    }
    /// GetCycleTarget: list order, wrapping, from the last applied preset.
    pub fn cycle_target(&self, direction: i32) -> Option<usize> {
        let last = self
            .core
            .store
            .text(Key::QuickTrumpetLastAppliedId.storage_key())
            .unwrap_or_default();
        self.list.cycle_target(&last, direction)
    }

    // ---------- editing ----------

    /// SaveProfile: replaces a preset of the same name (keeping its id,
    /// creation time and shortcut) or adds it.
    pub fn save(&mut self, profile: VolumeProfile) -> usize {
        let index = self.list.save_replacement(profile, &now_text());
        self.persist();
        index
    }
    /// Persists an in-place edit made through `edit`.
    pub fn edit(&mut self, index: usize, change: impl FnOnce(&mut VolumeProfile)) -> bool {
        let Some(profile) = self.list.get_mut(index) else {
            return false;
        };
        change(profile);
        self.list.commit_edit(index, &now_text());
        self.persist();
        true
    }
    pub fn delete(&mut self, index: usize) -> bool {
        let deleted = self.list.delete(index);
        if deleted {
            self.persist();
        }
        deleted
    }
    /// RenameProfile: keeps the slug and the update time.
    pub fn rename(&mut self, index: usize, name: &str) -> bool {
        let renamed = self.list.rename(index, Some(name.into()));
        if renamed {
            self.persist();
        }
        renamed
    }
    pub fn set_device_entry(
        &mut self,
        index: usize,
        device_id: &str,
        volume: Option<i32>,
        muted: Option<bool>,
    ) -> bool {
        let now = now_text();
        let changed = self
            .list
            .get_mut(index)
            .is_some_and(|p| p.set_device_entry(device_id, volume, muted, &now));
        if changed {
            self.persist();
        }
        changed
    }
    pub fn remove_device_entry(&mut self, index: usize, device_id: &str) -> bool {
        let now = now_text();
        let changed = self
            .list
            .get_mut(index)
            .is_some_and(|p| p.remove_device_entry(device_id, &now));
        if changed {
            self.persist();
        }
        changed
    }
    /// AddDeviceEntry: adds or re-includes an output at its current volume.
    pub fn add_device_entry(&mut self, index: usize, device_id: &str) -> bool {
        let devices = self.core.audio.devices().unwrap_or_default();
        let Some(device) = devices
            .devices
            .iter()
            .find(|d| ordinal_ignore_case(&d.id, device_id))
        else {
            return false;
        };
        let (id, name, volume, muted) = (
            device.id.clone(),
            device.name.clone(),
            percent(device.volume),
            device.muted,
        );
        self.edit(index, |profile| {
            let list = profile.devices.get_or_insert_default();
            let position = list.iter().position(|d| {
                d.as_ref()
                    .and_then(|d| d.device_id.as_deref())
                    .is_some_and(|key| ordinal_ignore_case(key, device_id))
            });
            let entry = match position {
                Some(i) => list[i].as_mut().expect("matched entry"),
                None => {
                    list.push(Some(DeviceVolumeEntry {
                        device_id: Some(id),
                        ..DeviceVolumeEntry::default()
                    }));
                    list.last_mut().and_then(Option::as_mut).expect("pushed")
                }
            };
            entry.display_name = Some(name);
            entry.volume = volume;
            entry.is_muted = muted;
            entry.include_volume = true;
            profile.include_device_volumes = true;
        })
    }
    pub fn set_app_entry(
        &mut self,
        index: usize,
        app_key: &str,
        volume: Option<i32>,
        muted: Option<bool>,
    ) -> bool {
        let now = now_text();
        let changed = self
            .list
            .get_mut(index)
            .is_some_and(|p| p.set_app_entry(app_key, volume, muted, &now));
        if changed {
            self.persist();
        }
        changed
    }
    pub fn remove_app_entry(&mut self, index: usize, app_key: &str) -> bool {
        let now = now_text();
        let changed = self
            .list
            .get_mut(index)
            .is_some_and(|p| p.remove_app_entry(app_key, &now));
        if changed {
            self.persist();
        }
        changed
    }
    /// SetDefaultDevice: an empty id clears the slot ("don't change").
    pub fn set_default_device(&mut self, index: usize, role: Role, device_id: &str) -> bool {
        let name = (!is_blank(Some(device_id)))
            .then(|| {
                self.core
                    .audio
                    .endpoints(role.is_capture())
                    .unwrap_or_default()
                    .into_iter()
                    .find(|e| ordinal_ignore_case(&e.id, device_id))
                    .map(|e| e.name)
            })
            .flatten();
        self.edit(index, |profile| {
            let set = profile.default_devices.get_or_insert_default();
            if is_blank(Some(device_id)) {
                set_slot(set, role, None);
                return;
            }
            let previous = set.get(role_of(role)).and_then(|e| e.display_name.clone());
            let name = name.or(previous).unwrap_or_else(|| device_id.into());
            set_slot(
                set,
                role,
                Some(DefaultDeviceEntry {
                    device_id: Some(device_id.into()),
                    display_name: Some(name),
                }),
            );
        })
    }
    /// ExportProfile: one preset as indented JSON.
    pub fn export(&self, index: usize) -> Option<String> {
        serde_json::to_string_pretty(self.list.profiles().get(index)?).ok()
    }
    /// ImportProfile: a fresh id if it collides, " (Imported)" if the name
    /// is taken.
    pub fn import(&mut self, json: &str) -> Option<usize> {
        let index = self
            .list
            .import_json(json, &WindowsZone, &mut new_id)
            .ok()
            .flatten()?;
        self.persist();
        Some(index)
    }
}

fn volume_ids(snapshot: &Snapshot, options: &CaptureOptions) -> Vec<String> {
    if options.all_devices {
        return snapshot.devices.iter().map(|d| d.id.clone()).collect();
    }
    if let Some(ids) = &options.device_ids {
        return ids
            .iter()
            .filter(|id| !is_blank(Some(id)))
            .cloned()
            .collect();
    }
    snapshot
        .devices
        .iter()
        .find(|d| d.is_default)
        .or(snapshot.devices.first())
        .map(|d| vec![d.id.clone()])
        .unwrap_or_default()
}

fn fill_devices(
    profile: &mut VolumeProfile,
    snapshot: &Snapshot,
    volume_ids: &[String],
    include_apps: bool,
    apps_from_all_devices: bool,
) {
    let devices = profile.devices.get_or_insert_default();
    for device in &snapshot.devices {
        let has_volume = volume_ids
            .iter()
            .any(|id| ordinal_ignore_case(id, &device.id));
        let take_apps = include_apps && (apps_from_all_devices || has_volume);
        let apps: Vec<Option<AppVolumeEntry>> = if take_apps {
            device
                .apps
                .iter()
                .map(|app| {
                    Some(AppVolumeEntry {
                        exe_name: Some(rules::exe_name(app)),
                        app_id: Some(rules::app_id(app)),
                        display_name: Some(app.display_name.clone()),
                        device_id: Some(device.id.clone()),
                        device_display_name: Some(device.name.clone()),
                        volume: percent(app.volume),
                        is_muted: app.muted,
                    })
                })
                .collect()
        } else {
            vec![]
        };
        if !has_volume && apps.is_empty() {
            continue;
        }
        devices.push(Some(DeviceVolumeEntry {
            device_id: Some(device.id.clone()),
            display_name: Some(device.name.clone()),
            volume: percent(device.volume),
            is_muted: device.muted,
            include_volume: has_volume,
            apps: Some(apps),
        }));
    }
}

/// FindDevice: by id, then by display name.
fn find_device<'s>(
    snapshot: &'s Snapshot,
    saved: &DeviceVolumeEntry,
) -> Option<&'s bt_audio::Device> {
    snapshot
        .devices
        .iter()
        .find(|d| saved.device_id.as_deref() == Some(d.id.as_str()))
        .or_else(|| {
            snapshot
                .devices
                .iter()
                .find(|d| saved.display_name.as_deref() == Some(d.name.as_str()))
        })
}

fn matches(saved: &AppVolumeEntry, app: &bt_audio::AppSession) -> bool {
    let same = |saved: Option<&str>, current: String| {
        saved.is_some_and(|s| !s.is_empty() && ordinal_ignore_case(s, &current))
    };
    same(saved.exe_name.as_deref(), rules::exe_name(app))
        || same(saved.app_id.as_deref(), rules::app_id(app))
}

/// ApplyAppState: every session of the app on any output takes the saved
/// volume and mute. Returns whether any matched.
pub fn apply_app_state(core: &Core, saved: &AppVolumeEntry) -> bool {
    let snapshot = core.audio.snapshot().unwrap_or_default();
    let mut found = false;
    for device in &snapshot.devices {
        for app in device.apps.iter().filter(|a| matches(saved, a)) {
            found = true;
            let _ = core
                .audio
                .set_app_volume(&device.id, &app.key, saved.volume as f32 / 100.0);
            let _ = core
                .audio
                .set_app_mute(&device.id, &app.key, saved.is_muted);
        }
    }
    found
}

fn add_missing(result: &mut ApplyResult, name: Option<&str>) {
    let name = name
        .filter(|n| !is_blank(Some(n)))
        .unwrap_or("?")
        .to_owned();
    if !result.devices_missing.contains(&name) {
        result.devices_missing.push(name.clone());
    }
    result.warnings.push(format!("Device not found: {name}"));
}

/// ApplyProfile: default devices first, then device volumes, then apps
/// (routed back to their device when RouteApps). Not recorded for undo.
pub fn apply(core: &mut Core, profile: &VolumeProfile) -> ApplyResult {
    let mut result = ApplyResult {
        name: profile.name.clone(),
        ..ApplyResult::default()
    };
    if let Some(set) = profile.default_devices.as_ref().filter(|s| !s.is_empty()) {
        for role in Role::ALL {
            let Some(entry) = set.get(role_of(role)) else {
                continue;
            };
            let id = entry.device_id.clone().unwrap_or_default();
            if !core.audio.is_active(&id).unwrap_or(false) {
                add_missing(
                    &mut result,
                    entry.display_name.as_deref().or(Some(id.as_str())),
                );
                continue;
            }
            if core.audio.set_default_role(&id, role).is_ok() {
                result
                    .defaults_applied
                    .push((role, id, entry.display_name.clone()));
            } else {
                result.warnings.push(format!(
                    "Windows did not switch the {role:?} default to {}",
                    entry.display_name.as_deref().unwrap_or_default()
                ));
            }
        }
    }
    let snapshot = core.audio.snapshot().unwrap_or_default();
    if profile.include_device_volumes {
        for saved in profile.volume_devices() {
            let Some(device) = find_device(&snapshot, saved) else {
                add_missing(&mut result, saved.display_name.as_deref());
                continue;
            };
            let _ = core
                .audio
                .set_device_volume(&device.id, saved.volume as f32 / 100.0);
            let _ = core.audio.set_device_mute(&device.id, saved.is_muted);
            result.devices_applied += 1;
        }
    }
    if profile.include_app_volumes {
        for saved_device in profile.devices.iter().flatten().flatten() {
            let target = find_device(&snapshot, saved_device).map(|d| d.id.clone());
            for saved in saved_device.apps.iter().flatten().flatten() {
                let mut routed = 0;
                if let (true, Some(target)) = (profile.route_apps, &target) {
                    // TryRouteApp: every matching app on any output.
                    let mut keys: Vec<&str> = vec![];
                    for device in &snapshot.devices {
                        for app in device.apps.iter().filter(|a| matches(saved, a)) {
                            routed += 1;
                            if !keys.contains(&app.key.as_str()) {
                                keys.push(&app.key);
                                let _ = core.audio.set_app_device(&app.key, Some(target));
                            }
                        }
                    }
                    result.apps_routed += routed;
                }
                if apply_app_state(core, saved) {
                    result.apps_applied += 1;
                    if routed > 0 {
                        result.settle.push(saved.clone());
                    }
                } else {
                    result.apps_missing += 1;
                    result.warnings.push(format!(
                        "App not found: {} ({})",
                        saved.display_name.as_deref().unwrap_or_default(),
                        saved.exe_name.as_deref().unwrap_or_default()
                    ));
                }
            }
        }
    }
    result
}

// ---------- QuickTrumpetSummary ----------

const SEPARATOR: &str = " · ";

fn count_devices(n: usize) -> String {
    if n == 1 {
        tr("QuickTrumpetSummaryDeviceOne").into()
    } else {
        trf("QuickTrumpetSummaryDevicesFormat", &[("0", &n.to_string())])
    }
}

fn count_apps(n: usize) -> String {
    if n == 1 {
        tr("QuickTrumpetSummaryAppOne").into()
    } else {
        trf("QuickTrumpetSummaryAppsFormat", &[("0", &n.to_string())])
    }
}

/// QuickTrumpetToastTitleFormat.
pub fn toast_title(name: &str) -> String {
    trf("QuickTrumpetToastTitleFormat", &[("0", name)])
}

/// DescribeResult: "Output → Speakers · 2 devices · 3 apps", plus a
/// "Not connected" line.
pub fn describe_result(result: &ApplyResult) -> String {
    let main = |role: Role| {
        result
            .defaults_applied
            .iter()
            .find(|(r, _, _)| *r == role)
            .map(|(_, id, _)| id.as_str())
    };
    let mut parts = vec![];
    for (role, id, name) in &result.defaults_applied {
        let base = match role {
            Role::PlaybackCommunications => main(Role::Playback),
            Role::RecordingCommunications => main(Role::Recording),
            _ => None,
        };
        if base.is_some_and(|b| ordinal_ignore_case(b, id)) {
            continue;
        }
        let format = match role {
            Role::Playback => "QuickTrumpetSummaryPlaybackFormat",
            Role::PlaybackCommunications => "QuickTrumpetSummaryPlaybackCommsFormat",
            Role::Recording => "QuickTrumpetSummaryRecordingFormat",
            Role::RecordingCommunications => "QuickTrumpetSummaryRecordingCommsFormat",
        };
        parts.push(trf(format, &[("0", name.as_deref().unwrap_or_default())]));
    }
    if result.devices_applied > 0 {
        parts.push(count_devices(result.devices_applied));
    }
    if result.apps_applied > 0 {
        parts.push(count_apps(result.apps_applied));
    }
    let mut text = if parts.is_empty() {
        tr("QuickTrumpetSummaryNothing").into()
    } else {
        parts.join(SEPARATOR)
    };
    if !result.devices_missing.is_empty() {
        text.push('\n');
        text += &trf(
            "QuickTrumpetSummaryMissingFormat",
            &[("0", &result.devices_missing.join(", "))],
        );
    }
    text
}

/// DescribeProfile: "Speakers + Desk mic · 2 devices · 3 apps".
pub fn describe_profile(profile: &VolumeProfile) -> String {
    let mut parts = vec![];
    let mut defaults: Vec<String> = vec![];
    if let Some(set) = &profile.default_devices {
        for role in Role::ALL {
            let name = set.get(role_of(role)).and_then(|e| e.display_name.clone());
            if let Some(name) = name.filter(|n| !is_blank(Some(n))) {
                if !defaults.iter().any(|d| ordinal_ignore_case(d, &name)) {
                    defaults.push(name);
                }
            }
        }
    }
    if !defaults.is_empty() {
        parts.push(if defaults.len() <= 2 {
            defaults.join(" + ")
        } else {
            format!("{} + {} +{}", defaults[0], defaults[1], defaults.len() - 2)
        });
    }
    let devices = if profile.include_device_volumes {
        profile.volume_devices().count()
    } else {
        0
    };
    if devices > 0 {
        parts.push(count_devices(devices));
    }
    let apps = if profile.include_app_volumes {
        let mut keys: Vec<String> = vec![];
        for app in profile.all_apps() {
            let key = if is_blank(app.exe_name.as_deref()) {
                app.app_id.clone()
            } else {
                app.exe_name.clone()
            };
            if let Some(key) = key.filter(|k| !is_blank(Some(k))) {
                if !keys.iter().any(|k| ordinal_ignore_case(k, &key)) {
                    keys.push(key);
                }
            }
        }
        keys.len()
    } else {
        0
    };
    if apps > 0 {
        parts.push(count_apps(apps));
    }
    if parts.is_empty() {
        tr("QuickTrumpetSummaryEmpty").into()
    } else {
        parts.join(SEPARATOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_754), (2026, 10, 28));
        assert_eq!(civil(-719_162), (1, 1, 1));
    }

    #[test]
    fn result_text_follows_quicktrumpet_summary() {
        let result = ApplyResult {
            defaults_applied: vec![
                (Role::Playback, "a".into(), Some("Speakers".into())),
                (
                    Role::PlaybackCommunications,
                    "A".into(),
                    Some("Speakers".into()),
                ),
                (Role::Recording, "m".into(), Some("Desk mic".into())),
            ],
            devices_applied: 2,
            apps_applied: 1,
            devices_missing: vec!["Headset".into()],
            ..ApplyResult::default()
        };
        assert_eq!(
            describe_result(&result),
            "Output → Speakers · Mic → Desk mic · 2 devices · 1 app\nNot connected: Headset"
        );
        assert_eq!(
            describe_result(&ApplyResult::default()),
            "Nothing to change"
        );
    }

    #[test]
    fn profile_text_counts_distinct_apps() {
        let app = |exe: &str| {
            Some(AppVolumeEntry {
                exe_name: Some(exe.into()),
                ..AppVolumeEntry::default()
            })
        };
        let profile = VolumeProfile {
            devices: Some(vec![Some(DeviceVolumeEntry {
                device_id: Some("d".into()),
                apps: Some(vec![app("spotify"), app("Spotify"), app("chrome")]),
                ..DeviceVolumeEntry::default()
            })]),
            ..VolumeProfile::default()
        };
        assert_eq!(describe_profile(&profile), "1 device · 2 apps");
        assert_eq!(
            describe_profile(&VolumeProfile {
                devices: Some(vec![]),
                ..VolumeProfile::default()
            }),
            "Empty preset"
        );
    }
}
