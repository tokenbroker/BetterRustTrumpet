mod support;

use bt_core::typed_lists;
use serde_json::{Value, json};
use support::Central;

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/typed_lists_reference.json")).unwrap()
}
/// A captured result: the serialized value, or the loader's empty fallback
/// when the original threw (the hidden-list capture has no catch of its own).
fn expected(result: &Value, empty: Value) -> Value {
    match result["value"].as_str() {
        Some(text) => serde_json::from_str(text).unwrap(),
        None => empty,
    }
}

#[test]
fn hand_edited_app_rules_and_legacy_hard_mutes_load_like_the_original() {
    let reference = reference();
    let today = reference["todayTicks"].as_i64().unwrap();
    let mut failures = vec![];
    for case in reference["apps"].as_array().unwrap() {
        let input = &case["input"];
        let loaded = typed_lists::load_app_rules(
            input["current"].as_str(),
            input["legacy"].as_str(),
            &Central,
            today,
        );
        let actual = json!({"entries": loaded.entries, "saveMigration": loaded.save_migration});
        let expected = expected(&case["result"], json!(null));
        if actual != expected {
            failures.push(format!("{input}: {actual} != {expected}"));
        }
        // The raw DeserializeObject result, before normalization.
        let raw = typed_lists::decode_app_rules(
            input["current"].as_str().unwrap_or("[]"),
            &Central,
            today,
        );
        let raw_matches = match (&raw, case["raw"]["value"].as_str()) {
            (Ok(list), Some(text)) => json!(list) == serde_json::from_str::<Value>(text).unwrap(),
            (Err(_), None) => true,
            _ => false,
        };
        if !raw_matches {
            failures.push(format!("raw {input}: {raw:?} != {}", case["raw"]));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn hand_edited_hidden_lists_load_like_the_original() {
    let reference = reference();
    let today = reference["todayTicks"].as_i64().unwrap();
    let mut failures = vec![];
    for case in reference["hiddenApps"].as_array().unwrap() {
        let actual = json!(typed_lists::load_hidden_apps(
            case["input"].as_str(),
            &Central,
            today
        ));
        let expected = expected(&case["result"], json!([]));
        if actual != expected {
            failures.push(format!("apps {}: {actual} != {expected}", case["input"]));
        }
    }
    for case in reference["hiddenDevices"].as_array().unwrap() {
        let actual = json!(typed_lists::load_hidden_devices(
            case["input"].as_str(),
            &Central,
            today
        ));
        let expected = expected(&case["result"], json!([]));
        if actual != expected {
            failures.push(format!("devices {}: {actual} != {expected}", case["input"]));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
