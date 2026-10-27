mod support;
use bt_core::{
    app_rules::{AppRules, Effect, Environment, Metadata, Mutation},
    models::VolumeRuleMode,
    newtonsoft::{self, Token},
};
use serde_json::{Value, json};
use std::collections::HashMap;

fn json_or_raw(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| json!(text))
}

#[test]
fn rule_mutations_cache_migration_saves_and_events_match_original() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/typed_lists_reference.json")).unwrap();
    let reference = &fixture["appLifecycle"];
    let comparisons: HashMap<_, _> = reference["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                (c[0].as_str().unwrap(), c[1].as_str().unwrap()),
                c[2].as_i64().unwrap().cmp(&0),
            )
        })
        .collect();
    let Token::Date(now) =
        newtonsoft::parse_dynamic_document(&json!(reference["now"]).to_string(), &support::Central)
            .unwrap()
            .unwrap()
    else {
        panic!()
    };
    let env = Environment {
        zone: &support::Central,
        today_ticks: fixture["todayTicks"].as_i64().unwrap(),
        now_utc: now,
    };
    for scenario in reference["scenarios"].as_array().unwrap() {
        let mut rules = AppRules::new(
            scenario["initial"].as_str().map(str::to_owned),
            scenario["legacy"].as_str().map(str::to_owned),
        );
        for step in scenario["steps"].as_array().unwrap() {
            let op = step["op"].as_array().unwrap();
            let string = |n| op.get(n).and_then(Value::as_str);
            let metadata = |offset| Metadata {
                exe_name: string(1),
                display_name: string(offset),
                icon_path: string(offset + 1),
                is_desktop_app: op.get(offset + 2).and_then(Value::as_bool),
            };
            let mut result = Value::Null;
            match string(0).unwrap() {
                "add" => rules.change(metadata(2), Mutation::Add, &env),
                "hard" => rules.change(
                    metadata(3),
                    Mutation::HardMute(op[2].as_bool().unwrap()),
                    &env,
                ),
                "focus" => rules.change(
                    metadata(3),
                    Mutation::FocusLost(op[2].as_bool().unwrap()),
                    &env,
                ),
                "volume" => rules.change(
                    metadata(4),
                    Mutation::Volume(
                        VolumeRuleMode(op[2].as_i64().unwrap() as i32),
                        op[3].as_i64().unwrap() as i32,
                    ),
                    &env,
                ),
                "remove" => rules.remove(string(1), &env),
                "clear" => rules.clear(&env),
                "set" => rules.set_json(string(1)),
                "legacyImport" => rules.import_legacy(string(1), &env),
                "get" => result = json!(rules.get(string(1), &env)),
                "hardGet" => result = json!(rules.is_hard_muted(string(1), &env)),
                "json" => result = json!(rules.json(&env)),
                "clone" => {
                    let mut copy = rules.get(string(1), &env).unwrap();
                    copy.hard_muted = !copy.hard_muted;
                    copy.display_name = Some("tampered".into());
                }
                other => panic!("unknown operation {other}"),
            }
            let mut effects = rules.take_effects();
            let operation_writes = effects
                .iter()
                .filter(|e| matches!(e, Effect::Write(_)))
                .count();
            let sorted = rules.sorted(&env, |a, b| comparisons[&(a, b)]);
            effects.extend(rules.take_effects());
            let writes = effects
                .iter()
                .filter(|e| matches!(e, Effect::Write(_)))
                .count();
            let events = effects
                .iter()
                .filter(|e| matches!(e, Effect::Changed))
                .count();
            assert_eq!(
                operation_writes,
                step["operationWrites"].as_u64().unwrap() as usize,
                "{op:?}"
            );
            assert_eq!(writes, step["writes"].as_u64().unwrap() as usize, "{op:?}");
            assert_eq!(events, step["events"].as_u64().unwrap() as usize, "{op:?}");
            assert_eq!(json!(sorted), step["sorted"], "{op:?}");
            assert_eq!(
                json_or_raw(rules.stored_json()),
                json_or_raw(step["stored"].as_str().unwrap()),
                "{op:?}"
            );
            assert_eq!(
                rules.legacy_json(),
                scenario["legacy"].as_str(),
                "original legacy key must survive {op:?}"
            );
            if string(0) == Some("json") {
                assert_eq!(
                    json_or_raw(result.as_str().unwrap()),
                    json_or_raw(step["result"].as_str().unwrap()),
                    "{op:?}"
                );
            } else {
                assert_eq!(result, step["result"], "{op:?}");
            }
        }
    }
}
