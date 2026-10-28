//! The `.btsettings` file (SettingsExportService) over the Rust store:
//! export reads the AppSettings getters, import runs each converted field
//! through the store's setters, in the original's order. The original
//! registry settings are never written.
use crate::{
    core::Core,
    folders::{self, with_folder_env},
    store::{Store, with_env},
};
use bt_app::win32::{
    clock::{self, WindowsZone},
    paths, startup,
};
use bt_core::{
    cli_dispatch::SettingsFiles,
    colors::{self, NumberCulture},
    settings::{Key, Value},
    settings_export::{self, Assignment, Metadata, Source, Target},
};
use std::path::Path;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The getters CaptureSettings reads.
struct Getters<'a> {
    store: &'a mut Store,
    /// This build's own "Start with Windows" entry (the original reads its
    /// `BetterTrumpet` Run value).
    run_at_startup: bool,
}

impl Source for Getters<'_> {
    fn setting(&mut self, key: Key) -> Value {
        self.store.get(key)
    }
    fn color(&mut self, name: &str) -> String {
        let culture = NumberCulture::INVARIANT;
        colors::parse_setting(self.store.text(name).as_deref(), &culture).to_text(&culture)
    }
    fn run_at_startup(&mut self) -> bool {
        self.run_at_startup
    }
    fn app_rules_json(&mut self) -> Option<String> {
        let json = with_env(|env| self.store.rules.json(env).to_owned());
        // Reading runs the pending legacy migration; save it like the getter.
        self.store.flush();
        Some(json)
    }
    fn folder_rules_json(&mut self) -> Option<String> {
        let rules = &mut self.store.folder_rules;
        Some(with_folder_env(|env| rules.rules_json(env)))
    }
}

/// ExportToFile's text, or why there is none.
fn export(store: &mut Store, run_at_startup: bool, utc_ticks: i64) -> Option<String> {
    let mut getters = Getters {
        store,
        run_at_startup,
    };
    let metadata = Metadata {
        utc_ticks,
        app_version: Some(VERSION),
    };
    settings_export::export(&mut getters, metadata)
        .map_err(|e| eprintln!("BetterTrumpet: settings export failed: {e}"))
        .ok()
}

/// ImportFromFile's parse; `None` answers false.
fn read(path: &Path) -> Option<Vec<Assignment>> {
    let bytes = std::fs::read(path)
        .map_err(|e| eprintln!("BetterTrumpet: settings import failed: {e}"))
        .ok()?;
    settings_export::import(&bytes, &WindowsZone)
        .map_err(|e| eprintln!("BetterTrumpet: settings import failed: {e}"))
        .ok()
}

/// ApplySettings' setter calls. Returns whether the folder rules changed.
fn apply(store: &mut Store, assignments: &[Assignment]) -> bool {
    let mut folders_changed = false;
    for assignment in assignments {
        match assignment.target {
            Target::Setting(key) => store.set(key, assignment.setting_value()),
            Target::Color(name) => store.set_text(name, assignment.text().unwrap_or_default()),
            Target::AppRules => store.rules.set_json(assignment.text()),
            Target::LegacyHardMutedApps => {
                with_env(|env| store.rules.import_legacy(assignment.text(), env));
            }
            Target::FolderRules => {
                let outcome = store.folder_rules.set_json(assignment.text());
                folders::persist(store, outcome);
                folders_changed |= outcome.changed;
            }
        }
        // Rule setters queue their writes; save them in order.
        store.flush();
    }
    folders_changed
}

impl Core {
    /// ExportToFile: writes UTF-8 without a BOM, replacing the file.
    pub fn export_settings(&mut self, path: &Path) -> bool {
        let Some(text) = export(
            &mut self.store,
            startup::is_enabled(),
            clock::utc_now_ticks(),
        ) else {
            return false;
        };
        std::fs::write(path, text)
            .map_err(|e| eprintln!("BetterTrumpet: settings export failed: {e}"))
            .is_ok()
    }

    /// ImportFromFile: false only when the file cannot be read or is not a
    /// JSON object; fields that do not convert are skipped (Q13). The
    /// caller reloads whatever caches settings (`Ui::import_settings`).
    pub fn import_settings(&mut self, path: &Path) -> bool {
        let Some(assignments) = read(path) else {
            return false;
        };
        // FolderVolumeRulesChanged and AppRulesChanged: live sessions follow.
        if apply(&mut self.store, &assignments) {
            self.apply_folder_defaults();
        }
        self.enforce_rules();
        true
    }
}

/// The CLI's `export-settings` / `import-settings` over the live app.
pub struct Files<'a> {
    pub core: &'a mut Core,
    /// Set after a successful import: the UI reloads its settings.
    pub imported: bool,
}

impl SettingsFiles for Files<'_> {
    fn settings_available(&self) -> bool {
        true
    }
    fn default_export_path(&self) -> String {
        settings_export::default_cli_path(&paths::desktop(), clock::local_now_ms() * 10_000)
    }
    fn file_exists(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }
    fn export_to(&mut self, path: &str) -> bool {
        self.core.export_settings(Path::new(path))
    }
    fn import_from(&mut self, path: &str) -> bool {
        let ok = self.core.import_settings(Path::new(path));
        self.imported |= ok;
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bt_core::models::HotkeyData;

    fn import_text(store: &mut Store, dir: &Path, text: &str) -> bool {
        let path = dir.join("in.btsettings");
        std::fs::write(&path, text).unwrap();
        match read(&path) {
            Some(assignments) => {
                apply(store, &assignments);
                true
            }
            None => false,
        }
    }

    #[test]
    fn import_goes_through_the_store_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::in_directory(dir.path());
        let text = r#"{
            "_formatVersion": 7, "RunAtStartup": true, "Unknown": 1,
            "UseLegacyIcon": true, "PeakMeterFps": "45", "EcoMode": "yes",
            "QuickTrumpetNotificationSeconds": 99, "WindowBackgroundOpacity": 0.5,
            "TextColor": "Red", "AccentGlowColor": "bogus", "ActiveThemeName": null,
            "UseMonkeyTickSound": true, "MonkeyTickSoundUnlocked": true,
            "FlyoutHotkey": {"Modifiers": 131072, "Key": 65},
            "MixerHotkey": {"Modifiers": 1},
            "AppRulesJson": "[{\"ExeName\":\"a.exe\",\"VolumeMode\":2,\"VolumePercent\":40}]",
            "HardMutedAppsJson": "[{\"ExeName\":\"b.exe\"}]",
            "FolderVolumeRulesJson": ""
        }"#;
        assert!(import_text(&mut store, dir.path(), text));
        let check = |store: &mut Store| {
            assert!(store.bool(Key::UseLegacyIcon));
            assert_eq!(store.int(Key::PeakMeterFps), 45);
            assert!(!store.bool(Key::EcoMode));
            assert_eq!(store.int(Key::QuickTrumpetNotificationSeconds), 10);
            assert_eq!(store.float(Key::WindowBackgroundOpacity), 0.5);
            assert!(store.bool(Key::UseMonkeyTickSound));
            assert_eq!(store.text("TextColor").as_deref(), Some("#FFFF0000"));
            assert_eq!(store.text("AccentGlowColor"), None);
            assert_eq!(
                store.get(Key::FlyoutHotkey),
                Value::Hotkey(Some(HotkeyData {
                    modifiers: 131072,
                    key: 65
                }))
            );
            assert_eq!(
                store.get(Key::MixerHotkey),
                Value::Hotkey(Some(HotkeyData::default()))
            );
            let rules = with_env(|env| {
                let a = store.rules.get(Some("a.exe"), env).unwrap();
                let b = store.rules.get(Some("b.exe"), env).unwrap();
                (a.volume_percent, b.hard_muted)
            });
            assert_eq!(rules, (40, true));
            assert_eq!(store.folder_rules.stored(), Some("[]"));
        };
        check(&mut store);
        assert_eq!(store.get(Key::ActiveThemeName), Value::Text(None));
        // A second store over the same directory reads the saved overrides.
        let mut reopened = Store::in_directory(dir.path());
        check(&mut reopened);
        // Like the registry bag: Set(null) stores the nil XML, which the
        // string getter later returns as text.
        assert_eq!(
            reopened.get(Key::ActiveThemeName),
            Value::Text(Some(bt_core::legacy_xml::encode_null_string().into()))
        );
    }

    #[test]
    fn malformed_files_import_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::in_directory(dir.path());
        for text in [
            "",
            "null",
            "[]",
            r#"{"EcoMode": true"#,
            r#"{"a": new Date(0)}"#,
        ] {
            assert!(!import_text(&mut store, dir.path(), text), "{text}");
        }
        assert!(read(&dir.path().join("missing.btsettings")).is_none());
        assert!(!store.bool(Key::EcoMode));
        assert!(!dir.path().join("settings.v1.json").exists());
    }

    #[test]
    fn export_reads_the_store_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::in_directory(dir.path());
        store.set(Key::EcoMode, Value::Bool(true));
        store.set(Key::VolumeAnimationSpeed, Value::Float(0.25));
        store.set_text("PeakMeterColor", "#80112233");
        store.set(
            Key::SettingsHotkey,
            Value::Hotkey(Some(HotkeyData {
                modifiers: 262144,
                key: 112,
            })),
        );
        let text = export(&mut store, true, 639_000_000_000_000_000).unwrap();
        assert!(text.starts_with(
            "{\r\n  \"_formatVersion\": 1,\r\n  \"_exportedAt\": \"2025-11-29T08:00:00.0000000Z\",\r\n"
        ));
        for line in [
            "\"EcoMode\": true,",
            "\"RunAtStartup\": true,",
            "\"VolumeAnimationSpeed\": 0.25,",
            "\"PeakMeterColor\": \"#80112233\",",
            "\"SliderThumbColor\": \"#00FFFFFF\",",
            "\"SettingsHotkey\": {\r\n    \"Modifiers\": 262144,\r\n    \"Key\": 112\r\n  },",
            "\"AppRulesJson\": \"[]\",",
            "\"FolderVolumeRulesJson\": \"[]\"\r\n}",
        ] {
            assert!(text.contains(line), "{line}\n{text}");
        }
        let other = tempfile::tempdir().unwrap();
        let mut target = Store::in_directory(other.path());
        assert!(import_text(&mut target, other.path(), &text));
        assert!(target.bool(Key::EcoMode));
        assert_eq!(target.float(Key::VolumeAnimationSpeed), 0.25);
        assert_eq!(target.text("PeakMeterColor").as_deref(), Some("#80112233"));
        assert_eq!(
            target.get(Key::SettingsHotkey),
            store.get(Key::SettingsHotkey)
        );
    }
}
