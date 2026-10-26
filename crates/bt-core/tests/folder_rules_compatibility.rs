mod support;

use bt_core::{
    folder_rules::{self, Environment, FolderRuleStore},
    folders::{PathError, WindowsPathResolver},
};
use serde_json::{Value, json};

use support::{Central, TICKS_PER_DAY, days_from_civil};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/folder_rules_reference.json")).unwrap()
}

/// `Path.GetFullPath` with the capture's normalized cwd (C:\Reference\Workspace)
/// for the relative forms the fixtures contain: plain names on the current
/// drive and drive-relative `X:` on another drive (no per-drive cwd).
struct Workspace;
impl WindowsPathResolver for Workspace {
    fn full_path(&self, value: &str) -> Result<String, PathError> {
        let bytes = value.as_bytes();
        if bytes.len() == 2 && bytes[1] == b':' && !bytes[0].eq_ignore_ascii_case(&b'C') {
            return Ok(format!("{value}\\"));
        }
        if value.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Ok(format!(r"C:\Reference\Workspace\{value}"));
        }
        panic!("unmodelled relative path {value:?}")
    }
}

struct Ids(u32);
impl Ids {
    fn next(&mut self) -> String {
        self.0 += 1;
        format!("generated{:02}", self.0)
    }
}
fn now_ticks(reference: &Value) -> i64 {
    // "2030-01-02T03:04:05.1234567Z"
    let text = reference["generatedNow"].as_str().unwrap();
    assert_eq!(text, "\"2030-01-02T03:04:05.1234567Z\"");
    days_from_civil(2030, 1, 2) * TICKS_PER_DAY + (3 * 3600 + 4 * 60 + 5) * 10_000_000 + 1_234_567
}

#[test]
fn folder_rule_lists_deserialize_like_newtonsoft() {
    let reference = reference();
    let today = reference["todayTicks"].as_i64().unwrap();
    let mut failures = vec![];
    for case in reference["loads"].as_array().unwrap() {
        let input = case["input"].as_str();
        let actual = match folder_rules::deserialize(input.unwrap_or("[]"), &Central, today) {
            Ok(None) => json!({"value": "null"}),
            Ok(Some(list)) => json!({"value": folder_rules::serialize_optional(&list)}),
            Err(_) => json!({"errorType": "error"}),
        };
        let expected = &case["deserialize"];
        let matches = match expected.get("errorType") {
            Some(_) => actual.get("errorType").is_some(),
            None => actual == *expected,
        };
        if !matches {
            failures.push(format!("{input:?}: {actual} != {expected}"));
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
fn folder_rule_loading_normalization_and_ordering_match_app_settings() {
    let reference = reference();
    let today = reference["todayTicks"].as_i64().unwrap();
    let now = now_ticks(&reference);
    let mut failures = vec![];
    for case in reference["loads"].as_array().unwrap() {
        let mut ids = Ids(0);
        let mut next = || ids.next();
        let mut env = Environment {
            resolver: &Workspace,
            zone: &Central,
            today_ticks: today,
            now_utc_ticks: now,
            new_id: &mut next,
        };
        let mut store = FolderRuleStore::new(case["input"].as_str().map(str::to_owned));
        let sorted = folder_rules::serialize(&store.sorted(&mut env));
        let json = store.rules_json(&mut env);
        let actual = json!({"sorted": sorted, "json": json, "saved": false});
        let expected =
            json!({"sorted": case["sorted"], "json": case["json"], "saved": case["saved"]});
        if actual != expected {
            failures.push(format!("{:?}: {actual} != {expected}", case["input"]));
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
fn folder_rule_lifecycle_saves_notifies_and_matches_like_app_settings() {
    let reference = reference();
    let today = reference["todayTicks"].as_i64().unwrap();
    let now = now_ticks(&reference);
    let mut failures = vec![];
    for scenario in reference["scenarios"].as_array().unwrap() {
        let mut ids = Ids(0);
        let mut next = || ids.next();
        let mut env = Environment {
            resolver: &Workspace,
            zone: &Central,
            today_ticks: today,
            now_utc_ticks: now,
            new_id: &mut next,
        };
        let mut store = FolderRuleStore::new(scenario["initial"].as_str().map(str::to_owned));
        for step in scenario["steps"].as_array().unwrap() {
            let op = step["op"].as_array().unwrap();
            let s = |i: usize| op[i].as_str();
            let n = |i: usize| op[i].as_i64().unwrap() as i32;
            let mut result = Value::Null;
            let outcome = match s(0).unwrap() {
                "add" => store.add(s(1), n(2), &mut env).unwrap(),
                "addDefault" => store.add(s(1), 5, &mut env).unwrap(),
                "update" => store.update(s(1), s(2), n(3), &mut env).unwrap(),
                "remove" => store.remove(s(1), &mut env),
                "set" => store.set_json(s(1)),
                "match" => {
                    let matched = store.try_match(s(1), &mut env).unwrap();
                    result = json!({"matched": matched.is_some(), "volume": matched.unwrap_or(0)});
                    Default::default()
                }
                other => panic!("{other}"),
            };
            let actual = json!({
                "events": u32::from(outcome.changed), "saved": outcome.saved, "stored": store.stored(),
                "sorted": folder_rules::serialize(&store.sorted(&mut env)), "result": result,
            });
            let expected = json!({
                "events": step["events"], "saved": step["saved"], "stored": step["stored"],
                "sorted": step["sorted"], "result": step["result"],
            });
            if actual != expected {
                failures.push(format!("{op:?}: {actual} != {expected}"));
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
