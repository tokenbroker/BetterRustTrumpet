mod support;

use bt_core::typed_lists;
use serde_json::{Value, json};
use support::Central;

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/models_reference.json")).unwrap()
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
// Canonical fixture dates are ISO with Z, so "today" is never consulted.
const TODAY: i64 = 0;

#[test]
fn app_rule_cleanup_and_legacy_fallback_match_original_loader() {
    for case in fixtures()["ruleNormalization"]["rules"].as_array().unwrap() {
        let input = &case["input"];
        let legacy = (!input["legacy"].is_null()).then(|| text(&input["legacy"]));
        let result = typed_lists::load_app_rules(
            Some(&text(&input["current"])),
            legacy.as_deref(),
            &Central,
            TODAY,
        );
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
        let result = typed_lists::load_hidden_apps(Some(&text(&case["input"])), &Central, TODAY);
        assert_eq!(json!(result), case["result"]["value"]);
    }
}

#[test]
fn hidden_device_cleanup_matches_source() {
    for case in fixtures()["ruleNormalization"]["hiddenDevices"]
        .as_array()
        .unwrap()
    {
        let result = typed_lists::load_hidden_devices(Some(&text(&case["input"])), &Central, TODAY);
        assert_eq!(json!(result), case["result"]["value"]);
    }
}
