//! Folder volume rules (FolderVolumeRuleEntriesJson): a new session whose
//! executable is under a rule's folder starts at that rule's volume, unless
//! its app has a volume rule of its own (DeviceViewModel.ApplyRuleToApp).
//! The deepest matching folder wins, then the earliest CreatedAtUtc (Q22,
//! Q33); matching, persistence and the add/update/remove lifecycle are
//! bt-core's `folder_rules`.
use crate::{
    core::Core,
    rules,
    store::{Store, with_env},
};
use bt_app::win32::{
    clock::{self, WindowsZone},
    paths::{self, FullPath},
};
use bt_core::{
    folder_rules::{Environment, FolderRule, FolderRuleStore, Outcome},
    folders::PathError,
    models::{AppRuleEntry, VolumeRuleMode},
};

/// AddFolderVolumeRule's default volume.
#[allow(dead_code)] // For the settings window's Rules page (not ported yet).
pub const DEFAULT_VOLUME: i32 = 5;

/// Runs `work` with the native path resolver, zone, clock and GUIDs.
pub fn with_folder_env<T>(
    work: impl FnOnce(&mut Environment<'_, FullPath, WindowsZone>) -> T,
) -> T {
    let mut new_id = paths::new_id;
    let mut env = Environment {
        resolver: &FullPath,
        zone: &WindowsZone,
        today_ticks: clock::today_ticks(),
        now_utc_ticks: clock::utc_now_ticks(),
        new_id: &mut new_id,
    };
    work(&mut env)
}

/// Whether any folder rule exists (keeps idle work at zero without them).
pub fn any(rules: &mut FolderRuleStore) -> bool {
    with_folder_env(|env| !rules.sorted(env).is_empty())
}

/// `TryGetFolderVolume` for an app id (the executable path). A resolver
/// failure counts as no match.
pub fn matched_volume(rules: &mut FolderRuleStore, app_id: &str) -> Option<i32> {
    with_folder_env(|env| rules.try_match(Some(app_id), env).ok().flatten())
}

/// The volume ApplyRuleToApp writes, in its order: Lock always (when the
/// volume differs), Launch on a new process, otherwise a folder default on
/// a new process for apps without a volume rule. `folder` is only asked
/// when needed.
pub fn volume_target(
    rule: Option<&AppRuleEntry>,
    current: i32,
    fresh: bool,
    folder: impl FnOnce() -> Option<i32>,
) -> Option<i32> {
    match rule {
        Some(r) if r.volume_mode == VolumeRuleMode::LOCK => {
            let target = r.volume_percent.clamp(0, 100);
            (current != target).then_some(target)
        }
        Some(r) if r.volume_mode == VolumeRuleMode::LAUNCH => {
            fresh.then(|| r.volume_percent.clamp(0, 100))
        }
        Some(r) if r.has_volume_rule() => None,
        _ if fresh => folder(),
        _ => None,
    }
}

/// Persists the rules after a change that saved them.
fn persist(store: &mut Store, outcome: Outcome) {
    if outcome.saved {
        if let Some(json) = store.folder_rules.stored().map(str::to_owned) {
            store.write_text(crate::store::FOLDER_RULES, &json);
        }
    }
}

// Entry points for the settings window's Rules page (not ported yet).
#[allow(dead_code)]
impl Core {
    /// GetFolderVolumeRules: the rules ordered by creation time.
    pub fn folder_rules(&mut self) -> Vec<FolderRule> {
        let rules = &mut self.store.folder_rules;
        with_folder_env(|env| rules.sorted(env))
    }

    /// FolderVolumeRulesJson (as exported).
    pub fn folder_rules_json(&mut self) -> String {
        let rules = &mut self.store.folder_rules;
        with_folder_env(|env| rules.rules_json(env))
    }

    /// AddFolderVolumeRule; an existing folder only has its volume updated.
    pub fn add_folder_rule(&mut self, folder: &str, volume: i32) -> Result<(), PathError> {
        let rules = &mut self.store.folder_rules;
        let outcome = with_folder_env(|env| rules.add(Some(folder), volume, env))?;
        self.folder_rules_changed(outcome);
        Ok(())
    }

    /// UpdateFolderVolumeRule by id.
    pub fn update_folder_rule(
        &mut self,
        id: &str,
        folder: &str,
        volume: i32,
    ) -> Result<(), PathError> {
        let rules = &mut self.store.folder_rules;
        let outcome = with_folder_env(|env| rules.update(Some(id), Some(folder), volume, env))?;
        self.folder_rules_changed(outcome);
        Ok(())
    }

    /// RemoveFolderVolumeRule by id.
    pub fn remove_folder_rule(&mut self, id: &str) {
        let rules = &mut self.store.folder_rules;
        let outcome = with_folder_env(|env| rules.remove(Some(id), env));
        self.folder_rules_changed(outcome);
    }

    /// The FolderVolumeRulesJson setter (settings import).
    pub fn set_folder_rules_json(&mut self, json: Option<&str>) {
        let outcome = self.store.folder_rules.set_json(json);
        self.folder_rules_changed(outcome);
    }

    fn folder_rules_changed(&mut self, outcome: Outcome) {
        persist(&mut self.store, outcome);
        if outcome.changed {
            self.apply_folder_defaults();
        }
    }

    /// ApplyFolderVolumeDefaults: after a folder rule change, live matching
    /// apps without a volume rule of their own move to the folder volume
    /// right away; Launch rules of other apps are left alone.
    pub fn apply_folder_defaults(&mut self) {
        let Ok(snapshot) = self.audio.snapshot() else {
            return;
        };
        for device in &snapshot.devices {
            for app in &device.apps {
                let exe = rules::exe_name(app);
                let store = &mut self.store;
                let rule = with_env(|env| store.rules.get(Some(&exe), env));
                if rule.is_some_and(|r| r.has_volume_rule()) {
                    continue;
                }
                let Some(volume) =
                    matched_volume(&mut self.store.folder_rules, &rules::app_id(app))
                else {
                    continue;
                };
                self.launch.release(&app.process_ids);
                self.launch.claim(&app.process_ids);
                if (app.volume * 100.0).round() as i32 != volume {
                    let _ = self
                        .audio
                        .set_app_volume(&device.id, &app.key, volume as f32 / 100.0);
                }
            }
        }
        self.store.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(mode: VolumeRuleMode, percent: i32) -> AppRuleEntry {
        AppRuleEntry {
            volume_mode: mode,
            volume_percent: percent,
            ..AppRuleEntry::default()
        }
    }

    #[test]
    fn app_volume_rules_take_precedence_over_folders() {
        let folder = || Some(5);
        let lock = rule(VolumeRuleMode::LOCK, 40);
        assert_eq!(volume_target(Some(&lock), 70, false, folder), Some(40));
        assert_eq!(volume_target(Some(&lock), 40, true, folder), None);
        let launch = rule(VolumeRuleMode::LAUNCH, 30);
        assert_eq!(volume_target(Some(&launch), 70, true, folder), Some(30));
        assert_eq!(volume_target(Some(&launch), 70, false, folder), None);
        // Neutral rules (hard mute, focus lost, pinned) leave folders active.
        let neutral = rule(VolumeRuleMode::NONE, 0);
        assert_eq!(volume_target(Some(&neutral), 70, true, folder), Some(5));
        assert_eq!(volume_target(None, 70, true, folder), Some(5));
        // Folder defaults only apply to new processes.
        assert_eq!(volume_target(None, 70, false, folder), None);
        assert_eq!(volume_target(None, 70, true, || None), None);
    }

    #[test]
    fn folder_is_not_resolved_when_an_app_rule_decides() {
        let lock = rule(VolumeRuleMode::LOCK, 40);
        let target = volume_target(Some(&lock), 10, true, || panic!("not needed"));
        assert_eq!(target, Some(40));
    }

    #[test]
    fn deepest_folder_wins_and_duplicate_folders_keep_the_first() {
        let mut store = FolderRuleStore::new(Some(
            r#"[{"Id":"a","FolderPath":"C:\\Games","VolumePercent":10,"CreatedAtUtc":"2024-01-02T00:00:00Z"},
                {"Id":"b","FolderPath":"C:\\Games\\Steam","VolumePercent":20,"CreatedAtUtc":"2024-01-03T00:00:00Z"},
                {"Id":"c","FolderPath":"C:/Games/Steam/","VolumePercent":30,"CreatedAtUtc":"2024-01-01T00:00:00Z"}]"#
                .into(),
        ));
        assert_eq!(
            matched_volume(&mut store, r"C:\Games\Steam\steamapps\x.exe"),
            Some(20)
        );
        assert_eq!(matched_volume(&mut store, r"c:\games\other.exe"), Some(10));
        assert_eq!(matched_volume(&mut store, r"D:\Games\x.exe"), None);
        assert_eq!(
            matched_volume(&mut store, "System.SystemSoundsSession"),
            None
        );
    }
}
