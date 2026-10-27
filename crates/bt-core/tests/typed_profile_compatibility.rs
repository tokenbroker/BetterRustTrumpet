mod support;
use bt_core::{profiles, typed_profiles};
use serde_json::{Value, json};

#[test]
fn typed_profile_records_and_normalization_match_original_loaders() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/typed_lists_reference.json")).unwrap();
    let mut failures = vec![];
    for case in reference["profiles"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let actual = typed_profiles::decode(input, &support::Central);
        let expected = case["raw"]["value"]
            .as_str()
            .map(|text| serde_json::from_str::<Value>(text).unwrap());
        match (actual, expected) {
            (Ok(actual), Some(expected)) if json!(actual) == expected => {}
            (Err(_), None) => {}
            (actual, expected) => failures.push(format!("raw {input}: {actual:?} != {expected:?}")),
        }
        let mut index = 0;
        let actual = profiles::parse(input, &support::Central, &mut || {
            let id = format!("captured-id-{index}");
            index += 1;
            id
        })
        .map(|p| json!({"profiles": p.profiles, "assignedIds": p.assigned_ids}));
        let expected = case["result"]["value"]
            .as_str()
            .map(|text| serde_json::from_str::<Value>(text).unwrap());
        match (actual, expected) {
            (Ok(actual), Some(expected)) if actual == expected => {}
            (Err(_), None) => {}
            (actual, expected) => {
                failures.push(format!("loaded {input}: {actual:?} != {expected:?}"))
            }
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
fn hand_edited_single_profile_imports_match_original() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/typed_lists_reference.json")).unwrap();
    let mut failures = vec![];
    for case in reference["profileImports"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let mut collection = profiles::ProfileCollection::new(vec![]);
        let index = collection
            .import_json(input, &support::Central, &mut || {
                panic!("every valid import case supplies an ID")
            })
            .unwrap_or_default();
        let actual = index.map(|index| &collection.profiles()[index]);
        let expected: Value =
            serde_json::from_str(case["result"]["value"].as_str().unwrap()).unwrap();
        if json!(actual) != expected {
            failures.push(format!("{input}: {} != {expected}", json!(actual)));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
