//! App identity as the original's settings key it, and its rule
//! enforcement (DeviceViewModel.ApplyRuleToApp).
use crate::store::{Store, with_env};
use bt_audio::{AppSession, AudioEngine, Snapshot};
use bt_core::models::VolumeRuleMode;
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

fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
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
