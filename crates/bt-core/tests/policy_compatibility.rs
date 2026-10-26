//! Executes Rust against fixtures captured by linked, unmodified C# policies.
use bt_core::{
    focus,
    folders::{self, PathError, WindowsPathResolver},
    identity, window_policy,
};
use serde_json::{Value, json};

fn policies() -> Value {
    serde_json::from_str::<Value>(include_str!("fixtures/models_reference.json")).unwrap()["policies"].clone()
}
fn number(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| value.as_str().unwrap().parse().unwrap())
}

struct ReferencePaths(Value);
impl ReferencePaths {
    fn result(&self, input: &str, key: &str) -> Result<&Value, PathError> {
        let case = self
            .0
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["input"].as_str() == Some(input))
            .unwrap_or_else(|| panic!("uncaptured Windows path: {input:?}"));
        let result = &case[key];
        if let Some(value) = result.get("value") {
            Ok(value)
        } else {
            Err(match result["errorType"].as_str().unwrap() {
                "ArgumentException" | "ArgumentNullException" => PathError::InvalidArgument,
                "NotSupportedException" => PathError::NotSupported,
                other => PathError::Other(other.into()),
            })
        }
    }
}
impl WindowsPathResolver for ReferencePaths {
    fn full_path(&self, input: &str) -> Result<String, PathError> {
        Ok(self.result(input, "full")?.as_str().unwrap().into())
    }
}

#[test]
fn windows_filename_normalization_and_app_matching_match_csharp() {
    let reference = policies();
    for case in reference["identity"].as_array().unwrap() {
        let input = case["input"].as_str();
        assert_eq!(
            identity::normalize_exe_name(input),
            case["exe"]["value"].as_str().unwrap(),
            "{case}"
        );
        assert_eq!(
            identity::normalize_token(input),
            case["token"]["value"].as_str().unwrap(),
            "{case}"
        );
    }
    for case in reference["matches"].as_array().unwrap() {
        let c = &case["input"];
        let (exe, display, query) = (
            c["exe"].as_str(),
            c["display"].as_str(),
            c["query"].as_str(),
        );
        let actual = json!({
            "exact":identity::matches_exact(exe,display,query), "partial":identity::matches_partial(exe,display,query),
            "app":identity::matches_app(exe,display,query), "device":identity::matches_device(display,query),
            "score":identity::score(exe,display,c["appId"].as_str(),query)
        });
        assert_eq!(actual, case["result"]["value"], "{c}");
    }
}

#[test]
fn folder_matching_and_relative_path_delegation_match_csharp() {
    let reference = policies();
    let resolver = ReferencePaths(reference["pathResolution"].clone());
    for case in reference["pathResolution"].as_array().unwrap() {
        assert_eq!(
            identity::windows_is_fully_qualified(case["input"].as_str().unwrap()),
            case["qualified"]["value"].as_bool().unwrap()
        );
    }
    for case in reference["folders"].as_array().unwrap() {
        let c = &case["input"];
        let (exe, folder) = (c["exe"].as_str(), c["folder"].as_str());
        let actual = (|| -> Result<Value, PathError> {
            Ok(json!({
                "folder":folders::normalize_folder(folder,&resolver)?,
                "executable":folders::normalize_executable(exe,&resolver)?,
                "usable":folders::is_usable_executable(exe),
                "under":folders::is_executable_under_folder(exe,folder,&resolver)?
            }))
        })();
        if case["result"].get("errorType").is_some() {
            assert!(actual.is_err(), "{c}");
        } else {
            assert_eq!(actual.unwrap(), case["result"]["value"], "{c}");
        }
    }
    for case in reference["folderRules"].as_array().unwrap() {
        let c = &case["input"];
        let rules: Vec<_> = c["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| folders::Rule {
                folder_path: r["path"].as_str().map(str::to_owned),
                volume_percent: r["volume"].as_i64().unwrap() as i32,
                created_at_ticks: r["ticks"].as_i64().unwrap(),
            })
            .collect();
        let matched = folders::try_match(c["exe"].as_str(), &rules, &resolver).unwrap();
        assert_eq!(
            json!({"matched":matched.is_some(),"volume":matched.unwrap_or(0)}),
            case["result"]["value"]
        );
    }
}

#[test]
fn focus_supervisor_snapshots_restore_scope_and_pruning_match_csharp() {
    for scenario in policies()["focus"].as_array().unwrap() {
        let mut supervisor = focus::Supervisor::default();
        for step in scenario.as_array().unwrap() {
            let input = &step["input"];
            let sessions: Vec<focus::Session> = input
                .get("sessions")
                .map(|s| serde_json::from_value(s.clone()).unwrap())
                .unwrap_or_default();
            let changes = supervisor.on_foreground_changed(
                input["pid"].as_i64().unwrap() as i32,
                &sessions,
                focus::Mode(input["mode"].as_i64().unwrap() as i32),
                input["percent"].as_i64().unwrap() as i32,
                input["ignoredPid"].as_i64().unwrap_or(0) as i32,
                input["exe"].as_str(),
            );
            assert_eq!(
                serde_json::to_value(changes).unwrap(),
                step["adjustments"],
                "{input}"
            );
            assert_eq!(
                supervisor.has_saved_state(),
                step["saved"].as_bool().unwrap()
            );
        }
    }
}

#[test]
fn fade_rounding_window_sizes_and_notification_gates_match_csharp() {
    let reference = policies();
    for case in reference["fade"].as_array().unwrap() {
        let c = &case["input"];
        assert_eq!(
            focus::interpolate_volume(
                c[0].as_i64().unwrap() as i32,
                c[1].as_i64().unwrap() as i32,
                number(&c[2])
            ),
            case["volume"].as_i64().unwrap() as i32,
            "{c}"
        );
    }
    for case in reference["windowSizes"].as_array().unwrap() {
        let c = &case["input"];
        let size = window_policy::normalize_size(number(&c[0]), number(&c[1]));
        let (width, height) = size.unwrap_or((0.0, 0.0));
        assert_eq!(
            json!({"valid":size.is_some(),"width":width,"height":height}),
            case["result"]["value"]
        );
    }
    for case in reference["notifications"].as_array().unwrap() {
        let c = &case["input"];
        assert_eq!(
            window_policy::should_notify_default_change(
                c[0].as_str(),
                c[1].as_str(),
                c[2].as_bool().unwrap()
            ),
            case["value"].as_bool().unwrap()
        );
    }
    assert!(!window_policy::should_restore_user_size(3));
    assert!(window_policy::should_restore_user_size(4));
}

#[test]
fn attenuation_remains_absolute_and_clamps_only_when_attenuating() {
    let snapshot = focus::Snapshot {
        volume: 150,
        is_muted: true,
    };
    assert_eq!(focus::resolve_mode(true, 0), focus::Mode::MUTE);
    assert_eq!(focus::resolve_mode(false, 80), focus::Mode::OFF);
    assert_eq!(
        focus::apply_background(snapshot, focus::Mode::ATTENUATE, 25).volume,
        25
    );
    assert_eq!(
        focus::apply_background(snapshot, focus::Mode::MUTE, 25).volume,
        150
    );
    assert_eq!(
        focus::apply_background(snapshot, focus::Mode(99), 25),
        snapshot
    );
    assert_eq!(focus::clamp_attenuate_percent(-1), 0);
    assert_eq!(focus::clamp_fade_duration_ms(6000), 5000);
}
