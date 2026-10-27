//! AppSettings app-rule cache and mutation lifecycle. Native code executes the
//! ordered effects against the Rust destination store, never the legacy source.
use crate::{
    models::{AppRuleEntry, VolumeRuleMode},
    newtonsoft::{self, Date, LocalZone},
    rules::normalize_key,
    text::is_blank,
    typed_lists,
};
use std::cmp::Ordering;

pub struct Environment<'a> {
    pub zone: &'a dyn LocalZone,
    pub today_ticks: i64,
    /// The native clock supplies DateTime.UtcNow, including its UTC kind.
    pub now_utc: Date,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Write(String),
    Changed,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Metadata<'a> {
    pub exe_name: Option<&'a str>,
    pub display_name: Option<&'a str>,
    pub icon_path: Option<&'a str>,
    pub is_desktop_app: Option<bool>,
}

#[derive(Clone, Copy, Debug)]
pub enum Mutation {
    Add,
    HardMute(bool),
    FocusLost(bool),
    Volume(VolumeRuleMode, i32),
}

#[derive(Clone, Debug, Default)]
pub struct AppRules {
    raw: Option<String>,
    legacy: Option<String>,
    entries: Option<Vec<AppRuleEntry>>,
    effects: Vec<Effect>,
}

impl AppRules {
    pub fn new(current: Option<String>, legacy: Option<String>) -> Self {
        Self {
            raw: current,
            legacy,
            ..Self::default()
        }
    }

    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Inspect the underlying bag without triggering the lazy cache/migration.
    pub fn stored_json(&self) -> &str {
        self.raw.as_deref().unwrap_or("[]")
    }

    pub fn legacy_json(&self) -> Option<&str> {
        self.legacy.as_deref()
    }

    fn ensure_loaded(&mut self, env: &Environment<'_>) {
        if self.entries.is_some() {
            return;
        }
        let loaded = typed_lists::load_app_rules(
            self.raw.as_deref(),
            self.legacy.as_deref(),
            env.zone,
            env.today_ticks,
        );
        self.entries = Some(loaded.entries);
        if loaded.save_migration {
            self.save();
        }
    }

    fn save(&mut self) {
        // This DTO contains only strings, integers, booleans and collections;
        // serde_json cannot fail for these values.
        let json = serde_json::to_string(self.entries.as_ref().expect("loaded rule cache"))
            .expect("serializable rule DTOs");
        self.raw = Some(json.clone());
        self.effects.push(Effect::Write(json));
    }

    pub fn json(&mut self, env: &Environment<'_>) -> &str {
        self.ensure_loaded(env);
        self.stored_json()
    }

    /// Passthrough import stores the input and invalidates the cache. It always
    /// notifies, even for identical JSON, and does not eagerly normalize it.
    pub fn set_json(&mut self, json: Option<&str>) {
        let json = if is_blank(json) { "[]" } else { json.unwrap() }.to_owned();
        self.raw = Some(json.clone());
        self.entries = None;
        self.effects.extend([Effect::Write(json), Effect::Changed]);
    }

    /// LegacyHardMutedAppsJson is import-only: merge through SetAppHardMuted,
    /// retaining existing rules and leaving the legacy migration source alone.
    pub fn import_legacy(&mut self, json: Option<&str>, env: &Environment<'_>) {
        if is_blank(json) {
            return;
        }
        let Ok(Some(entries)) =
            typed_lists::decode_legacy_hard_mutes(json.unwrap(), env.zone, env.today_ticks)
        else {
            return;
        };
        for entry in entries.into_iter().flatten() {
            self.change(
                Metadata {
                    exe_name: entry.exe_name.as_deref(),
                    display_name: entry.display_name.as_deref(),
                    ..Metadata::default()
                },
                Mutation::HardMute(true),
                env,
            );
        }
    }

    pub fn get(&mut self, exe: Option<&str>, env: &Environment<'_>) -> Option<AppRuleEntry> {
        let key = normalize_key(exe);
        if key.is_empty() {
            return None;
        }
        self.ensure_loaded(env);
        self.entries
            .as_ref()?
            .iter()
            .find(|r| r.exe_name.as_deref() == Some(&key))
            .cloned()
    }

    pub fn is_hard_muted(&mut self, exe: Option<&str>, env: &Environment<'_>) -> bool {
        self.get(exe, env).is_some_and(|rule| rule.hard_muted)
    }

    /// .NET's default string comparer is current-culture dependent. Native
    /// code supplies that comparison; no ordinal approximation is baked in.
    pub fn sorted(
        &mut self,
        env: &Environment<'_>,
        mut compare: impl FnMut(&str, &str) -> Ordering,
    ) -> Vec<AppRuleEntry> {
        self.ensure_loaded(env);
        let mut entries = self.entries.clone().unwrap_or_default();
        fn key(entry: &AppRuleEntry) -> &str {
            entry
                .display_name
                .as_deref()
                .filter(|s| !s.is_empty())
                .or(entry.exe_name.as_deref())
                .unwrap_or_default()
        }
        entries.sort_by(|a, b| {
            compare(key(a), key(b)).then_with(|| {
                compare(
                    a.exe_name.as_deref().unwrap_or_default(),
                    b.exe_name.as_deref().unwrap_or_default(),
                )
            })
        });
        entries
    }

    pub fn change(&mut self, metadata: Metadata<'_>, mutation: Mutation, env: &Environment<'_>) {
        let key = normalize_key(metadata.exe_name);
        if key.is_empty() {
            return;
        }
        let display = metadata.display_name.unwrap_or_default().trim();
        let icon = metadata.icon_path.unwrap_or_default().trim();
        self.ensure_loaded(env);
        let entries = self.entries.as_mut().unwrap();
        let existing = entries
            .iter()
            .position(|r| r.exe_name.as_deref() == Some(&key));
        let mut rule = existing
            .map(|i| entries[i].clone())
            .unwrap_or_else(|| AppRuleEntry {
                exe_name: Some(key),
                display_name: Some(display.into()),
                icon_path: Some(icon.into()),
                is_desktop_app: metadata.is_desktop_app.unwrap_or(false),
                is_pinned: true,
                created_at_utc: newtonsoft::format_date(&env.now_utc),
                ..AppRuleEntry::default()
            });
        let mut changed = match mutation {
            Mutation::Add => existing.is_none(),
            Mutation::HardMute(value) => {
                let changed = rule.hard_muted != value;
                rule.hard_muted = value;
                changed
            }
            Mutation::FocusLost(value) => {
                let changed = rule.focus_lost_enabled != value;
                rule.focus_lost_enabled = value;
                changed
            }
            Mutation::Volume(mode, percent) => {
                let bounded = percent.clamp(0, 100);
                if rule.volume_mode == mode
                    && (mode == VolumeRuleMode::NONE || rule.volume_percent == bounded)
                {
                    false
                } else {
                    rule.volume_mode = mode;
                    rule.volume_percent = if mode == VolumeRuleMode::NONE {
                        0
                    } else {
                        bounded
                    };
                    true
                }
            }
        };
        if existing.is_none() && !changed {
            return;
        }
        if existing.is_some() {
            if matches!(mutation, Mutation::Add) && !rule.is_pinned {
                rule.is_pinned = true;
                changed = true;
            }
            if !display.is_empty() && rule.display_name.as_deref() != Some(display) {
                rule.display_name = Some(display.into());
                changed = true;
            }
            let desktop_differs = !matches!(mutation, Mutation::Add)
                && metadata
                    .is_desktop_app
                    .is_some_and(|d| d != rule.is_desktop_app);
            if !icon.is_empty() && (rule.icon_path.as_deref() != Some(icon) || desktop_differs) {
                rule.icon_path = Some(icon.into());
                rule.is_desktop_app = metadata.is_desktop_app.unwrap_or(rule.is_desktop_app);
                changed = true;
            }
        }
        if !changed {
            return;
        }
        match existing {
            Some(i) => entries[i] = rule,
            None => entries.push(rule),
        }
        self.save();
        self.effects.push(Effect::Changed);
    }

    pub fn remove(&mut self, exe: Option<&str>, env: &Environment<'_>) {
        let key = normalize_key(exe);
        if key.is_empty() {
            return;
        }
        self.ensure_loaded(env);
        let entries = self.entries.as_mut().unwrap();
        let before = entries.len();
        entries.retain(|r| r.exe_name.as_deref() != Some(&key));
        if before != entries.len() {
            self.save();
            self.effects.push(Effect::Changed);
        }
    }

    pub fn clear(&mut self, env: &Environment<'_>) {
        self.ensure_loaded(env);
        let entries = self.entries.as_mut().unwrap();
        if !entries.is_empty() {
            entries.clear();
            self.save();
            self.effects.push(Effect::Changed);
        }
    }
}
