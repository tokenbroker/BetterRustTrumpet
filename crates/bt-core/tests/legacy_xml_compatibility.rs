use bt_core::legacy_xml::{self, Kind};
use serde_json::{Value, json};
fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/legacy_storage_reference.json")).unwrap()
}
fn kind(value: &Value) -> Kind {
    serde_json::from_value(value.clone()).unwrap()
}
fn result(xml: &str, kind: Kind) -> Value {
    match legacy_xml::decode(xml, kind) {
        Ok(value) => json!({"value":value.to_json()}),
        Err(_) => json!({"errorType":"InvalidOperationException"}),
    }
}

#[test]
fn original_xml_serializer_scalar_hotkey_and_placement_outputs_decode() {
    for case in reference()["samples"].as_array().unwrap() {
        assert_eq!(
            result(case["xml"].as_str().unwrap(), kind(&case["kind"])),
            case["result"],
            "{case}"
        );
    }
}
#[test]
fn xml_reader_values_namespaces_nulls_duplicates_and_failures_match_source() {
    let reference = reference();
    let mismatches: Vec<_> = reference["parses"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|case| {
            let input = &case["input"];
            let actual = result(input["xml"].as_str().unwrap(), kind(&input["kind"]));
            (actual != case["result"])
                .then(|| format!("{input} => {actual}, expected {}", case["result"]))
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "{}",
        mismatches.join(
            "
"
        )
    );
}
