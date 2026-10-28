//! `.btsettings` export/import against the original SettingsExportService,
//! captured on .NET 8 (settings_export_reference.json).
mod support;

use bt_core::{
    colors::{self, NumberCulture},
    models::HotkeyData,
    newtonsoft,
    settings::{Key, Value},
    settings_export::{self, Imported, Metadata, Source, Target},
};
use serde_json::{Value as Json, json};
use support::Central;

fn reference() -> Json {
    serde_json::from_str(include_str!("fixtures/settings_export_reference.json")).unwrap()
}

fn base64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = vec![];
    let (mut bits, mut count) = (0u32, 0);
    for c in text.bytes().filter(|&c| c != b'=') {
        let value = ALPHABET.iter().position(|&a| a == c).unwrap() as u32;
        bits = bits << 6 | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out
}

/// `double.ToString("R")` under the invariant culture.
fn r_text(value: f64) -> String {
    let text = newtonsoft::format_double(value);
    match text.strip_suffix(".0") {
        Some(whole) if !text.contains('E') => whole.into(),
        _ => text,
    }
}

struct Values<'a>(&'a serde_json::Map<String, Json>);

impl Source for Values<'_> {
    fn setting(&mut self, key: Key) -> Value {
        let v = &self.0[key.name()];
        match key.default_value() {
            Value::Bool(_) => Value::Bool(v.as_bool().unwrap()),
            Value::Int(_) => Value::Int(v.as_i64().unwrap() as i32),
            Value::Float(_) => Value::Float(v.as_str().unwrap().parse().unwrap()),
            Value::Text(_) => Value::Text(v.as_str().map(str::to_owned)),
            Value::Hotkey(_) => Value::Hotkey(v.as_array().map(|h| HotkeyData {
                modifiers: h[0].as_i64().unwrap() as i32,
                key: h[1].as_i64().unwrap() as i32,
            })),
        }
    }
    fn color(&mut self, name: &str) -> String {
        let culture = NumberCulture::INVARIANT;
        colors::parse(self.0[name].as_str().unwrap(), &culture)
            .unwrap()
            .to_text(&culture)
    }
    fn run_at_startup(&mut self) -> bool {
        self.0["RunAtStartup"].as_bool().unwrap()
    }
    fn app_rules_json(&mut self) -> Option<String> {
        self.0["AppRulesJson"].as_str().map(str::to_owned)
    }
    fn folder_rules_json(&mut self) -> Option<String> {
        self.0["FolderVolumeRulesJson"].as_str().map(str::to_owned)
    }
}

#[test]
fn export_matches_capture_settings_and_indented_serialization() {
    let reference = reference();
    let cases = reference["exports"].as_array().unwrap();
    assert_eq!(cases.len(), 4);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut source = Values(case["values"].as_object().unwrap());
        let metadata = Metadata {
            utc_ticks: case["utcTicks"].as_i64().unwrap(),
            app_version: case["version"].as_str(),
        };
        let actual = settings_export::export(&mut source, metadata);
        if case["ok"].as_bool().unwrap() {
            assert_eq!(
                actual.as_deref(),
                Ok(case["text"].as_str().unwrap()),
                "{name}"
            );
        } else {
            assert!(actual.is_err(), "{name}");
        }
    }
}

fn project(assignment: &settings_export::Assignment) -> Json {
    let value = match &assignment.value {
        Imported::Bool(v) => json!(v),
        Imported::Int(v) => json!(v),
        Imported::Float(v) => json!(r_text(*v)),
        Imported::Text(v) => json!(v),
        Imported::Color(v) => json!(v),
        Imported::Hotkey(h) => json!([h.modifiers, h.key]),
    };
    json!([assignment.property(), value])
}

#[test]
fn import_matches_apply_settings_conversions_and_order() {
    let reference = reference();
    let cases = reference["imports"].as_array().unwrap();
    assert!(cases.len() > 150);
    let mut failures = vec![];
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let bytes = if let Some(b) = case["base64"].as_str() {
            base64(b)
        } else if let Some(text) = case["text"].as_str() {
            text.as_bytes().to_vec()
        } else {
            // A missing file is the app's read error: false, nothing set.
            assert_eq!(case["ok"], json!(false), "{name}");
            continue;
        };
        let actual = match settings_export::import(&bytes, &Central) {
            Ok(assignments) => json!({
                "ok": true,
                "sets": assignments.iter().map(project).collect::<Vec<_>>(),
            }),
            Err(_) => json!({"ok": false, "sets": []}),
        };
        let expected = json!({"ok": case["ok"], "sets": case["sets"]});
        if actual != expected {
            failures.push(format!("{name}: expected {expected}, got {actual}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn import_targets_rules_and_skips_run_at_startup() {
    let text = br#"{"RunAtStartup": true, "HardMutedAppsJson": "[]", "AppRulesJson": "[]",
        "FlyoutHotkey": {"Modifiers": 1, "Key": 2}, "TextColor": "Red"}"#;
    let assignments = settings_export::import(text, &Central).unwrap();
    let targets: Vec<Target> = assignments.iter().map(|a| a.target).collect();
    assert_eq!(
        targets,
        [
            Target::Color("TextColor"),
            Target::Setting(Key::FlyoutHotkey),
            Target::AppRules,
            Target::LegacyHardMutedApps,
        ]
    );
    assert_eq!(
        assignments[1].setting_value(),
        Value::Hotkey(Some(HotkeyData {
            modifiers: 1,
            key: 2
        }))
    );
    assert_eq!(assignments[0].text(), Some("#FFFF0000"));
}

#[test]
fn null_string_matches_the_registry_bag_serializer() {
    assert_eq!(
        bt_core::legacy_xml::encode_null_string(),
        reference()["nullStringXml"].as_str().unwrap()
    );
}

#[test]
fn export_round_trips_through_import() {
    let reference = reference();
    let case = &reference["exports"][1];
    let text = case["text"].as_str().unwrap();
    let assignments = settings_export::import(text.as_bytes(), &Central).unwrap();
    // Every exported field but RunAtStartup comes back.
    assert_eq!(assignments.len(), 56);
    assert!(assignments.iter().all(|a| a.key != "RunAtStartup"));
}
