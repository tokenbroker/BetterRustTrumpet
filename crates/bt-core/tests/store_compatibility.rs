use bt_core::{
    legacy_store::{self, PackageValue, PortableStore, Read, RegistryValue},
    legacy_xml::{Kind, Value},
    newtonsoft::LocalZone,
};
use serde_json::{Value as Json, json};

fn reference() -> Json {
    serde_json::from_str(include_str!("fixtures/store_reference.json")).unwrap()
}

const TICKS_PER_DAY: i64 = 864_000_000_000;
const TICKS_PER_HOUR: i64 = 36_000_000_000;

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let leap = |y: i64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let y = year - 1;
    let mut days = y * 365 + y / 4 - y / 100 + y / 400;
    for m in 1..month {
        days += match m {
            2 if leap(year) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
    }
    days + day - 1
}
/// nth Sunday of a month as days since 0001-01-01 (a Monday).
fn sunday(year: i64, month: i64, nth: i64) -> i64 {
    let first = days_from_civil(year, month, 1);
    let to_sunday = (7 - (first + 1) % 7) % 7;
    first + to_sunday + (nth - 1) * 7
}
fn year_of(ticks: i64) -> i64 {
    let mut year = ticks / (TICKS_PER_DAY * 365) + 1;
    while days_from_civil(year, 1, 1) * TICKS_PER_DAY > ticks {
        year -= 1;
    }
    while days_from_civil(year + 1, 1, 1) * TICKS_PER_DAY <= ticks {
        year += 1;
    }
    year
}
/// US Central time (the capture machine's zone) with post-2007 rules.
/// `zone_rules_match_captured_windows_offsets` checks it against Windows.
struct Central;
impl LocalZone for Central {
    fn offset_for_utc(&self, utc: i64) -> i32 {
        let year = year_of(utc);
        let start = sunday(year, 3, 2) * TICKS_PER_DAY + 8 * TICKS_PER_HOUR;
        let end = sunday(year, 11, 1) * TICKS_PER_DAY + 7 * TICKS_PER_HOUR;
        if (start..end).contains(&utc) {
            -300
        } else {
            -360
        }
    }
    fn offset_for_local(&self, wall: i64) -> i32 {
        // Invalid and ambiguous wall times use the standard offset.
        let year = year_of(wall);
        let start = sunday(year, 3, 2) * TICKS_PER_DAY + 3 * TICKS_PER_HOUR;
        let end = sunday(year, 11, 1) * TICKS_PER_DAY + TICKS_PER_HOUR;
        if (start..end).contains(&wall) {
            -300
        } else {
            -360
        }
    }
}

#[test]
fn zone_rules_match_captured_windows_offsets() {
    let reference = reference();
    assert_eq!(reference["localTimeZone"], "Central Standard Time");
    for entry in reference["localOffsets"].as_array().unwrap() {
        let ticks = entry["utcTicks"].as_i64().unwrap();
        assert_eq!(
            f64::from(Central.offset_for_utc(ticks)),
            entry["offsetMinutes"].as_f64().unwrap(),
            "{entry}"
        );
    }
    for entry in reference["localWallOffsets"].as_array().unwrap() {
        let ticks = entry["wallTicks"].as_i64().unwrap();
        assert_eq!(
            f64::from(Central.offset_for_local(ticks)),
            entry["offsetMinutes"].as_f64().unwrap(),
            "{entry}"
        );
    }
}

const GETS: &[&str] = &[
    "Bool",
    "BoolTrue",
    "I32",
    "F64",
    "F64Bits",
    "String",
    "Hotkey",
    "OptionalPlacement",
];
fn kind(get: &str) -> Kind {
    match get {
        "Bool" | "BoolTrue" => Kind::Bool,
        "I32" => Kind::I32,
        "F64" | "F64Bits" => Kind::F64,
        "String" => Kind::String,
        "Hotkey" => Kind::Hotkey,
        _ => Kind::OptionalPlacement,
    }
}
/// The capture's defaults: false/true, 30, 0.7, "fallback", empty hotkey, null.
fn expected(get: &str, read: Read) -> Json {
    match read {
        Read::Throws => json!({"errorType": "InvalidOperationException"}),
        Read::Default => json!({"value": match get {
            "Bool" => json!(false),
            "BoolTrue" => json!(true),
            "I32" => json!(30),
            "F64" => json!(0.7),
            "F64Bits" => json!(0.7_f64.to_bits() as i64),
            "String" => json!("fallback"),
            "Hotkey" => json!({"Modifiers": 0, "Key": 0, "IsEmpty": true}),
            _ => Json::Null,
        }}),
        Read::Value(Value::F64(f)) if get == "F64Bits" => json!({"value": f.to_bits() as i64}),
        Read::Value(value) => json!({"value": value.to_json()}),
    }
}
fn compare(
    label: &str,
    gets: &Json,
    mut read: impl FnMut(Kind) -> Read,
    failures: &mut Vec<String>,
) {
    for get in GETS {
        let actual = expected(get, read(kind(get)));
        if actual != gets[*get] {
            failures.push(format!("{label} {get}: {actual} != {}", gets[*get]));
        }
    }
}

fn registry(raw: &Json) -> Option<RegistryValue> {
    let value = &raw["value"];
    Some(match raw["type"].as_str()? {
        "String" => RegistryValue::String(value.as_str()?.to_owned()),
        "DWord" => RegistryValue::DWord(value.as_u64()? as u32),
        "QWord" => RegistryValue::QWord(value.as_u64()?),
        "Binary" => RegistryValue::Binary(
            value
                .as_array()?
                .iter()
                .map(|b| b.as_u64().unwrap() as u8)
                .collect(),
        ),
        "MultiString" => RegistryValue::MultiString(
            value
                .as_array()?
                .iter()
                .map(|s| s.as_str().unwrap().to_owned())
                .collect(),
        ),
        other => panic!("{other}"),
    })
}
fn package(raw: &Json) -> Option<PackageValue> {
    Some(match raw["type"].as_str()? {
        "String" => PackageValue::String(raw["value"].as_str()?.to_owned()),
        "Bool" => PackageValue::Bool(raw["value"].as_bool()?),
        _ => PackageValue::Other,
    })
}

#[test]
fn registry_and_package_bags_cast_decode_and_throw_like_the_original() {
    let reference = reference();
    let mut failures = vec![];
    for case in reference["registry"].as_array().unwrap() {
        let raw = registry(&case["raw"]);
        assert_eq!(raw.is_some(), case["hasKey"].as_bool().unwrap());
        compare(
            &format!("registry {}", case["raw"]),
            &case["gets"],
            |k| legacy_store::read_registry(raw.as_ref(), k),
            &mut failures,
        );
    }
    for case in reference["package"].as_array().unwrap() {
        let raw = package(&case["raw"]);
        assert_eq!(raw.is_some(), case["hasKey"].as_bool().unwrap());
        compare(
            &format!("package {}", case["raw"]),
            &case["gets"],
            |k| legacy_store::read_package(raw.as_ref(), k),
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn portable_json_bag_parses_converts_and_falls_back_like_newtonsoft() {
    let reference = reference();
    let mut failures = vec![];
    for doc in reference["portable"].as_array().unwrap() {
        let input = &doc["input"];
        let bytes = input["text"].as_str().map(|t| t.as_bytes().to_vec());
        let store = PortableStore::load(bytes.as_deref(), &Central);
        for (key, case) in doc["keys"].as_object().unwrap() {
            let key = match input["namespace"].as_str() {
                Some(ns) => legacy_store::namespaced_key(ns, key),
                None => key.clone(),
            };
            let label = format!("{} {key:?}", input["name"]);
            if store.has_key(&key) != case["hasKey"].as_bool().unwrap() {
                failures.push(format!("{label} hasKey"));
            }
            compare(
                &label,
                &case["gets"],
                |k| store.read(&key, k),
                &mut failures,
            );
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
