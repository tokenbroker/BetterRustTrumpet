mod support;
use support::Central;
// Golden expectations execute verbatim extracted C# profile methods.
use bt_core::{
    models::*,
    profiles::{self, ProfileCollection},
    text,
};
use serde_json::Value;

const NEW_ID: &str = "00000000000000000000000000000001";
fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/models_reference.json")).unwrap()
}

#[test]
fn unicode_slug_and_case_fixtures_match_dotnet() {
    for case in reference()["slugs"].as_array().unwrap() {
        assert_eq!(
            text::profile_slug(case["input"].as_str()),
            case["value"].as_str().unwrap(),
            "{}",
            case["input"]
        );
        if let Some(input) = case["input"].as_str() {
            assert_eq!(
                text::invariant_lower(input),
                case["lower"].as_str().unwrap()
            );
        }
    }
    for case in reference()["ordinalCases"].as_array().unwrap() {
        assert_eq!(
            text::ordinal_ignore_case_optional(case["left"].as_str(), case["right"].as_str()),
            case["equal"].as_bool().unwrap(),
            "{case}"
        );
    }
    // A compact C# oracle checks classification/casing over every valid BMP
    // character. FNV here is only a regression fingerprint, not cryptography.
    let bmp: String = (0..=0xffff).filter_map(char::from_u32).collect();
    let slug = text::profile_slug(Some(&bmp));
    let hash = slug
        .bytes()
        .fold(14_695_981_039_346_656_037u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
        });
    assert_eq!(
        format!("{hash:016x}"),
        reference()["slugAllBmpFingerprint"].as_str().unwrap()
    );
}

#[test]
fn old_profile_normalization_matches_csharp_and_allocates_id_once() {
    for case in reference()["profileMigrations"].as_array().unwrap() {
        let mut profile: VolumeProfile =
            serde_json::from_str(case["input"].as_str().unwrap()).unwrap();
        let mut calls = 0;
        let assigned = profiles::normalize(&mut profile, &mut || {
            calls += 1;
            NEW_ID.into()
        });
        assert_eq!(
            assigned,
            case["result"]["value"]["assignedId"].as_bool().unwrap()
        );
        assert_eq!(calls, usize::from(assigned));
        assert_eq!(
            serde_json::to_value(&profile).unwrap(),
            case["result"]["value"]["profile"]
        );
        assert!(!profiles::normalize(&mut profile, &mut || panic!(
            "normalized ID must remain stable"
        )));
    }
}

#[test]
fn profile_list_nil_empty_null_filtering_and_error_facade_match_source() {
    for case in reference()["profileParses"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let mut allocate = || panic!("these fixtures already have IDs");
        let parsed = profiles::parse(input, &Central, &mut allocate);
        if case["result"].get("errorType").is_some() {
            assert!(parsed.is_err(), "{input}");
        } else {
            let parsed = parsed.unwrap();
            assert_eq!(
                parsed.assigned_ids,
                case["result"]["value"]["assignedIds"].as_bool().unwrap()
            );
            assert_eq!(
                serde_json::to_value(parsed.profiles).unwrap(),
                case["result"]["value"]["profiles"]
            );
        }
        assert_eq!(
            serde_json::to_value(profiles::parse_legacy(input, &Central, &mut allocate)).unwrap(),
            case["legacy"]
        );
    }
}

#[test]
fn collection_lookup_cycles_save_rename_and_import_match_csharp() {
    let reference = reference();
    let scenario = &reference["profileCollection"];
    let mut collection =
        ProfileCollection::new(serde_json::from_value(scenario["initial"].clone()).unwrap());
    for lookup in scenario["lookups"].as_array().unwrap() {
        let index = collection
            .find(lookup["query"].as_str().unwrap())
            .map(|i| i as i64)
            .unwrap_or(-1);
        assert_eq!(index, lookup["index"].as_i64().unwrap());
    }
    for cycle in scenario["cycles"].as_array().unwrap() {
        let index = collection
            .cycle_target(
                cycle["last"].as_str().unwrap(),
                cycle["direction"].as_i64().unwrap() as i32,
            )
            .unwrap();
        assert_eq!(index, cycle["index"].as_u64().unwrap() as usize);
    }
    let replacement = VolumeProfile {
        id: Some("replace".into()),
        name: Some("Night".into()),
        created_at: Some("replacement".into()),
        ..Default::default()
    };
    collection.save_replacement(
        replacement,
        scenario["saved"][0]["UpdatedAt"].as_str().unwrap(),
    );
    assert_eq!(
        serde_json::to_value(collection.profiles()).unwrap(),
        scenario["saved"]
    );
    collection.rename(0, Some("Renamed".into()));
    assert_eq!(
        serde_json::to_value(collection.profiles()).unwrap(),
        scenario["renamed"]
    );
    let mut calls = 0;
    let index = collection
        .import_json(
            r#"{"Id":"A","Name":"Renamed","Slug":"night"}"#,
            &Central,
            &mut || {
                calls += 1;
                NEW_ID.into()
            },
        )
        .unwrap();
    assert_eq!(index, Some(3));
    assert_eq!(calls, 1);
    assert_eq!(
        serde_json::to_value(collection.profiles()).unwrap(),
        scenario["imported"]
    );
}

#[test]
fn profile_device_app_edits_and_empty_carrier_pruning_match_csharp() {
    for case in reference()["profileEdits"].as_array().unwrap() {
        let mut profile: VolumeProfile =
            serde_json::from_str(case["profile"].as_str().unwrap()).unwrap();
        for step in case["steps"].as_array().unwrap() {
            let op = &step["operation"];
            let key = op["key"].as_str().unwrap();
            let volume = op["volume"].as_i64().map(|n| n as i32);
            let muted = op["muted"].as_bool();
            let now = step["profile"]["UpdatedAt"].as_str().unwrap_or("unused");
            let result = match op["method"].as_str().unwrap() {
                "setDevice" => profile.set_device_entry(key, volume, muted, now),
                "removeDevice" => profile.remove_device_entry(key, now),
                "setApp" => profile.set_app_entry(key, volume, muted, now),
                "removeApp" => profile.remove_app_entry(key, now),
                other => panic!("unknown fixture operation: {other}"),
            };
            assert_eq!(result, step["result"].as_bool().unwrap());
            assert_eq!(
                serde_json::to_value(&profile).unwrap(),
                step["profile"],
                "{op}"
            );
        }
    }
}

#[test]
fn load_persistence_signal_only_tracks_new_ids_and_roundtrips_stably() {
    let parsed = profiles::parse(
        r#"[null,{"Id":"keep","Hotkey":null},{"Name":"New"}]"#,
        &Central,
        &mut || NEW_ID.into(),
    )
    .unwrap();
    assert!(parsed.assigned_ids);
    let collection = ProfileCollection::new(parsed.profiles);
    let json = collection.to_json().unwrap();
    let second =
        profiles::parse(&json, &Central, &mut || panic!("IDs must survive reload")).unwrap();
    assert!(!second.assigned_ids);
    assert_eq!(second.profiles, collection.profiles());
    let normalized = profiles::parse(
        r#"[{"Id":"keep","Hotkey":null}]"#,
        &Central,
        &mut || panic!(),
    )
    .unwrap();
    assert!(!normalized.assigned_ids);
    assert_eq!(normalized.profiles[0].hotkey, Some(HotkeyData::default()));
}
