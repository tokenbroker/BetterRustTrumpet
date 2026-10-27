use bt_core::{
    legacy_xml::{Kind, Value},
    models::HotkeyData,
    registry_snapshot::{RawRegistryValue, RegistrySnapshot},
    registry_values::{self, Environment, ExpansionError, Object, Read, Request},
};
use serde_json::{Value as Json, json};
use std::collections::HashMap;

struct CapturedEnvironment {
    expansions: HashMap<Vec<u16>, Vec<u16>>,
    equal: HashMap<(Vec<u16>, Vec<u16>), bool>,
}
impl Environment for CapturedEnvironment {
    fn names_equal(&self, a: &[u16], b: &[u16]) -> bool {
        a == b
            || self
                .equal
                .get(&(a.to_vec(), b.to_vec()))
                .copied()
                .unwrap_or(false)
    }
    fn expand(&self, text: &[u16]) -> Result<Vec<u16>, ExpansionError> {
        self.expansions.get(text).cloned().ok_or(ExpansionError)
    }
}
fn units(value: &Json) -> Vec<u16> {
    serde_json::from_value(value.clone()).unwrap()
}
fn fixture() -> (Json, CapturedEnvironment) {
    let all: Json =
        serde_json::from_str(include_str!("fixtures/registry_projection_reference.json")).unwrap();
    let reference = all;
    let mut env = CapturedEnvironment {
        expansions: HashMap::new(),
        equal: HashMap::new(),
    };
    for c in reference["values"].as_array().unwrap() {
        if c["input"]["kind"] == 2 && c["unexpanded"]["type"] == "String" {
            let a = units(&c["unexpanded"]["units"]);
            let b = units(&c["expanded"]["units"]);
            if let Some(old) = env.expansions.insert(a, b.clone()) {
                assert_eq!(old, b);
            }
        }
    }
    for c in reference["lookups"].as_array().unwrap() {
        let stored: Vec<_> = c["input"]["stored"]
            .as_str()
            .unwrap()
            .encode_utf16()
            .collect();
        for q in c["queries"].as_array().unwrap() {
            let query = q["query"]
                .as_str()
                .unwrap_or_default()
                .encode_utf16()
                .take_while(|c| *c != 0)
                .collect();
            assert_eq!(q["rtlEqual"], q["hasKey"]);
            env.equal
                .insert((stored.clone(), query), q["rtlEqual"].as_bool().unwrap());
        }
    }
    (reference, env)
}
fn object(value: Option<Object>) -> Json {
    match value {
        None => Json::Null,
        Some(Object::String(s)) => json!({"type":"String","units":s}),
        Some(Object::DWord(bits)) => json!({"type":"DWord","bits":bits}),
        Some(Object::QWord(bits)) => json!({"type":"QWord","bits":bits}),
        Some(Object::Binary(bytes)) => json!({"type":"Binary","bytes":bytes}),
        Some(Object::MultiString(strings)) => json!({"type":"MultiString","strings":strings}),
    }
}
fn projected(value: Read, default: Json) -> Json {
    let value = match value {
        Read::StringUtf16(s) => return json!({"stringUnits":s}),
        Read::Value(Value::String(Some(s))) => {
            return json!({"stringUnits":s.encode_utf16().collect::<Vec<_>>()});
        }
        Read::Value(value) => value.to_json(),
        Read::Default => default,
        Read::Throws => return json!({"errorType":"InvalidOperationException"}),
    };
    if let Some(s) = value.as_str() {
        json!({"stringUnits":s.encode_utf16().collect::<Vec<_>>()})
    } else {
        json!({"value":value})
    }
}

#[test]
fn raw_registry_values_and_original_bag_gets_match_dotnet() {
    let (reference, env) = fixture();
    let requests = [
        ("Bool", Request::Value(Kind::Bool), json!(false)),
        ("BoolTrue", Request::Value(Kind::Bool), json!(true)),
        ("I32", Request::Value(Kind::I32), json!(30)),
        ("F64", Request::Value(Kind::F64), json!(0.7)),
        ("String", Request::Value(Kind::String), json!("fallback")),
        ("NullDefaultString", Request::NullDefaultString, Json::Null),
        (
            "Hotkey",
            Request::Value(Kind::Hotkey),
            json!(HotkeyData::default()),
        ),
        (
            "OptionalPlacement",
            Request::Value(Kind::OptionalPlacement),
            Json::Null,
        ),
    ];
    let mut failures = vec![];
    for (index, c) in reference["values"].as_array().unwrap().iter().enumerate() {
        let raw = RawRegistryValue {
            name_utf16: vec![b'k' as u16],
            kind: c["input"]["kind"].as_u64().unwrap() as u32,
            bytes: serde_json::from_value(c["input"]["bytes"].clone()).unwrap(),
        };
        for (expand, field) in [(false, "unexpanded"), (true, "expanded")] {
            let actual = registry_values::project(&raw, expand, &env).map(object);
            if actual.as_ref().ok() != Some(&c[field]) {
                failures.push(format!("case {index} {field}: projection mismatch"));
            }
        }
        let snapshot = RegistrySnapshot {
            key_existed: true,
            values: vec![raw],
        };
        let has_key = snapshot.has_key(Some("k"), &env);
        if has_key.as_ref().ok() != c["hasKey"].as_bool().as_ref() {
            failures.push(format!("case {index} has_key: {has_key:?}"));
        }
        for (kind, request, default) in &requests {
            let actual = projected(snapshot.read(Some("k"), *request, &env), default.clone());
            if actual != c["gets"][kind] {
                failures.push(format!("case {index} {kind}: getter mismatch"));
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
fn registry_lookup_default_names_and_nul_truncation_match_windows() {
    let (reference, env) = fixture();
    for c in reference["lookups"].as_array().unwrap() {
        let raw = RawRegistryValue {
            name_utf16: c["input"]["stored"]
                .as_str()
                .unwrap()
                .encode_utf16()
                .collect(),
            kind: 1,
            bytes: "hit\0".encode_utf16().flat_map(u16::to_le_bytes).collect(),
        };
        let snapshot = RegistrySnapshot {
            key_existed: true,
            values: vec![raw],
        };
        for q in c["queries"].as_array().unwrap() {
            let name = q["query"].as_str();
            assert_eq!(
                snapshot.has_key(name, &env).unwrap(),
                q["hasKey"].as_bool().unwrap(),
                "{q}"
            );
            let actual = match snapshot.read(name, Request::Value(Kind::String), &env) {
                Read::StringUtf16(s) => String::from_utf16(&s).unwrap(),
                Read::Default => "fallback".into(),
                other => panic!("{other:?}"),
            };
            assert_eq!(json!(actual), q["get"]["value"], "{q}");
        }
    }
}

#[test]
fn expansion_failure_escapes_has_key_but_defaults_the_bag_getter() {
    struct Failing;
    impl Environment for Failing {
        fn names_equal(&self, a: &[u16], b: &[u16]) -> bool {
            a == b
        }
        fn expand(&self, _: &[u16]) -> Result<Vec<u16>, ExpansionError> {
            Err(ExpansionError)
        }
    }
    let snapshot = RegistrySnapshot {
        key_existed: true,
        values: vec![RawRegistryValue {
            name_utf16: vec![107],
            kind: 2,
            bytes: "text\0".encode_utf16().flat_map(u16::to_le_bytes).collect(),
        }],
    };
    assert!(snapshot.has_key(Some("k"), &Failing).is_err());
    assert_eq!(
        snapshot.read(Some("k"), Request::Value(Kind::String), &Failing),
        Read::Default
    );
}
