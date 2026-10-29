//! Expected values/events come from verbatim C# properties, not Rust defaults.
use bt_core::settings::{Context, Key, Settings, Value};
use serde_json::{Value as Json, json};

fn reference() -> Json {
    serde_json::from_str::<Json>(include_str!("fixtures/models_reference.json")).unwrap()["settings"].clone()
}
fn context(on_battery: bool) -> Context<'static> {
    Context {
        region_code: "US",
        on_battery,
    }
}
fn key(name: &str) -> Key {
    *Key::ALL
        .iter()
        .find(|k| k.name() == name)
        .unwrap_or_else(|| panic!("uncaptured key {name}"))
}
fn number(value: &Json) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| value.as_str().unwrap().parse().unwrap())
}
fn decoded(value: &Json, template: Value) -> Value {
    match template {
        Value::Bool(_) => Value::Bool(value.as_bool().unwrap()),
        Value::Int(_) => Value::Int(value.as_i64().unwrap() as i32),
        Value::Float(_) => Value::Float(number(value)),
        Value::Text(_) => Value::Text(value.as_str().map(str::to_owned)),
        Value::Hotkey(_) => Value::Hotkey(serde_json::from_value(value.clone()).unwrap()),
    }
}
fn encoded(value: &Value) -> Json {
    match value {
        Value::Bool(v) => json!(v),
        Value::Int(v) => json!(v),
        Value::Float(v) if v.is_nan() => json!("NaN"),
        Value::Float(v) if *v == f64::INFINITY => json!("Infinity"),
        Value::Float(v) if *v == f64::NEG_INFINITY => json!("-Infinity"),
        Value::Float(v) => json!(v),
        Value::Text(v) => json!(v),
        Value::Hotkey(v) => json!(v),
    }
}
fn snapshot(settings: &Settings, on_battery: bool) -> Json {
    let mut result = serde_json::Map::new();
    let context = context(on_battery);
    for &key in Key::ALL {
        // Reading this source property allocates an ID; test it explicitly.
        // AnimateTrayIcon, ShowProjectLinks and CheckGitHubForUpdates are
        // port-only (D51, D54, D55); the C# app has no such properties.
        if key != Key::VoteAnonymousId
            && key != Key::AnimateTrayIcon
            && key != Key::ShowProjectLinks
            && key != Key::CheckGitHubForUpdates
            && key != Key::NotifyUpdates
            && key != Key::UpdateNotifiedVersion
        {
            result.insert(key.name().into(), encoded(&settings.get(key, context)));
        }
    }
    result.insert(
        "HasStoredTelemetryConsent".into(),
        json!(settings.has_telemetry_consent()),
    );
    result.insert(
        "IsEffectiveEcoMode".into(),
        json!(settings.effective_eco_mode(context)),
    );
    result.insert(
        "EffectivePeakMeterFps".into(),
        json!(settings.effective_peak_fps(context)),
    );
    Json::Object(result)
}

fn stored(settings: &Settings) -> Json {
    Json::Object(
        settings
            .values
            .iter()
            .map(|(key, value)| (key.clone(), encoded(value)))
            .collect(),
    )
}

#[test]
fn dynamic_device_and_vote_keys_preserve_case_whitespace_and_null_behavior() {
    for case in reference()["dynamicKeys"].as_array().unwrap() {
        let id = case["id"].as_str();
        let mut settings = Settings::default();
        assert_eq!(json!(settings.device_hotkey(id)), case["beforeHotkey"]);
        assert_eq!(json!(settings.poll_vote(id)), case["beforeVote"]);
        let hotkey = Some(bt_core::models::HotkeyData {
            modifiers: 131072,
            key: 80,
        });
        let change = settings.set_device_hotkey(id, hotkey);
        let wrote_vote = settings.set_poll_vote(id, Some("{\"question\":\"option\"}"));
        assert_eq!(change.wrote, wrote_vote);
        assert_eq!(change.wrote, id.is_some_and(|id| !id.is_empty()));
        assert_eq!(stored(&settings), case["firstStored"]);
        assert_eq!(json!(settings.device_hotkey(id)), case["firstHotkey"]);
        assert_eq!(json!(settings.poll_vote(id)), case["firstVote"]);
        settings.set_device_hotkey(id, None);
        settings.set_poll_vote(id, None);
        assert_eq!(stored(&settings), case["finalStored"]);
        assert_eq!(json!(settings.device_hotkey(id)), case["finalHotkey"]);
        assert_eq!(json!(settings.poll_vote(id)), case["finalVote"]);
    }
}

#[test]
fn scalar_defaults_and_region_exclusion_list_match_csharp() {
    let reference = reference();
    assert_eq!(snapshot(&Settings::default(), false), reference["defaults"]);
    for case in reference["regions"].as_array().unwrap() {
        let settings = Settings::default();
        let ctx = Context {
            region_code: case["region"].as_str().unwrap(),
            on_battery: false,
        };
        assert_eq!(
            encoded(&settings.get(Key::IsTelemetryEnabled, ctx)),
            case["value"]
        );
        assert!(!settings.has_telemetry_consent());
        assert!(settings.values.is_empty());
    }
}

#[test]
fn settings_get_set_clamps_batch_notifications_and_raw_storage_match_source() {
    for scenario in reference()["scenarios"].as_array().unwrap() {
        let mut settings = Settings::default();
        for (name, value) in scenario["input"]["initial"].as_object().unwrap() {
            let key = *Key::ALL.iter().find(|k| k.storage_key() == name).unwrap();
            let template = if key == Key::UseLegacyIcon {
                Value::Text(None)
            } else {
                key.default_value()
            };
            settings
                .values
                .insert(name.clone(), decoded(value, template));
        }
        assert_eq!(snapshot(&settings, false), scenario["before"]);
        for step in scenario["steps"].as_array().unwrap() {
            let input = &step["input"];
            let (wrote, events) = match input["batch"].as_str() {
                Some("begin") => {
                    settings.begin_batch();
                    (false, vec![])
                }
                Some("end") => (false, vec![format!("{:?}", settings.end_batch())]),
                _ => {
                    let key = key(input["key"].as_str().unwrap());
                    let value = decoded(&input["value"], key.default_value());
                    let change = settings.set(key, value, context(false)).unwrap();
                    (
                        change.wrote,
                        change.events.iter().map(|e| format!("{e:?}")).collect(),
                    )
                }
            };
            assert_eq!(json!(wrote), step["wrote"], "{input}");
            assert_eq!(json!(events), step["events"], "{input}");
            assert_eq!(snapshot(&settings, false), step["values"], "{input}");
            let stored: serde_json::Map<String, Json> = settings
                .values
                .iter()
                .map(|(k, v)| (k.clone(), encoded(v)))
                .collect();
            assert_eq!(Json::Object(stored), step["stored"], "{input}");
        }
        assert_eq!(snapshot(&settings, true), scenario["onBattery"]);
    }
}

#[test]
fn vote_id_is_lazy_stable_and_independent_of_telemetry_id() {
    let mut settings = Settings::default();
    let mut allocations = 0;
    let mut allocate = || {
        allocations += 1;
        "0123456789abcdef0123456789abcdef".into()
    };
    assert!(settings.values.is_empty());
    let first = settings.vote_id(&mut allocate);
    let second = settings.vote_id(&mut allocate);
    assert_eq!(
        json!({"writes":allocations,"stable":first == second,"length":first.len()}),
        reference()["vote"]
    );
    assert!(!settings.values.contains_key("TelemetryAnonymousId"));
    assert!(!settings.has_telemetry_consent());
}

#[test]
fn typed_setter_rejects_wrong_types_and_preserves_other_keys() {
    let mut settings = Settings::default();
    settings
        .values
        .insert("FutureSetting".into(), Value::Text(Some("keep".into())));
    assert!(
        settings
            .set(Key::PeakMeterFps, Value::Bool(true), context(false))
            .is_err()
    );
    assert_eq!(settings.values.len(), 1);
    let hotkey = Some(bt_core::models::HotkeyData {
        modifiers: 131072,
        key: 80,
    });
    let change = settings
        .set(Key::FlyoutHotkey, Value::Hotkey(hotkey), context(false))
        .unwrap();
    assert_eq!(
        change.hotkey_replacement,
        Some((Some(Default::default()), hotkey))
    );
    assert!(settings.values.contains_key("Hotkey"));
    assert!(!settings.values.contains_key("FlyoutHotkey"));
}
