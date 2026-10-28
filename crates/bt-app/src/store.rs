//! The user's BetterTrumpet settings: the original registry values, archived
//! once (read-only) to `%LOCALAPPDATA%\BetterTrumpet-Rust\migration.v1.json`,
//! with this app's own changes as overrides in `settings.v1.json` beside it
//! (D12, D40, D43). The original key is never written.
use bt_app::win32::{
    clock::{self, WindowsZone},
    registry::{self, RegistryEnvironment},
};
use bt_core::{
    app_rules::{AppRules, Effect, Environment},
    folder_rules::FolderRuleStore,
    hidden::{HiddenApps, HiddenDevices},
    legacy_xml::{self, Kind},
    models::HotkeyData,
    newtonsoft::{Date, DateKind},
    overrides::EncodedValue,
    registry_snapshot::RegistrySnapshot,
    registry_values::{Environment as _, Read, Request},
    settings::{Context, Key, Settings, Value},
};
use bt_storage::editable::EditableFile;
use std::path::PathBuf;

const HIDDEN_APPS: &str = "HiddenAppEntriesJson";
const HIDDEN_DEVICES: &str = "HiddenDeviceEntriesJson";
const APP_RULES: &str = "AppRuleEntriesJson";
const LEGACY_HARD_MUTED: &str = "HardMutedAppEntriesJson";
pub const FOLDER_RULES: &str = "FolderVolumeRuleEntriesJson";

pub struct Store {
    archive: RegistrySnapshot,
    /// `None` when the Rust settings directory is unusable; changes then
    /// last for this session only.
    file: Option<EditableFile>,
    pub settings: Settings,
    pub hidden_apps: HiddenApps,
    pub hidden_devices: HiddenDevices,
    pub rules: AppRules,
    pub folder_rules: FolderRuleStore,
}

fn directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("BetterTrumpet-Rust"))
}

/// The original's XmlSerializer text for a non-string value.
fn xml(value: &Value) -> Option<String> {
    const HEADER: &str = r#"<?xml version="1.0" encoding="utf-16"?>"#;
    Some(match value {
        Value::Bool(v) => format!("{HEADER}<boolean>{v}</boolean>"),
        Value::Int(v) => format!("{HEADER}<int>{v}</int>"),
        Value::Float(v) if v.is_nan() => format!("{HEADER}<double>NaN</double>"),
        Value::Float(v) if v.is_infinite() => {
            format!(
                "{HEADER}<double>{}INF</double>",
                if *v < 0.0 { "-" } else { "" }
            )
        }
        Value::Float(v) => format!("{HEADER}<double>{v}</double>"),
        Value::Text(text) => return text.clone(),
        // Hotkeys are edited in the settings window, which is not ported yet.
        Value::Hotkey(_) => return None,
    })
}

fn kind(key: Key) -> Kind {
    match key.default_value() {
        Value::Bool(_) => Kind::Bool,
        Value::Int(_) => Kind::I32,
        Value::Float(_) => Kind::F64,
        Value::Text(_) => Kind::String,
        Value::Hotkey(_) => Kind::Hotkey,
    }
}

pub fn context() -> Context<'static> {
    Context {
        region_code: "",
        on_battery: false,
    }
}

/// The clock and zone the original's lists read when loading and stamping.
pub fn with_env<T>(work: impl FnOnce(&Environment<'_>) -> T) -> T {
    let env = Environment {
        zone: &WindowsZone,
        today_ticks: clock::today_ticks(),
        now_utc: Date {
            ticks: clock::utc_now_ticks(),
            kind: DateKind::Utc,
        },
    };
    work(&env)
}

impl Store {
    pub fn open() -> Self {
        let dir = directory();
        let archive = dir
            .as_deref()
            .and_then(|d| registry::migrate_legacy_settings(d).ok())
            .map(|m| m.snapshot)
            .unwrap_or_default();
        let file = dir.as_deref().and_then(|d| EditableFile::open(d).ok());
        let mut store = Self {
            archive,
            file,
            settings: Settings::default(),
            hidden_apps: HiddenApps::default(),
            hidden_devices: HiddenDevices::default(),
            rules: AppRules::default(),
            folder_rules: FolderRuleStore::default(),
        };
        for &key in Key::ALL {
            if let Some(value) = store.load(key) {
                store
                    .settings
                    .values
                    .insert(key.storage_key().into(), value);
            }
        }
        store.hidden_apps = HiddenApps::new(store.text(HIDDEN_APPS));
        store.hidden_devices = HiddenDevices::new(store.text(HIDDEN_DEVICES));
        store.rules = AppRules::new(store.text(APP_RULES), store.text(LEGACY_HARD_MUTED));
        store.folder_rules = FolderRuleStore::new(store.text(FOLDER_RULES));
        store
    }

    fn read(&self, key: &str, request: Request) -> Read {
        let env = RegistryEnvironment;
        match &self.file {
            Some(file) => file
                .document
                .registry_read(&self.archive, Some(key), request, &env)
                .unwrap_or(Read::Default),
            None => self.archive.read(Some(key), request, &env),
        }
    }

    /// A string setting stored as-is (lists, colours, names).
    pub fn text(&self, key: &str) -> Option<String> {
        match self.read(key, Request::Value(Kind::String)) {
            Read::StringUtf16(units) => Some(String::from_utf16_lossy(&units)),
            Read::Value(legacy_xml::Value::String(text)) => text,
            _ => None,
        }
    }

    /// A hotkey stored under a dynamic name ("DeviceHotkey:<id>").
    pub fn hotkey(&self, name: &str) -> Option<HotkeyData> {
        match self.read(name, Request::Value(Kind::Hotkey)) {
            Read::Value(legacy_xml::Value::Hotkey(v)) => v,
            _ => None,
        }
    }

    fn load(&self, key: Key) -> Option<Value> {
        let name = key.storage_key();
        if key == Key::HasShownFirstRun {
            let env = RegistryEnvironment;
            let present = match &self.file {
                Some(file) => file
                    .document
                    .registry_has_key(&self.archive, Some(name), &env)
                    .unwrap_or(false),
                None => self.archive.has_key(Some(name), &env).unwrap_or(false),
            };
            return present.then_some(Value::Bool(true));
        }
        // UseLegacyIcon is a "True"/"False" string in the original.
        let kind = if key == Key::UseLegacyIcon {
            Kind::String
        } else {
            kind(key)
        };
        Some(match self.read(name, Request::Value(kind)) {
            Read::StringUtf16(units) => Value::Text(Some(String::from_utf16_lossy(&units))),
            Read::Value(legacy_xml::Value::Bool(v)) => Value::Bool(v),
            Read::Value(legacy_xml::Value::I32(v)) => Value::Int(v),
            Read::Value(legacy_xml::Value::F64(v)) => Value::Float(v),
            Read::Value(legacy_xml::Value::String(v)) => Value::Text(v),
            Read::Value(legacy_xml::Value::Hotkey(v)) => Value::Hotkey(v),
            _ => return None,
        })
    }

    pub fn get(&self, key: Key) -> Value {
        self.settings.get(key, context())
    }
    pub fn bool(&self, key: Key) -> bool {
        matches!(self.get(key), Value::Bool(true))
    }
    pub fn int(&self, key: Key) -> i32 {
        match self.get(key) {
            Value::Int(v) => v,
            _ => 0,
        }
    }
    pub fn float(&self, key: Key) -> f64 {
        match self.get(key) {
            Value::Float(v) => v,
            _ => 0.0,
        }
    }

    /// Changes a setting and saves it as an override.
    pub fn set(&mut self, key: Key, value: Value) {
        if self.settings.set(key, value, context()).is_err() {
            return;
        }
        let stored = self.settings.values.get(key.storage_key()).cloned();
        if let Some(text) = stored.as_ref().and_then(xml) {
            self.write(key.storage_key(), &text);
        }
    }

    /// Saves a string setting as-is (lists the app keeps itself).
    pub fn write_text(&mut self, key: &str, text: &str) {
        self.write(key, text);
    }

    fn write(&mut self, key: &str, text: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let units: Vec<u16> = key.encode_utf16().collect();
        let env = RegistryEnvironment;
        let value = EncodedValue::Text(text.encode_utf16().collect());
        if file
            .document
            .set(&units, value, |a, b| env.names_equal(a, b))
            .is_ok()
        {
            if let Err(e) = file.save() {
                eprintln!("BetterTrumpet: could not save settings: {e}");
            }
        }
    }

    /// Writes the lists' pending changes to their keys.
    pub fn flush(&mut self) {
        for (key, effects) in [
            (HIDDEN_APPS, self.hidden_apps.take_effects()),
            (HIDDEN_DEVICES, self.hidden_devices.take_effects()),
            (APP_RULES, self.rules.take_effects()),
        ] {
            for effect in effects {
                if let Effect::Write(json) = effect {
                    self.write(key, &json);
                }
            }
        }
    }
}
