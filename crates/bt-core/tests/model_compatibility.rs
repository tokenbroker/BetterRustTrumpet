//! Actual .NET 8/Newtonsoft fixtures produced by capture_model_fixtures.py,
//! using linked original Hotkey/Theme source and extracted profile/rule DTOs.
mod support;
use bt_core::{
    models::*,
    themes::{BUILTIN_THEMES, CATEGORIES, Theme},
};
use serde_json::Value;

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/models_reference.json")).unwrap()
}

#[test]
fn profile_defaults_nulls_enums_and_property_order_match_actual_csharp() {
    for case in reference()["profiles"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let profile: VolumeProfile = serde_json::from_str(input).unwrap();
        assert_eq!(
            serde_json::to_value(profile).unwrap(),
            case["result"]["value"],
            "{input}"
        );
    }
}

#[test]
fn app_rule_serialization_omits_derived_behavior_properties() {
    for case in reference()["appRules"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let rule: AppRuleEntry = serde_json::from_str(input).unwrap();
        assert_eq!(
            serde_json::to_value(rule).unwrap(),
            case["result"]["value"],
            "{input}"
        );
    }
    let neutral = AppRuleEntry {
        is_pinned: true,
        ..Default::default()
    };
    assert!(!neutral.has_active_behavior());
    let unknown = AppRuleEntry {
        volume_mode: VolumeRuleMode(45),
        ..Default::default()
    };
    assert!(unknown.has_volume_rule());
}

#[test]
fn hotkey_computed_property_and_invalid_default_slot_match_source() {
    assert_eq!(
        serde_json::to_value(HotkeyData::default()).unwrap(),
        reference()["hotkey"]
    );
    let hotkey: HotkeyData =
        serde_json::from_str(r#"{"Modifiers":0,"Key":0,"IsEmpty":false}"#).unwrap();
    assert!(hotkey.is_empty());
    let slots: DefaultDeviceSet =
        serde_json::from_str(r#"{"Playback":{"DeviceId":"  "}}"#).unwrap();
    assert!(slots.is_empty());
    assert!(slots.get(DefaultDeviceRole(99)).is_none());
}

#[test]
fn all_twelve_palettes_match_actual_csharp_color_exports() {
    // Alucard and Blood Moon are ours (D60); the other twelve are the original's.
    let ours = ["Alucard", "Blood Moon"];
    let actual: Vec<Value> = BUILTIN_THEMES
        .into_iter()
        .filter(|theme| !ours.contains(&theme.name))
        .map(|theme| serde_json::from_str(&theme.to_theme().to_json().unwrap()).unwrap())
        .collect();
    assert_eq!(actual, *reference()["themes"].as_array().unwrap());
    assert_eq!(
        CATEGORIES,
        ["Minimal", "Apps", "Editors", "Atmosphere", "Accessibility"]
    );
    assert!(!BUILTIN_THEMES[0].to_theme().is_custom);
    assert_eq!(BUILTIN_THEMES[0].opacity, None);
    for name in ours {
        let theme = BUILTIN_THEMES.iter().find(|t| t.name == name).unwrap();
        assert!(CATEGORIES.contains(&theme.category));
    }
}

#[test]
fn theme_import_legacy_defaults_clamps_and_failures_match_csharp() {
    for case in reference()["themeImports"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let result = Theme::from_json(input, &support::Central);
        if case["result"].get("errorType").is_some() {
            assert!(result.is_err(), "C# rejected: {input}");
        } else {
            let theme = result.unwrap();
            assert!(theme.is_custom);
            let actual: Value = serde_json::from_str(&theme.to_json().unwrap()).unwrap();
            assert_eq!(actual, case["result"]["value"], "{input}");
        }
    }
}
