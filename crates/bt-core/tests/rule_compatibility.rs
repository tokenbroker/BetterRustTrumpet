use bt_core::rules;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/models_reference.json")).unwrap()
}

// Mirrors the loader's catch/null boundary, not Newtonsoft's full grammar.
// Permissive scalar/DateTime decoding remains a separate compatibility gate.
fn parsed<T: DeserializeOwned>(value: &Value) -> Vec<Option<T>> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    serde_json::from_str::<Option<Vec<Option<T>>>>(&text)
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[test]
fn app_rule_cleanup_and_legacy_fallback_match_original_loader() {
    for case in fixtures()["ruleNormalization"]["rules"].as_array().unwrap() {
        let input = &case["input"];
        let result = rules::load_apps(parsed(&input["current"]), Some(parsed(&input["legacy"])));
        assert_eq!(
            json!({"entries":result.entries,"saveMigration":result.save_migration}),
            case["result"]["value"],
            "{input}"
        );
    }
}

#[test]
fn hidden_app_cleanup_preserves_legacy_delimiter_collisions() {
    for case in fixtures()["ruleNormalization"]["hiddenApps"]
        .as_array()
        .unwrap()
    {
        let result = rules::normalize_hidden_apps(parsed(&case["input"]));
        assert_eq!(json!(result), case["result"]["value"]);
    }
}

#[test]
fn hidden_device_cleanup_matches_source() {
    for case in fixtures()["ruleNormalization"]["hiddenDevices"]
        .as_array()
        .unwrap()
    {
        let result = rules::normalize_hidden_devices(parsed(&case["input"]));
        assert_eq!(json!(result), case["result"]["value"]);
    }
}
