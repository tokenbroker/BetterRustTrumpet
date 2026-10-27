mod support;
use bt_core::{
    app_rules::{Effect, Environment},
    hidden::{AppKey, HiddenApps, HiddenDevices},
    newtonsoft::{self, Token},
};
use serde_json::{Value, json};
use std::collections::HashMap;

fn json_or_raw(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| json!(text))
}

#[test]
fn hidden_lookup_mutation_clone_reload_and_notifications_match_original() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/typed_lists_reference.json")).unwrap();
    let reference = &fixture["hiddenLifecycle"];
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
    let compare = |a: &str, b: &str| comparisons[&(a, b)];
    let Token::Date(now) =
        newtonsoft::parse_dynamic_document(&reference["now"].to_string(), &support::Central)
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
        let mut apps = HiddenApps::new(scenario["initialApps"].as_str().map(str::to_owned));
        let mut devices =
            HiddenDevices::new(scenario["initialDevices"].as_str().map(str::to_owned));
        for step in scenario["steps"].as_array().unwrap() {
            let op = step["op"].as_array().unwrap();
            let string = |n| op.get(n).and_then(Value::as_str);
            let key = AppKey {
                device_id: string(1),
                app_id: string(2),
                exe_name: string(3),
            };
            let mut result = Value::Null;
            match string(0).unwrap() {
                "hideApp" => apps.hide(key, string(4), &env),
                "isApp" => result = json!(apps.is_hidden(key, &env)),
                "unhideApp" => apps.unhide(key, &env),
                "unhideDeviceApps" => apps.unhide_device(string(1), &env),
                "allApps" => apps.clear(&env),
                "countApps" => result = json!(apps.count(&env)),
                "countDeviceApps" => result = json!(apps.count_for_device(string(1), &env)),
                "listDeviceApps" => result = json!(apps.for_device(string(1), &env, compare)),
                "hideDevice" => devices.hide(string(1), string(2), &env),
                "isDevice" => result = json!(devices.is_hidden(string(1), &env)),
                "unhideDevice" => devices.unhide(string(1), &env),
                "allDevices" => devices.clear(&env),
                "countDevices" => result = json!(devices.count(&env)),
                "copyApps" => {
                    let mut copy = apps.sorted(&env, compare);
                    if let Some(first) = copy.first_mut() {
                        first.display_name = Some("tampered".into());
                    }
                    copy.clear();
                }
                "copyDevices" => {
                    let mut copy = devices.sorted(&env, compare);
                    if let Some(first) = copy.first_mut() {
                        first.display_name = Some("tampered".into());
                    }
                    copy.clear();
                }
                "reload" => {
                    apps = HiddenApps::new(Some(apps.stored_json().into()));
                    devices = HiddenDevices::new(Some(devices.stored_json().into()));
                }
                other => panic!("unknown operation {other}"),
            }
            let apps_events = apps.take_effects();
            let devices_events = devices.take_effects();
            let changes = |events: &[Effect]| {
                events
                    .iter()
                    .filter(|e| matches!(e, Effect::Changed))
                    .count()
            };
            let writes = apps_events
                .iter()
                .chain(&devices_events)
                .filter(|e| matches!(e, Effect::Write(_)))
                .count();
            assert_eq!(
                changes(&apps_events),
                step["appsEvents"].as_u64().unwrap() as usize,
                "{op:?}"
            );
            assert_eq!(
                changes(&devices_events),
                step["devicesEvents"].as_u64().unwrap() as usize,
                "{op:?}"
            );
            assert_eq!(writes, step["writes"].as_u64().unwrap() as usize, "{op:?}");
            assert_eq!(result, step["result"], "{op:?}");
            assert_eq!(json!(apps.sorted(&env, compare)), step["apps"], "{op:?}");
            assert_eq!(
                json!(devices.sorted(&env, compare)),
                step["devices"],
                "{op:?}"
            );
            assert_eq!(
                json_or_raw(apps.stored_json()),
                json_or_raw(step["appsRaw"].as_str().unwrap()),
                "{op:?}"
            );
            assert_eq!(
                json_or_raw(devices.stored_json()),
                json_or_raw(step["devicesRaw"].as_str().unwrap()),
                "{op:?}"
            );
            assert!(
                apps.take_effects().is_empty() && devices.take_effects().is_empty(),
                "reads must not rewrite source: {op:?}"
            );
        }
    }
}
