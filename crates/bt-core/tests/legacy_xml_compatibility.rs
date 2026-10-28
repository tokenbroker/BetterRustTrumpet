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

#[test]
fn hotkey_encoding_matches_xml_serializer_samples_and_round_trips() {
    use bt_core::models::HotkeyData;
    let decode = |xml: &str| match legacy_xml::decode(xml, Kind::Hotkey).unwrap() {
        legacy_xml::Value::Hotkey(h) => h,
        other => panic!("{other:?}"),
    };
    for case in reference()["samples"].as_array().unwrap() {
        if case["kind"] != "Hotkey" {
            continue;
        }
        let xml = case["xml"].as_str().unwrap();
        assert_eq!(legacy_xml::encode_hotkey(decode(xml)), xml);
    }
    for (modifiers, key) in [
        (0x7_0000, 112),
        (0x1_0000 | 91, 77),
        (0x2_0000, 13),
        (0x4_0000, 33),
        (0, 0x7),
    ] {
        let value = Some(HotkeyData { modifiers, key });
        assert_eq!(decode(&legacy_xml::encode_hotkey(value)), value);
    }
}
