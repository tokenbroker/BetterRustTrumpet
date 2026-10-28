//! The settings window's list pages: QuickTrumpet presets with their
//! capture form, app rules, folder defaults and device shortcuts, and what
//! their controls do. Labels and behaviour follow the original's web pages
//! (SettingsPages.tsx ProfilesPage/RulesPage/ShortcutsPage) and the host's
//! profile*, appRule* and folderRule* actions (WebSettingsWindow, BRIDGE.md).
use super::pages::{
    Action, Button, CaptureForm, Choose, Control, Expand, Field, HotkeyTarget, Include, PageState,
    Row, Section, Set, Texts, Unit, button, hotkey_text, note, row, section,
};
use crate::{
    core::Core,
    folders,
    profiles::{self, CaptureOptions},
    rules,
    store::with_env,
};
use bt_app::i18n::{tr, trf};
use bt_app::win32::dialog;
use bt_audio::{Endpoint, Role, Snapshot};
use bt_core::{
    app_rules::{Metadata, Mutation},
    identity::normalize_exe_name,
    models::{AppRuleEntry, HotkeyData, VolumeProfile, VolumeRuleMode},
    profiles as stored,
    settings::{Key, Value},
    text::{is_blank, ordinal_ignore_case},
};

/// ROLE_LABELS (resource keys), in DefaultDeviceRole order.
const ROLE_LABELS: [&str; 4] = [
    "QuickTrumpetRolePlayback",
    "QuickTrumpetRolePlaybackComms",
    "QuickTrumpetRoleRecording",
    "QuickTrumpetRoleRecordingComms",
];

fn profile_filters() -> [dialog::Filter<'static>; 2] {
    [
        (tr("FileFilterBtProfile"), "*.btprofile"),
        (tr("FileFilterJson"), "*.json"),
    ]
}

// Segoe Fluent Icons.
const GLYPH_SAVE: &str = "\u{E74E}";
const GLYPH_IMPORT: &str = "\u{E898}";
const GLYPH_EXPORT: &str = "\u{E896}";
const GLYPH_REFRESH: &str = "\u{E72C}";
const GLYPH_DELETE: &str = "\u{E74D}";
const GLYPH_REMOVE: &str = "\u{E711}";
const GLYPH_VOLUME: &str = "\u{E767}";
const GLYPH_MUTED: &str = "\u{E74F}";
const GLYPH_ADD: &str = "\u{E710}";
const GLYPH_FOLDER: &str = "\u{E8B7}";
const GLYPH_NEW_FOLDER: &str = "\u{E8F4}";
const GLYPH_CHOOSE: &str = "\u{E70D}";

/// What a list action asks of the window and the app.
#[derive(Default)]
pub struct Effect {
    /// VolumeProfilesJson changed: preset shortcuts register again.
    pub presets: bool,
    /// Apply this preset (and show its confirmation).
    pub apply: Option<usize>,
}

fn role_index(role: Role) -> usize {
    Role::ALL.iter().position(|r| *r == role).unwrap_or(0)
}

/// The live audio the QuickTrumpet page captures from (BuildAudioState).
struct Audio {
    snapshot: Snapshot,
    playback: Vec<Endpoint>,
    recording: Vec<Endpoint>,
    /// The current default of each role.
    defaults: [Option<Endpoint>; 4],
}

fn audio(core: &Core) -> Audio {
    let a = &core.audio;
    Audio {
        snapshot: a.snapshot().unwrap_or_default(),
        playback: a.endpoints(false).unwrap_or_default(),
        recording: a.endpoints(true).unwrap_or_default(),
        defaults: Role::ALL.map(|role| a.default_endpoint(role).ok().flatten()),
    }
}

impl Audio {
    /// QuickTrumpetSummary.CountDistinctApps over the open apps.
    fn app_count(&self) -> usize {
        let mut keys: Vec<String> = vec![];
        for app in self.snapshot.devices.iter().flat_map(|d| &d.apps) {
            let exe = rules::exe_name(app);
            let key = if is_blank(Some(&exe)) {
                rules::app_id(app)
            } else {
                exe
            };
            if !is_blank(Some(&key)) && !keys.iter().any(|k| ordinal_ignore_case(k, &key)) {
                keys.push(key);
            }
        }
        keys.len()
    }
}

fn percent(volume: f32) -> i32 {
    (volume * 100.0).round() as i32
}

/// The capture form's checked outputs: the picked ones still connected,
/// or the default output.
fn selected_ids(form: &CaptureForm, audio: &Audio) -> Vec<String> {
    let devices = &audio.snapshot.devices;
    let ids: Vec<String> = match &form.device_ids {
        Some(ids) => ids.clone(),
        None => devices
            .iter()
            .filter(|d| d.is_default)
            .map(|d| d.id.clone())
            .collect(),
    };
    ids.into_iter()
        .filter(|id| devices.iter().any(|d| d.id == *id))
        .collect()
}

/// The checked roles that have a current default.
fn selected_roles(form: &CaptureForm, audio: &Audio) -> Vec<usize> {
    form.roles
        .iter()
        .copied()
        .filter(|&r| audio.defaults[r].is_some())
        .collect()
}

/// The list key of a preset (keyOf: id, slug or name).
pub fn preset_key(profile: &VolumeProfile) -> String {
    [&profile.id, &profile.slug, &profile.name]
        .into_iter()
        .flatten()
        .find(|k| !k.is_empty())
        .cloned()
        .unwrap_or_default()
}

fn check(label: &str, desc: Option<&str>, on: bool, set: Set) -> Row {
    row(label, desc, Control::Check(on, set))
}

fn indented(mut row: Row) -> Row {
    row.indent = true;
    row
}

fn heading(label: &str) -> Row {
    indented(row(label, None, Control::Heading))
}

fn volume(value: i32, set: Set) -> Control {
    Control::Slider {
        value: f64::from(value),
        min: 0.0,
        max: 100.0,
        step: 1.0,
        unit: Unit::Percent,
        set,
    }
}

fn icon(glyph: &'static str, action: Action) -> Button {
    button("", Some(glyph), action)
}

// ---------- QuickTrumpet ----------

/// The Presets card: the capture form, then every preset with its details
/// under it when open.
pub fn presets(core: &mut Core, state: &PageState) -> Vec<Row> {
    let audio = audio(core);
    let list = profiles::read(&core.store);
    let last = core
        .store
        .text(Key::QuickTrumpetLastAppliedId.storage_key())
        .unwrap_or_default();
    let mut rows = vec![];
    if list.is_empty() {
        rows.push(note(tr("WebSettingsQuickTrumpetEmptyHint")));
    }
    rows.extend(capture_form(&state.capture, &audio));
    for (index, profile) in list.iter().enumerate() {
        let key = preset_key(profile);
        let open = state.expanded == Some(Expand::Preset(key.clone()));
        let last_applied = profile
            .id
            .as_deref()
            .is_some_and(|id| !id.is_empty() && ordinal_ignore_case(id, &last));
        rows.push(Row {
            badge: last_applied.then(|| tr("WebSettingsQtLastApplied").into()),
            trail: vec![button(
                tr("SettingsProfileApply"),
                None,
                Action::PresetApply(index),
            )],
            expand: Some((Action::PresetToggle(Expand::Preset(key)), open)),
            ..row(
                profile.name.as_deref().unwrap_or_default(),
                Some(&profiles::describe_profile(profile)),
                Control::Hotkey {
                    target: HotkeyTarget::Preset(index),
                    text: profile.hotkey.as_ref().map(hotkey_text).unwrap_or_default(),
                    fallback: None,
                },
            )
        });
        if open {
            rows.extend(preset_details(index, profile, &audio));
        }
    }
    rows
}

fn capture_form(form: &CaptureForm, audio: &Audio) -> Vec<Row> {
    let ids = selected_ids(form, audio);
    let roles = selected_roles(form, audio);
    let has_devices = form.device_volumes && !ids.is_empty();
    let has_defaults = form.defaults && !roles.is_empty();
    let nothing = !has_devices && !form.app_volumes && !has_defaults;
    let mut save = button(
        tr("WebSettingsSave"),
        Some(GLYPH_SAVE),
        Action::PresetCapture,
    );
    save.primary = !nothing;
    let mut rows = vec![
        row(
            "",
            None,
            Control::Input {
                field: Field::Preset,
                placeholder: tr("WebSettingsPresetName"),
                buttons: vec![
                    save,
                    button(
                        tr("SettingsProfileImport"),
                        Some(GLYPH_IMPORT),
                        Action::PresetImport,
                    ),
                ],
            },
        ),
        row(tr("WebSettingsQtWhatToSave"), None, Control::Heading),
    ];
    let choose = |which| {
        vec![button(
            tr("WebSettingsQtChoose"),
            Some(GLYPH_CHOOSE),
            Action::CaptureChoose(which),
        )]
    };
    // Default devices.
    let names: Vec<&str> = audio
        .defaults
        .iter()
        .enumerate()
        .filter(|(r, _)| roles.contains(r) && (*r == 0 || *r == 2))
        .filter_map(|(_, e)| e.as_ref().map(|e| e.name.as_str()))
        .collect();
    rows.push(Row {
        chips: if form.defaults && !names.is_empty() {
            vec![names.join(" + ")]
        } else {
            vec![]
        },
        trail: choose(Choose::Defaults),
        ..check(
            tr("WebSettingsQtIncludeDefaults"),
            Some(tr("WebSettingsQtIncludeDefaultsDesc")),
            form.defaults,
            Set::CaptureDefaults,
        )
    });
    if form.defaults && form.open == Some(Choose::Defaults) {
        for (r, label) in ROLE_LABELS.map(tr).iter().enumerate() {
            let current = audio.defaults[r].as_ref();
            rows.push(Row {
                enabled: current.is_some(),
                indent: true,
                ..check(
                    label,
                    Some(current.map_or(tr("WebSettingsQtNoDevice"), |e| e.name.as_str())),
                    current.is_some() && form.roles.contains(&r),
                    Set::CaptureRole(r),
                )
            });
        }
    }
    // Device volumes.
    let devices = &audio.snapshot.devices;
    rows.push(Row {
        chips: if form.device_volumes {
            vec![trf(
                "WebSettingsQtDevicesCheckedFormat",
                &[
                    ("0", &ids.len().to_string()),
                    ("1", &devices.len().to_string()),
                ],
            )]
        } else {
            vec![]
        },
        trail: choose(Choose::Devices),
        ..check(
            tr("WebSettingsQtIncludeDevices"),
            Some(tr("WebSettingsQtIncludeDevicesDesc")),
            form.device_volumes,
            Set::CaptureDevices,
        )
    });
    if form.device_volumes && form.open == Some(Choose::Devices) {
        for device in devices {
            rows.push(Row {
                badge: device
                    .is_default
                    .then(|| tr("WebSettingsDefaultDeviceBadge").into()),
                chips: vec![if device.muted {
                    tr("WebSettingsQtMuteEntry").into()
                } else {
                    format!("{}%", percent(device.volume))
                }],
                indent: true,
                ..check(
                    &device.name,
                    None,
                    ids.contains(&device.id),
                    Set::CaptureDevice(device.id.clone()),
                )
            });
        }
    }
    // App volumes.
    rows.push(Row {
        chips: if form.app_volumes {
            vec![trf(
                "WebSettingsQtAppsOpenFormat",
                &[("0", &audio.app_count().to_string())],
            )]
        } else {
            vec![]
        },
        ..check(
            tr("WebSettingsQtIncludeApps"),
            Some(tr("WebSettingsQtIncludeAppsDesc")),
            form.app_volumes,
            Set::CaptureApps,
        )
    });
    if form.app_volumes {
        rows.push(indented(check(
            tr("WebSettingsQtRouteApps"),
            Some(tr("WebSettingsQtRouteAppsDesc")),
            form.route_apps,
            Set::CaptureRoute,
        )));
    }
    if nothing {
        rows.push(note(tr("WebSettingsQtNothingSelected")));
    }
    rows
}

/// PresetDetails: default devices, device and app volumes, rename and the
/// preset's actions.
fn preset_details(index: usize, profile: &VolumeProfile, audio: &Audio) -> Vec<Row> {
    let mut rows = vec![heading(tr("WebSettingsQtIncludeDefaults"))];
    for role in Role::ALL {
        let entry = profile
            .default_devices
            .as_ref()
            .and_then(|s| s.get(profiles::role_of(role)));
        let endpoints = if role.is_capture() {
            &audio.recording
        } else {
            &audio.playback
        };
        let mut options = vec![tr("WebSettingsQtDontChange").to_string()];
        let mut ids = vec![String::new()];
        for e in endpoints {
            options.push(e.name.clone());
            ids.push(e.id.clone());
        }
        let id = entry.and_then(|e| e.device_id.clone()).unwrap_or_default();
        if !id.is_empty() && !endpoints.iter().any(|e| e.id == id) {
            let name = entry
                .and_then(|e| e.display_name.clone())
                .unwrap_or_default();
            options.push(format!("{name} · {}", tr("WebSettingsQtNotConnected")));
            ids.push(id.clone());
        }
        let selected = ids.iter().position(|i| *i == id).unwrap_or(0);
        rows.push(indented(row(
            tr(ROLE_LABELS[role_index(role)]),
            None,
            Control::Choice {
                options,
                selected,
                set: Set::PresetDefault(index, role, ids),
            },
        )));
    }
    // Device volumes.
    rows.push(indented(row(
        tr("WebSettingsQtIncludeDevices"),
        None,
        Control::Toggle(
            profile.include_device_volumes,
            Set::PresetInclude(index, Include::Devices),
        ),
    )));
    let devices: Vec<_> = profile.volume_devices().collect();
    for entry in &devices {
        let key = entry.device_id.clone().unwrap_or_default();
        let name = entry
            .display_name
            .clone()
            .or(entry.device_id.clone())
            .unwrap_or_default();
        let missing = !audio
            .playback
            .iter()
            .any(|e| ordinal_ignore_case(&e.id, &key));
        rows.push(Row {
            chips: if missing {
                vec![tr("WebSettingsQtNotConnected").into()]
            } else {
                vec![]
            },
            trail: vec![
                icon(
                    if entry.is_muted {
                        GLYPH_MUTED
                    } else {
                        GLYPH_VOLUME
                    },
                    Action::PresetDeviceMute(index, key.clone(), !entry.is_muted),
                ),
                icon(GLYPH_REMOVE, Action::PresetDeviceRemove(index, key.clone())),
            ],
            indent: true,
            ..row(
                &name,
                None,
                volume(entry.volume, Set::PresetDeviceVolume(index, key)),
            )
        });
    }
    if devices.is_empty() {
        rows.push(indented(note(tr("WebSettingsQtNoDeviceEntries"))));
    }
    let unused: Vec<_> = audio
        .snapshot
        .devices
        .iter()
        .filter(|d| {
            !devices
                .iter()
                .any(|e| e.device_id.as_deref() == Some(&d.id))
        })
        .collect();
    if !unused.is_empty() {
        let mut options = vec![format!("+ {}", tr("WebSettingsQtAddDevice"))];
        let mut ids = vec![String::new()];
        for d in unused {
            options.push(format!("{} · {}%", d.name, percent(d.volume)));
            ids.push(d.id.clone());
        }
        rows.push(indented(row(
            "",
            None,
            Control::Choice {
                options,
                selected: 0,
                set: Set::PresetAddDevice(index, ids),
            },
        )));
    }
    // App volumes, one entry per app key.
    rows.push(indented(row(
        tr("WebSettingsQtIncludeApps"),
        None,
        Control::Toggle(
            profile.include_app_volumes,
            Set::PresetInclude(index, Include::Apps),
        ),
    )));
    let mut keys: Vec<String> = vec![];
    for app in profile.all_apps() {
        let key = stored::app_key(app);
        if keys.iter().any(|k| ordinal_ignore_case(k, &key)) {
            continue;
        }
        keys.push(key.clone());
        let name = app
            .display_name
            .clone()
            .filter(|n| !is_blank(Some(n)))
            .or(app.exe_name.clone())
            .unwrap_or_default();
        rows.push(Row {
            trail: vec![
                icon(
                    if app.is_muted {
                        GLYPH_MUTED
                    } else {
                        GLYPH_VOLUME
                    },
                    Action::PresetAppMute(index, key.clone(), !app.is_muted),
                ),
                icon(GLYPH_REMOVE, Action::PresetAppRemove(index, key.clone())),
            ],
            indent: true,
            ..row(
                &name,
                app.device_display_name.as_deref(),
                volume(app.volume, Set::PresetAppVolume(index, key)),
            )
        });
    }
    if keys.is_empty() {
        rows.push(indented(note(tr("WebSettingsQtNoAppEntries"))));
    } else {
        rows.push(indented(row(
            tr("WebSettingsQtRouteApps"),
            Some(tr("WebSettingsQtRouteAppsDesc")),
            Control::Toggle(
                profile.route_apps,
                Set::PresetInclude(index, Include::Route),
            ),
        )));
    }
    rows.push(indented(row(
        tr("WebSettingsRename"),
        None,
        Control::Input {
            field: Field::Rename,
            placeholder: tr("WebSettingsPresetName"),
            buttons: vec![button(
                tr("WebSettingsRename"),
                None,
                Action::PresetRename(index),
            )],
        },
    )));
    rows.push(indented(row(
        "",
        None,
        Control::Buttons(vec![
            button(
                tr("WebSettingsQtUpdateFromCurrent"),
                Some(GLYPH_REFRESH),
                Action::PresetUpdate(index),
            )
            .confirm(tr("WebSettingsQtUpdateConfirm")),
            button(
                tr("SettingsProfileExport"),
                Some(GLYPH_EXPORT),
                Action::PresetExport(index),
            ),
            button(
                tr("SettingsProfileDelete"),
                Some(GLYPH_DELETE),
                Action::PresetDelete(index),
            )
            .confirm(tr("WebSettingsConfirmDelete")),
        ]),
    )));
    rows
}

// ---------- App rules and folder defaults ----------

/// RulesPage: Application rules, then Folder defaults.
pub fn rules(core: &mut Core, state: &PageState) -> Vec<Section> {
    let store = &mut core.store;
    let compare = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase());
    let list = with_env(|env| store.rules.sorted(env, compare));
    store.flush();
    let mut rows = vec![row(
        "",
        None,
        Control::Input {
            field: Field::Exe,
            placeholder: tr("AppRulesAddPlaceholderText"),
            buttons: vec![
                button(tr("AppRulesBrowseButtonText"), None, Action::RuleBrowse),
                button(
                    tr("AppRulesAddRuleButtonText"),
                    Some(GLYPH_ADD),
                    Action::RuleAdd,
                )
                .primary(),
            ],
        },
    )];
    for rule in &list {
        let exe = rule.exe_name.clone().unwrap_or_default();
        let open = state.expanded == Some(Expand::Rule(exe.clone()));
        let name = rule
            .display_name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| exe.clone());
        rows.push(Row {
            chips: rule_badges(rule),
            expand: Some((Action::RuleToggle(Expand::Rule(exe.clone())), open)),
            ..row(
                &name,
                Some(&format!("{exe}.exe")),
                Control::Buttons(vec![
                    icon(GLYPH_DELETE, Action::RuleRemove(exe.clone()))
                        .confirm(tr("WebSettingsConfirmDelete")),
                ]),
            )
        });
        if open {
            rows.extend(rule_details(&exe, rule));
        }
    }
    if list.is_empty() {
        rows.push(note(tr("WebSettingsEmpty")));
    } else {
        rows.push(row(
            "",
            None,
            Control::Buttons(vec![
                button(
                    tr("AppRulesClearAllButtonText"),
                    Some(GLYPH_DELETE),
                    Action::RuleClear,
                )
                .confirm(tr("WebSettingsConfirmClearAll")),
            ]),
        ));
    }
    let mut folder_rows = vec![row(
        "",
        None,
        Control::Buttons(vec![button(
            tr("FolderVolumeRulesAddButtonText"),
            Some(GLYPH_NEW_FOLDER),
            Action::FolderAdd,
        )]),
    )];
    let folders = core.folder_rules();
    for rule in &folders {
        let id = rule.id.clone().unwrap_or_default();
        let path = rule.folder_path.clone().unwrap_or_default();
        let open = state.expanded == Some(Expand::Folder(id.clone()));
        let name = path
            .split('\\')
            .rfind(|s| !s.is_empty())
            .unwrap_or(&path)
            .to_owned();
        folder_rows.push(Row {
            chips: vec![format!("{}%", rule.volume_percent)],
            expand: Some((Action::FolderToggle(Expand::Folder(id.clone())), open)),
            ..row(&name, Some(&path), Control::None)
        });
        if open {
            folder_rows.push(indented(row(
                tr("AppRulesTargetVolumeText"),
                None,
                volume(rule.volume_percent, Set::FolderVolume(id.clone())),
            )));
            folder_rows.push(indented(row(
                "",
                None,
                Control::Buttons(vec![
                    button(
                        tr("FolderVolumeRulesBrowseButtonText"),
                        Some(GLYPH_FOLDER),
                        Action::FolderBrowse(id.clone()),
                    ),
                    button(
                        tr("SettingsProfileDelete"),
                        Some(GLYPH_DELETE),
                        Action::FolderRemove(id),
                    )
                    .confirm(tr("WebSettingsConfirmDelete")),
                ]),
            )));
        }
    }
    if folders.is_empty() {
        folder_rows.push(note(tr("FolderVolumeRulesEmptyText")));
    }
    vec![
        section(
            tr("AppRulesListHeaderText"),
            Some(tr("AppRulesListHeaderDesc")),
            rows,
        ),
        section(
            tr("FolderVolumeRulesHeaderText"),
            Some(tr("FolderVolumeRulesHeaderDesc")),
            folder_rows,
        ),
    ]
}

/// RulesPage summarize: the badges of a rule's header.
fn rule_badges(rule: &AppRuleEntry) -> Vec<String> {
    let mut parts = vec![];
    if rule.hard_muted {
        parts.push(tr("AppRulesHardMuteColumnText").into());
    }
    if rule.focus_lost_enabled {
        parts.push(tr("AppRulesFocusLostColumnText").into());
    }
    if rule.volume_mode == VolumeRuleMode::LAUNCH {
        parts.push(format!(
            "{}% · {}",
            rule.volume_percent,
            tr("AppRulesModeLaunchText")
        ));
    }
    if rule.volume_mode == VolumeRuleMode::LOCK {
        parts.push(format!(
            "{}% · {}",
            rule.volume_percent,
            tr("AppRulesModeLockText")
        ));
    }
    parts
}

fn rule_details(exe: &str, rule: &AppRuleEntry) -> Vec<Row> {
    let mut rows = vec![
        indented(row(
            tr("AppRulesHardMuteColumnText"),
            None,
            Control::Toggle(rule.hard_muted, Set::RuleHardMute(exe.into())),
        )),
        indented(row(
            tr("AppRulesFocusLostColumnText"),
            None,
            Control::Toggle(rule.focus_lost_enabled, Set::RuleFocusLost(exe.into())),
        )),
        indented(row(
            tr("AppRulesVolumeBehaviorText"),
            None,
            Control::Choice {
                options: [
                    tr("AppRulesModeNoneText"),
                    tr("AppRulesModeLaunchText"),
                    tr("AppRulesModeLockText"),
                ]
                .map(String::from)
                .to_vec(),
                selected: rule.volume_mode.0.clamp(0, 2) as usize,
                set: Set::RuleMode(exe.into()),
            },
        )),
    ];
    if rule.volume_mode.0 > 0 {
        rows.push(indented(row(
            tr("AppRulesTargetVolumeText"),
            None,
            volume(rule.volume_percent, Set::RuleVolume(exe.into())),
        )));
    }
    rows
}

// ---------- Device shortcuts ----------

/// One shortcut per active output (deviceHotkeys), the default marked.
pub fn device_shortcuts(core: &Core) -> Vec<Row> {
    let devices = core.audio.devices().unwrap_or_default().devices;
    devices
        .iter()
        .map(|d| Row {
            badge: d
                .is_default
                .then(|| tr("WebSettingsDefaultDeviceBadge").into()),
            ..row(
                &d.name,
                None,
                Control::Hotkey {
                    target: HotkeyTarget::Device(d.id.clone()),
                    text: core
                        .store
                        .hotkey(&format!("DeviceHotkey:{}", d.id))
                        .as_ref()
                        .map(hotkey_text)
                        .unwrap_or_default(),
                    fallback: None,
                },
            )
        })
        .collect()
}

/// SetHotkey for every target.
pub fn set_hotkey(core: &mut Core, target: &HotkeyTarget, hotkey: HotkeyData) {
    match target {
        HotkeyTarget::Setting(key) => core.store.set(*key, Value::Hotkey(Some(hotkey))),
        // SetProfileHotkey: the preset is saved in place.
        HotkeyTarget::Preset(index) => {
            profiles::Service::open(core).edit(*index, |p| p.hotkey = Some(hotkey));
        }
        HotkeyTarget::Device(id) if !id.is_empty() => {
            core.store.set_hotkey(&format!("DeviceHotkey:{id}"), hotkey)
        }
        HotkeyTarget::Device(_) => {}
    }
}

// ---------- values ----------

/// UpdateAppRule: one behaviour changes, the others keep their values.
fn update_rule(core: &mut Core, exe: &str, change: impl FnOnce(&AppRuleEntry) -> Mutation) {
    let store = &mut core.store;
    let Some(rule) = with_env(|env| store.rules.get(Some(exe), env)) else {
        return;
    };
    let mutation = change(&rule);
    with_env(|env| {
        store.rules.change(
            Metadata {
                exe_name: Some(exe),
                display_name: rule.display_name.as_deref(),
                ..Metadata::default()
            },
            mutation,
            env,
        )
    });
    store.flush();
    core.enforce_rules();
}

/// The list pages' part of `pages::apply`.
pub fn apply(core: &mut Core, state: &mut PageState, set: Set, value: f64) {
    let on = value != 0.0;
    let percent = value.round().clamp(0.0, 100.0) as i32;
    let form = &mut state.capture;
    match set {
        Set::PresetInclude(index, part) => {
            profiles::Service::open(core).edit(index, |p| match part {
                Include::Devices => p.include_device_volumes = on,
                Include::Apps => p.include_app_volumes = on,
                Include::Route => p.route_apps = on,
            });
        }
        Set::PresetDeviceVolume(index, key) => {
            profiles::Service::open(core).set_device_entry(index, &key, Some(percent), None);
        }
        Set::PresetAppVolume(index, key) => {
            profiles::Service::open(core).set_app_entry(index, &key, Some(percent), None);
        }
        Set::PresetDefault(index, role, ids) => {
            if let Some(id) = ids.get(value as usize) {
                profiles::Service::open(core).set_default_device(index, role, id);
            }
        }
        Set::PresetAddDevice(index, ids) => {
            if let Some(id) = ids.get(value as usize).filter(|id| !id.is_empty()) {
                profiles::Service::open(core).add_device_entry(index, id);
            }
        }
        Set::CaptureDefaults => form.defaults = on,
        Set::CaptureRole(role) => {
            form.roles.retain(|r| *r != role);
            if on {
                form.roles.push(role);
            }
        }
        Set::CaptureDevices => form.device_volumes = on,
        Set::CaptureDevice(id) => {
            let mut ids = selected_ids(form, &audio(core));
            ids.retain(|i| *i != id);
            if on {
                ids.push(id);
            }
            form.device_ids = Some(ids);
        }
        Set::CaptureApps => form.app_volumes = on,
        Set::CaptureRoute => form.route_apps = on,
        Set::RuleHardMute(exe) => update_rule(core, &exe, |_| Mutation::HardMute(on)),
        Set::RuleFocusLost(exe) => update_rule(core, &exe, |_| Mutation::FocusLost(on)),
        Set::RuleMode(exe) => update_rule(core, &exe, |r| {
            let mode = VolumeRuleMode((value as i32).clamp(0, 2));
            Mutation::Volume(mode, r.volume_percent)
        }),
        Set::RuleVolume(exe) => {
            update_rule(core, &exe, |r| Mutation::Volume(r.volume_mode, percent))
        }
        Set::FolderVolume(id) => {
            let rule = core
                .folder_rules()
                .into_iter()
                .find(|r| r.id.as_deref() == Some(&id));
            if let Some(path) = rule.and_then(|r| r.folder_path) {
                let _ = core.update_folder_rule(&id, &path, percent);
            }
        }
        _ => {}
    }
}

// ---------- actions ----------

fn toggle(state: &mut PageState, item: Expand) -> bool {
    if state.expanded.as_ref() == Some(&item) {
        state.expanded = None;
        false
    } else {
        state.expanded = Some(item);
        true
    }
}

/// `Path.GetInvalidFileNameChars` become underscores; blank is "profile".
pub fn sanitize_file_name(name: &str) -> String {
    if is_blank(Some(name)) {
        return "profile".into();
    }
    name.chars()
        .map(|c| {
            if (c as u32) < 32 || "\"<>|:*?\\/".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// CaptureProfile: the form's choices as CaptureOptions (the modern
/// branch of ReadCaptureOptions); `None` when nothing is checked.
fn capture_options(form: &CaptureForm, audio: &Audio) -> Option<CaptureOptions> {
    let ids = selected_ids(form, audio);
    let roles = selected_roles(form, audio);
    let has_devices = form.device_volumes && !ids.is_empty();
    let has_defaults = form.defaults && !roles.is_empty();
    if !has_devices && !form.app_volumes && !has_defaults {
        return None;
    }
    Some(CaptureOptions {
        include_device_volumes: has_devices,
        device_ids: Some(if has_devices { ids } else { vec![] }),
        all_devices: false,
        include_app_volumes: form.app_volumes,
        apps_from_all_devices: true,
        route_apps: form.app_volumes && form.route_apps,
        default_roles: Some(if has_defaults {
            roles.into_iter().map(|r| Role::ALL[r]).collect()
        } else {
            vec![]
        }),
    })
}

fn read_text(path: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(text.trim_start_matches('\u{FEFF}').to_owned())
}

/// Runs a list action; `owner` parents the file dialogs.
pub fn run(
    core: &mut Core,
    state: &mut PageState,
    texts: &mut Texts,
    owner: isize,
    action: &Action,
) -> Effect {
    let mut effect = Effect::default();
    match action {
        Action::PresetToggle(item) => {
            if toggle(state, item.clone()) {
                let Expand::Preset(key) = item else {
                    return effect;
                };
                texts.rename = profiles::read(&core.store)
                    .iter()
                    .find(|p| preset_key(p) == *key)
                    .and_then(|p| p.name.clone())
                    .unwrap_or_default();
            }
        }
        Action::PresetApply(index) => effect.apply = Some(*index),
        Action::PresetCapture => {
            let Some(options) = capture_options(&state.capture, &audio(core)) else {
                return effect;
            };
            let name = texts.preset.trim();
            let name = if name.is_empty() {
                bt_core::cli_presets::default_name(&profiles::now_text())
            } else {
                name.to_owned()
            };
            let mut service = profiles::Service::open(core);
            let profile = service.capture(&name, &options);
            let index = service.save(profile);
            if let Some(saved) = service.profiles().get(index) {
                state.expanded = Some(Expand::Preset(preset_key(saved)));
                texts.rename = saved.name.clone().unwrap_or_default();
            }
            texts.preset.clear();
            effect.presets = true;
        }
        Action::PresetImport => {
            let filters = [
                profile_filters()[0],
                profile_filters()[1],
                (tr("FileFilterAllFiles"), "*.*"),
            ];
            let Some(path) = dialog::open_file(
                owner,
                tr("ImportVolumeProfileDialogTitle"),
                &filters,
                "btprofile",
            ) else {
                return effect;
            };
            let Some(json) = read_text(&path) else {
                return effect;
            };
            let mut service = profiles::Service::open(core);
            if let Some(index) = service.import(&json) {
                if let Some(p) = service.profiles().get(index) {
                    state.expanded = Some(Expand::Preset(preset_key(p)));
                    texts.rename = p.name.clone().unwrap_or_default();
                }
                effect.presets = true;
            }
        }
        Action::PresetExport(index) => {
            let service = profiles::Service::open(core);
            let Some(profile) = service.profiles().get(*index) else {
                return effect;
            };
            let file = sanitize_file_name(profile.name.as_deref().unwrap_or_default());
            let Some(json) = service.export(*index) else {
                return effect;
            };
            if let Some(path) = dialog::save_file(
                owner,
                tr("ExportQuickTrumpetPresetDialogTitle"),
                &profile_filters(),
                "btprofile",
                &file,
            ) {
                // File.WriteAllText with Encoding.UTF8 writes a BOM.
                if let Err(e) = std::fs::write(&path, format!("\u{FEFF}{json}")) {
                    eprintln!("BetterTrumpet: export failed: {e}");
                }
            }
        }
        Action::PresetUpdate(index) => {
            profiles::Service::open(core).update_from_current_state(*index);
            effect.presets = true;
        }
        Action::PresetDelete(index) => {
            profiles::Service::open(core).delete(*index);
            state.expanded = None;
            effect.presets = true;
        }
        Action::PresetRename(index) => {
            let name = texts.rename.trim().to_owned();
            let mut service = profiles::Service::open(core);
            let same = service
                .profiles()
                .get(*index)
                .is_some_and(|p| p.name.as_deref() == Some(name.as_str()));
            if !name.is_empty() && !same {
                service.rename(*index, &name);
                effect.presets = true;
            }
        }
        Action::PresetDeviceMute(index, key, muted) => {
            profiles::Service::open(core).set_device_entry(*index, key, None, Some(*muted));
        }
        Action::PresetDeviceRemove(index, key) => {
            profiles::Service::open(core).remove_device_entry(*index, key);
        }
        Action::PresetAppMute(index, key, muted) => {
            profiles::Service::open(core).set_app_entry(*index, key, None, Some(*muted));
        }
        Action::PresetAppRemove(index, key) => {
            profiles::Service::open(core).remove_app_entry(*index, key);
        }
        Action::CaptureChoose(which) => {
            let form = &mut state.capture;
            form.open = if form.open == Some(*which) {
                None
            } else {
                Some(*which)
            };
        }
        Action::RuleToggle(item) | Action::FolderToggle(item) => {
            toggle(state, item.clone());
        }
        Action::RuleAdd => {
            let exe = normalize_exe_name(Some(&texts.exe));
            if !is_blank(Some(&exe)) {
                add_rule(core, &exe, None);
                state.expanded = Some(Expand::Rule(bt_core::rules::normalize_key(Some(&exe))));
            }
            texts.exe.clear();
        }
        Action::RuleBrowse => {
            // AppRulesBrowseDialogFilter: "Applications (*.exe)|*.exe".
            let filter = tr("AppRulesBrowseDialogFilter");
            let filters = [(filter.split('|').next().unwrap_or(filter), "*.exe")];
            if let Some(path) =
                dialog::open_file(owner, tr("AppRulesBrowseDialogTitle"), &filters, "exe")
            {
                let full = path.to_string_lossy().into_owned();
                let exe = normalize_exe_name(Some(&full));
                add_rule(core, &exe, Some(&full));
                state.expanded = Some(Expand::Rule(bt_core::rules::normalize_key(Some(&exe))));
                texts.exe.clear();
            }
        }
        Action::RuleRemove(exe) => {
            let store = &mut core.store;
            with_env(|env| store.rules.remove(Some(exe), env));
            store.flush();
        }
        Action::RuleClear => {
            let store = &mut core.store;
            with_env(|env| store.rules.clear(env));
            store.flush();
        }
        Action::FolderAdd => {
            if let Some(path) = dialog::pick_folder(owner, tr("FolderVolumeRulesBrowseDialogTitle"))
            {
                let path = path.to_string_lossy().into_owned();
                if core.add_folder_rule(&path, folders::DEFAULT_VOLUME).is_ok() {
                    let added = core.folder_rules().into_iter().find(|r| {
                        r.folder_path
                            .as_deref()
                            .is_some_and(|p| ordinal_ignore_case(p, &path))
                    });
                    if let Some(id) = added.and_then(|r| r.id) {
                        state.expanded = Some(Expand::Folder(id));
                    }
                }
            }
        }
        Action::FolderBrowse(id) => {
            let Some(rule) = core
                .folder_rules()
                .into_iter()
                .find(|r| r.id.as_deref() == Some(id.as_str()))
            else {
                return effect;
            };
            if let Some(path) = dialog::pick_folder(owner, tr("FolderVolumeRulesBrowseDialogTitle"))
            {
                let _ = core.update_folder_rule(id, &path.to_string_lossy(), rule.volume_percent);
            }
        }
        Action::FolderRemove(id) => core.remove_folder_rule(id),
        _ => {}
    }
    effect
}

/// AddAppRule(exe, exe[, path, true]): a neutral rule.
fn add_rule(core: &mut Core, exe: &str, path: Option<&str>) {
    let store = &mut core.store;
    with_env(|env| {
        store.rules.change(
            Metadata {
                exe_name: Some(exe),
                display_name: Some(exe),
                icon_path: path,
                is_desktop_app: path.map(|_| true),
            },
            Mutation::Add,
            env,
        )
    });
    store.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_labels_are_resource_keys() {
        for key in ROLE_LABELS {
            assert!(bt_app::i18n::KEYS.contains(&key), "{key}");
        }
    }

    #[test]
    fn export_file_names_replace_invalid_characters() {
        assert_eq!(sanitize_file_name("Night: <Mode>?"), "Night_ _Mode__");
        assert_eq!(sanitize_file_name("  "), "profile");
        assert_eq!(sanitize_file_name("a/b\\c"), "a_b_c");
    }

    #[test]
    fn preset_keys_prefer_id_then_slug_then_name() {
        let mut p = VolumeProfile {
            name: Some("Night".into()),
            ..VolumeProfile::default()
        };
        assert_eq!(preset_key(&p), "Night");
        p.slug = Some("night".into());
        assert_eq!(preset_key(&p), "night");
        p.id = Some("abc".into());
        assert_eq!(preset_key(&p), "abc");
    }
}
