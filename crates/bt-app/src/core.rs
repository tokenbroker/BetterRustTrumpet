//! The app's state outside the windows: audio, media, the settings store,
//! undo history and rule tracking, with the actions the flyout, the tray
//! and the hotkeys perform. Everything here runs on the UI thread.
use crate::{
    rules,
    store::{Store, with_env},
};
use bt_app::win32::clock;
use bt_audio::{AppSession, AudioEngine, Device};
use bt_core::{
    app_rules::{Metadata, Mutation},
    hidden::AppKey,
    models::{AppRuleEntry, VolumeRuleMode},
    undo::{Change, Target, UndoHistory},
};
use bt_media::MediaEngine;

pub struct Core {
    pub audio: AudioEngine,
    /// `None` when Windows media sessions are unavailable.
    pub media: Option<MediaEngine>,
    pub store: Store,
    pub undo: UndoHistory,
    pub launch: rules::LaunchTracker,
}

/// A hidden app entry saved for a device.
#[derive(Clone, Debug, PartialEq)]
pub struct HiddenEntry {
    pub app_id: String,
    pub exe_name: String,
    pub display_name: String,
}

impl HiddenEntry {
    pub fn label(&self) -> &str {
        [&self.display_name, &self.exe_name, &self.app_id]
            .into_iter()
            .find(|s| !s.is_empty())
            .map_or("", |s| s.as_str())
    }
}

/// An app as the flyout shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct AppView {
    pub app: AppSession,
    pub app_id: String,
    pub exe_name: String,
    pub hidden: bool,
    pub rule: Option<AppRuleEntry>,
}

impl AppView {
    pub fn locked(&self) -> bool {
        self.rule
            .as_ref()
            .is_some_and(|r| r.volume_mode == VolumeRuleMode::LOCK)
    }
}

/// A device with its apps and saved hidden state. `device.apps` is empty;
/// the apps are in `apps`.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceView {
    pub device: Device,
    pub hidden: bool,
    pub hidden_apps: Vec<HiddenEntry>,
    pub apps: Vec<AppView>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub devices: Vec<DeviceView>,
}

impl View {
    pub fn device(&self, id: &str) -> Option<&DeviceView> {
        self.devices.iter().find(|d| d.device.id == id)
    }
    pub fn device_mut(&mut self, id: &str) -> Option<&mut DeviceView> {
        self.devices.iter_mut().find(|d| d.device.id == id)
    }
    pub fn app(&self, device: &str, key: &str) -> Option<&AppView> {
        self.device(device)?.apps.iter().find(|a| a.app.key == key)
    }
    pub fn app_mut(&mut self, device: &str, key: &str) -> Option<&mut AppView> {
        self.device_mut(device)?
            .apps
            .iter_mut()
            .find(|a| a.app.key == key)
    }
}

fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn percent(volume: f32) -> i32 {
    (volume * 100.0).round() as i32
}

pub fn undo_target(id: &str, name: &str, is_device: bool) -> Target {
    Target {
        id: id.into(),
        display_name: name.into(),
        is_device,
    }
}

/// One of the app popup's rule buttons (FocusedAppItemViewModel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleKind {
    HardMute,
    FocusLost,
    Lock(i32),
    Launch(i32),
    None,
}

impl Core {
    /// Devices and apps with their hidden state and app rules.
    pub fn view(&mut self) -> View {
        let Ok(snapshot) = self.audio.snapshot() else {
            return View::default();
        };
        let store = &mut self.store;
        let view = with_env(|env| View {
            devices: snapshot
                .devices
                .into_iter()
                .map(|mut device| {
                    let id = device.id.clone();
                    let hidden_apps = store
                        .hidden_apps
                        .for_device(Some(&id), env, compare)
                        .into_iter()
                        .map(|e| HiddenEntry {
                            app_id: e.app_id.unwrap_or_default(),
                            exe_name: e.exe_name.unwrap_or_default(),
                            display_name: e.display_name.unwrap_or_default(),
                        })
                        .collect();
                    let apps = std::mem::take(&mut device.apps)
                        .into_iter()
                        .map(|app| {
                            let (app_id, exe_name) = (rules::app_id(&app), rules::exe_name(&app));
                            let hidden = store.hidden_apps.is_hidden(
                                AppKey {
                                    device_id: Some(&id),
                                    app_id: Some(&app_id),
                                    exe_name: Some(&exe_name),
                                },
                                env,
                            );
                            let rule = store.rules.get(Some(&exe_name), env);
                            AppView {
                                app,
                                app_id,
                                exe_name,
                                hidden,
                                rule,
                            }
                        })
                        .collect();
                    DeviceView {
                        hidden: store.hidden_devices.is_hidden(Some(&id), env),
                        device,
                        hidden_apps,
                        apps,
                    }
                })
                .collect(),
        });
        // Reading may migrate the legacy hard-mute list; keep what it wrote.
        self.store.flush();
        view
    }

    pub fn record_volume(&mut self, target: Target, old: i32, new: i32) {
        self.undo
            .record_volume_change(target, old, new, clock::local_now_ms());
    }

    fn record_mute(&mut self, target: Target, old: bool, new: bool) {
        self.undo
            .record_mute_change(target, old, new, clock::local_now_ms());
    }

    // Volume and mute changes from the user record an undo step, as the
    // original's view models do; `previous` is the volume before it.
    pub fn set_device_volume(&mut self, id: &str, name: &str, volume: f32, previous: i32) {
        self.record_volume(undo_target(id, name, true), previous, percent(volume));
        let _ = self.audio.set_device_volume(id, volume);
    }
    pub fn set_device_mute(&mut self, id: &str, name: &str, muted: bool) {
        self.record_mute(undo_target(id, name, true), !muted, muted);
        let _ = self.audio.set_device_mute(id, muted);
    }
    pub fn set_app_volume(
        &mut self,
        device: &str,
        key: &str,
        name: &str,
        volume: f32,
        previous: i32,
    ) {
        self.record_volume(undo_target(key, name, false), previous, percent(volume));
        let _ = self.audio.set_app_volume(device, key, volume);
    }
    pub fn set_app_mute(&mut self, device: &str, key: &str, name: &str, muted: bool) {
        self.record_mute(undo_target(key, name, false), !muted, muted);
        let _ = self.audio.set_app_mute(device, key, muted);
    }
    /// One process's session (the app popup's rows). The original cannot
    /// undo these: its undo targets whole apps.
    pub fn set_process_volume(&mut self, device: &str, key: &str, pid: u32, volume: f32) {
        let _ = self.audio.set_process_volume(device, key, pid, volume);
    }
    pub fn set_process_mute(&mut self, device: &str, key: &str, pid: u32, muted: bool) {
        let _ = self.audio.set_process_mute(device, key, pid, muted);
    }

    /// Hidden entries are saved (HiddenAppEntriesJson), so they stay hidden
    /// after a restart, as in the original.
    pub fn hide_app(&mut self, device: &str, app: &AppView) {
        let store = &mut self.store;
        with_env(|env| {
            store.hidden_apps.hide(
                AppKey {
                    device_id: Some(device),
                    app_id: Some(&app.app_id),
                    exe_name: Some(&app.exe_name),
                },
                Some(&app.app.display_name),
                env,
            )
        });
        self.store.flush();
    }
    pub fn unhide_app(&mut self, device: &str, entry: &HiddenEntry) {
        let store = &mut self.store;
        with_env(|env| {
            store.hidden_apps.unhide(
                AppKey {
                    device_id: Some(device),
                    app_id: Some(&entry.app_id),
                    exe_name: Some(&entry.exe_name),
                },
                env,
            )
        });
        self.store.flush();
    }
    /// "Restore all on this device".
    pub fn unhide_device_apps(&mut self, device: &str) {
        let store = &mut self.store;
        with_env(|env| store.hidden_apps.unhide_device(Some(device), env));
        self.store.flush();
    }
    pub fn hide_device(&mut self, device: &str, name: &str) {
        let store = &mut self.store;
        with_env(|env| store.hidden_devices.hide(Some(device), Some(name), env));
        self.store.flush();
    }

    /// Applies a rule button: hard mute and focus lost toggle; Lock and
    /// Launch set a volume rule at a percentage; None clears it.
    pub fn app_rule(&mut self, device: &str, app: &AppView, kind: RuleKind) {
        let Ok(snapshot) = self.audio.snapshot() else {
            return;
        };
        let session = snapshot
            .devices
            .iter()
            .find(|d| d.id == device)
            .and_then(|d| d.apps.iter().find(|a| a.key == app.app.key));
        let exe = Some(app.exe_name.as_str());
        let store = &mut self.store;
        let current = with_env(|env| store.rules.get(exe, env));
        let was_hard_muted = current.as_ref().is_some_and(|r| r.hard_muted);
        let mutation = match kind {
            RuleKind::HardMute => Mutation::HardMute(!was_hard_muted),
            RuleKind::FocusLost => {
                Mutation::FocusLost(!current.is_some_and(|r| r.focus_lost_enabled))
            }
            RuleKind::Lock(v) => Mutation::Volume(VolumeRuleMode::LOCK, v),
            RuleKind::Launch(v) => Mutation::Volume(VolumeRuleMode::LAUNCH, v),
            RuleKind::None => Mutation::Volume(VolumeRuleMode::NONE, 0),
        };
        with_env(|env| {
            store.rules.change(
                Metadata {
                    exe_name: exe,
                    display_name: Some(&app.app.display_name),
                    icon_path: app.app.executable.as_deref(),
                    is_desktop_app: Some(true),
                },
                mutation,
                env,
            )
        });
        self.store.flush();
        // Turning hard mute off unmutes the app the user is acting on.
        if was_hard_muted && kind == RuleKind::HardMute && session.is_some_and(|s| s.muted) {
            let _ = self.audio.set_app_mute(device, &app.app.key, false);
        }
        // A new volume rule applies right away, Launch included.
        if let Some(session) =
            session.filter(|_| matches!(kind, RuleKind::Lock(_) | RuleKind::Launch(_)))
        {
            self.launch.release(&session.process_ids);
        }
        rules::enforce(&self.audio, &mut self.store, &mut self.launch, &snapshot);
    }

    /// Ctrl+Z / Ctrl+Y: re-applies the old or new value to the device, or to
    /// the first device the app is on (UndoRedoHelper). Returns whether
    /// anything was undone or redone.
    pub fn replay(&mut self, is_undo: bool) -> bool {
        let action = if is_undo {
            self.undo.undo()
        } else {
            self.undo.redo()
        };
        let Some(action) = action else {
            return false;
        };
        let target = &action.target;
        let device_id = if target.is_device {
            Some(target.id.clone())
        } else {
            self.audio.snapshot().ok().and_then(|s| {
                s.devices
                    .iter()
                    .find(|d| d.apps.iter().any(|a| a.key == target.id))
                    .map(|d| d.id.clone())
            })
        };
        // The original skips targets that have gone away.
        let Some(device_id) = device_id else {
            return true;
        };
        let pick = |old, new| if is_undo { old } else { new };
        let audio = &self.audio;
        let _ = match (action.change, target.is_device) {
            (Change::Volume { old, new }, true) => {
                audio.set_device_volume(&device_id, pick(old, new) as f32 / 100.0)
            }
            (Change::Volume { old, new }, false) => {
                audio.set_app_volume(&device_id, &target.id, pick(old, new) as f32 / 100.0)
            }
            (Change::Mute { old, new }, true) => {
                audio.set_device_mute(&device_id, if is_undo { old } else { new })
            }
            (Change::Mute { old, new }, false) => {
                audio.set_app_mute(&device_id, &target.id, if is_undo { old } else { new })
            }
        };
        true
    }

    /// Applies the app rules (Lock, Launch, hard mute) when there are any;
    /// the session events that call this are what keep them enforced.
    pub fn enforce_rules(&mut self) {
        if !rules::any_active(&mut self.store) {
            return;
        }
        if let Ok(snapshot) = self.audio.snapshot() {
            rules::enforce(&self.audio, &mut self.store, &mut self.launch, &snapshot);
        }
    }

    /// Adjusts every unmuted output by `delta` percent, like the original's
    /// absolute volume hotkeys.
    pub fn step_all_devices(&mut self, delta: i32) {
        let Ok(devices) = self.audio.devices() else {
            return;
        };
        for device in devices.devices.iter().filter(|d| !d.muted) {
            let old = percent(device.volume);
            let new = (old + delta).clamp(0, 100);
            self.record_volume(undo_target(&device.id, &device.name, true), old, new);
            let _ = self
                .audio
                .set_device_volume(&device.id, device.volume + delta as f32 / 100.0);
        }
    }

    /// Mouse wheel over the tray icon: 2% per notch on the default output.
    pub fn step_default_device(&mut self, notches: i32) {
        let Ok(devices) = self.audio.devices() else {
            return;
        };
        if let Some(device) = devices.devices.iter().find(|d| d.is_default) {
            let old = percent(device.volume);
            let new = (old + notches * 2).clamp(0, 100);
            self.record_volume(undo_target(&device.id, &device.name, true), old, new);
            let _ = self
                .audio
                .set_device_volume(&device.id, device.volume + notches as f32 * 0.02);
        }
    }
}
