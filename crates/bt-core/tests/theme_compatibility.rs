mod support;
use bt_core::themes::Theme;
use serde_json::Value;

fn exported(theme: &Theme) -> Value {
    serde_json::from_str(&theme.to_json().unwrap()).unwrap()
}

#[test]
fn dynamic_theme_imports_match_original() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/typed_lists_reference.json")).unwrap();
    let mut failures = vec![];
    for case in reference["themes"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let actual = Theme::from_json(input, &support::Central).map(|theme| exported(&theme));
        let expected = case["result"]["value"]
            .as_str()
            .map(|s| serde_json::from_str::<Value>(s).unwrap());
        match (actual, expected) {
            (Ok(actual), Some(expected)) if actual == expected => {}
            (Err(_), None) => {}
            (actual, expected) => failures.push(format!("{input}: {actual:?} != {expected:?}")),
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
fn saved_theme_list_parse_and_item_failure_boundaries_match_original() {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/typed_lists_reference.json")).unwrap();
    for case in reference["themeLists"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let actual: Vec<Value> = Theme::load_custom(input, &support::Central)
            .iter()
            .map(exported)
            .collect();
        let expected: Value =
            serde_json::from_str(case["result"]["value"].as_str().unwrap()).unwrap();
        assert_eq!(serde_json::json!(actual), expected, "{input}");
    }
}
