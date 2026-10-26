use bt_core::{
    folder_rules::{self, Environment, FolderRuleStore},
    folders::{PathError, WindowsPathResolver},
    newtonsoft::LocalZone,
};
use serde_json::{Value, json};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/folder_rules_reference.json")).unwrap()
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
fn sunday(year: i64, month: i64, nth: i64) -> i64 {
    let first = days_from_civil(year, month, 1);
    first + (7 - (first + 1) % 7) % 7 + (nth - 1) * 7
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
/// US Central rules, verified against Windows in store_compatibility.rs.
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
