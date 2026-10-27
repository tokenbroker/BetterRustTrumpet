//! The flyout's view of the settings (hidden entries, app rules) and the
//! original's rule enforcement (DeviceViewModel.ApplyRuleToApp).
use crate::store::{Store, with_env};
use bt_audio::{AppSession, AudioEngine, Snapshot};
use bt_core::{
    hidden::AppKey,
    models::{AppRuleEntry, VolumeRuleMode},
};
use serde_json::{Value as Json, json};
use std::collections::HashSet;

/// The original's AppId: the process image path, or the system sounds id.
pub fn app_id(app: &AppSession) -> String {
    if app.is_system_sounds {
        "System.SystemSoundsSession".into()
    } else {
        app.executable.clone().unwrap_or_else(|| app.key.clone())
    }
}

/// The original's ExeName: the image file name without its extension.
pub fn exe_name(app: &AppSession) -> String {
    if app.is_system_sounds {
        return "*SystemSounds".into();
    }
    let path = app.executable.as_deref().unwrap_or(&app.key);
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match file.rfind('.') {
        Some(dot) if dot > 0 => file[..dot].to_owned(),
        _ => file.to_owned(),
    }
}

fn mode_name(mode: VolumeRuleMode) -> &'static str {
    match mode {
        VolumeRuleMode::LAUNCH => "Launch",
        VolumeRuleMode::LOCK => "Lock",
        _ => "None",
    }
}

fn rule_json(rule: &AppRuleEntry) -> Json {
    json!({
        "volumeMode": mode_name(rule.volume_mode),
        "volumePercent": rule.volume_percent,
        "hardMuted": rule.hard_muted,
        "focusLostEnabled": rule.focus_lost_enabled,
    })
}

fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// The snapshot with what the flyout needs from the settings: per device
/// `hidden` and its `hiddenApps`; per app `appId`, `exeName`, `hidden`
/// and its `rule`.
pub fn annotate(snapshot: &Snapshot, store: &mut Store) -> Json {
    let mut out = serde_json::to_value(snapshot).unwrap_or_else(|_| json!({ "devices": [] }));
    with_env(|env| {
        let Some(devices) = out.get_mut("devices").and_then(Json::as_array_mut) else {
            return;
        };
        for (device, source) in devices.iter_mut().zip(&snapshot.devices) {
            let id = source.id.as_str();
            let hidden_apps: Vec<Json> = store
                .hidden_apps
                .for_device(Some(id), env, compare)
                .into_iter()
                .map(|e| {
                    json!({
                        "appId": e.app_id.unwrap_or_default(),
                        "exeName": e.exe_name.unwrap_or_default(),
                        "displayName": e.display_name.unwrap_or_default(),
                    })
                })
                .collect();
            device["hidden"] = json!(store.hidden_devices.is_hidden(Some(id), env));
            device["hiddenApps"] = json!(hidden_apps);
            let Some(apps) = device.get_mut("apps").and_then(Json::as_array_mut) else {
                continue;
            };
            for (app, session) in apps.iter_mut().zip(&source.apps) {
                let (app_id, exe) = (app_id(session), exe_name(session));
                let hidden = store.hidden_apps.is_hidden(
                    AppKey {
                        device_id: Some(id),
                        app_id: Some(&app_id),
                        exe_name: Some(&exe),
                    },
                    env,
                );
                app["rule"] = store
                    .rules
                    .get(Some(&exe), env)
                    .map_or(Json::Null, |r| rule_json(&r));
                app["appId"] = json!(app_id);
                app["exeName"] = json!(exe);
                app["hidden"] = json!(hidden);
            }
        }
    });
    // Reading may migrate the legacy hard-mute list; keep what it wrote.
    store.flush();
    out
}

/// Process ids whose Launch rule has been applied (LaunchVolumeTracker).
#[derive(Default)]
pub struct LaunchTracker {
    claimed: HashSet<u32>,
}

impl LaunchTracker {
    /// Forgets an app's processes so its rule applies again right away.
    pub fn release(&mut self, pids: &[u32]) {
        for pid in pids {
            self.claimed.remove(pid);
        }
    }
}

fn percent(volume: f32) -> i32 {
    (volume * 100.0).round() as i32
}

/// Re-asserts Lock and hard mute on every app, and applies Launch to
/// processes seen for the first time. Rule writes record no undo step.
/// Returns whether anything was changed.
pub fn enforce(
    audio: &AudioEngine,
    store: &mut Store,
    tracker: &mut LaunchTracker,
    snapshot: &Snapshot,
) -> bool {
    let live: HashSet<u32> = snapshot
        .devices
        .iter()
        .flat_map(|d| d.apps.iter().flat_map(|a| a.process_ids.iter().copied()))
        .collect();
    // Exited processes are forgotten, so a relaunch counts as a launch.
    tracker.claimed.retain(|pid| live.contains(pid));
    let mut changed = false;
    with_env(|env| {
        for device in &snapshot.devices {
            for app in &device.apps {
                let fresh: Vec<u32> = app
                    .process_ids
                    .iter()
                    .copied()
                    .filter(|pid| *pid > 0 && !tracker.claimed.contains(pid))
                    .collect();
                tracker.claimed.extend(&fresh);
                let Some(rule) = store.rules.get(Some(&exe_name(app)), env) else {
                    continue;
                };
                let target = rule.volume_percent.clamp(0, 100);
                let set_volume = (rule.volume_mode == VolumeRuleMode::LOCK
                    && percent(app.volume) != target)
                    || (rule.volume_mode == VolumeRuleMode::LAUNCH && !fresh.is_empty());
                if set_volume {
                    let _ = audio.set_app_volume(&device.id, &app.key, target as f32 / 100.0);
                    changed = true;
                }
                // After the volume: a non-zero volume can clear a mute.
                if rule.hard_muted && (!app.muted || set_volume) {
                    let _ = audio.set_app_mute(&device.id, &app.key, true);
                    changed = true;
                }
            }
        }
    });
    store.flush();
    changed
}

/// Whether any rule needs enforcing at all (keeps idle work at zero).
pub fn any_active(store: &mut Store) -> bool {
    with_env(|env| {
        store
            .rules
            .sorted(env, compare)
            .iter()
            .any(|r| r.hard_muted || r.has_volume_rule())
    })
}
