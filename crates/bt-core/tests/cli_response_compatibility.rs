//! Original serialization expressions/projection methods supply golden shapes.
//! Native setters, dispatch, filesystem effects and update I/O are not exercised.
use bt_core::cli_responses::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/cli_responses_reference.json")).unwrap()
}
fn roundtrip<T: DeserializeOwned + Serialize>(value: &Value) {
    let parsed: T =
        serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"));
    assert_eq!(serde_json::to_value(parsed).unwrap(), *value);
}
fn projection<T: DeserializeOwned + Serialize>(value: &Value) {
    if value.get("error").is_some() {
        roundtrip::<ErrorResponse>(value);
    } else {
        roundtrip::<T>(value);
    }
}

#[test]
fn every_remaining_success_response_shape_matches_source_serialization() {
    // DescribeDefaults inserts in AllRoles order rather than sorting names.
    let names = DefaultNames {
        output: Some("Speaker".into()),
        output_calls: Some("Calls".into()),
        mic: Some("Mic".into()),
        mic_calls: Some("Call mic".into()),
    };
    assert_eq!(
        serde_json::to_string(&names).unwrap(),
        r#"{"output":"Speaker","outputCalls":"Calls","mic":"Mic","micCalls":"Call mic"}"#
    );
    for case in reference()["shapes"].as_array().unwrap() {
        let result = &case["result"];
        roundtrip::<PingResponse>(&result["Ping"]);
        for key in [
            "DeviceRelative",
            "DeviceAbsolute",
            "AppRelative",
            "AppAbsolute",
        ] {
            roundtrip::<SetVolumeResponse>(&result[key]);
        }
        for key in ["AppMute", "DeviceMute"] {
            roundtrip::<MuteResponse>(&result[key]);
        }
        roundtrip::<SetDefaultResponse>(&result["SetDefault"]);
        roundtrip::<RouteResponse>(&result["Route"]);
        roundtrip::<UpdateResponse>(&result["Update"]);
        roundtrip::<SaveResponse>(&result["Save"]);
        roundtrip::<DeleteResponse>(&result["Delete"]);
        roundtrip::<CreatePresetResponse>(&result["Create"]);
        roundtrip::<SettingsPathResponse>(&result["Path"]);
        roundtrip::<ApplyProfileResponse>(&result["ApplyProfile"]);
        roundtrip::<ErrorResponse>(&result["Error"]);
        assert_eq!(
            json!(ping(case["input"]["version"].as_str())),
            result["Ping"]
        );
        assert_eq!(
            json!(ErrorResponse::from(
                case["input"]["error"].as_str().unwrap()
            )),
            result["Error"]
        );
    }
}

#[test]
fn list_watch_default_volume_profile_and_doctor_shapes_match_original_methods() {
    for case in reference()["projections"].as_array().unwrap() {
        let result = &case["result"];
        projection::<Vec<DeviceResponse>>(&result["devices"]);
        projection::<Vec<AppResponse>>(&result["apps"]);
        projection::<GetVolumeResponse>(&result["volume"]);
        projection::<DefaultResponse>(&result["default"]);
        projection::<WatchResponse>(&result["watch"]);
        projection::<DoctorResponse>(&result["doctor"]);
        projection::<Vec<ProfileResponse>>(&result["profiles"]);
    }
}

#[test]
fn batch_split_quoting_order_json_fallback_and_error_presence_match_source() {
    for case in reference()["batches"].as_array().unwrap() {
        let args: Vec<String> = serde_json::from_value(case["input"]["args"].clone()).unwrap();
        let mut responses: std::collections::VecDeque<String> =
            serde_json::from_value(case["input"]["responses"].clone()).unwrap();
        let mut calls = Vec::new();
        let result = batch(&args, |command| {
            calls.push(command.to_owned());
            responses.pop_front().expect("missing callback fixture")
        });
        let actual = match result {
            Ok(result) => json!(result),
            Err(error) => json!(error),
        };
        assert_eq!(actual, case["result"]["response"], "{args:?}");
        assert_eq!(json!(calls), case["result"]["calls"], "{args:?}");
        assert!(responses.is_empty());
    }
}

#[test]
fn doctor_raw_counts_null_settings_json_and_degraded_state_match_source() {
    for case in reference()["projections"].as_array().unwrap() {
        let input = &case["input"];
        let collection = &input["collection"];
        let counts: Option<Vec<Option<usize>>> =
            collection["AllDevices"].as_array().map(|devices| {
                devices
                    .iter()
                    .map(|device| {
                        if device.is_null() {
                            None
                        } else {
                            Some(device["Apps"].as_array().map_or(0, Vec::len))
                        }
                    })
                    .collect()
            });
        let audio = (!collection.is_null()).then_some(HealthAudio {
            default_device: collection["Default"]["DisplayName"].as_str(),
            device_app_counts: counts.as_deref(),
        });
        let settings = (!input["settings"].is_null()).then_some(HealthSettings {
            profiles_json: input["settings"]["VolumeProfilesJson"].as_str(),
        });
        let result = doctor(
            input["version"].as_str(),
            audio,
            input["managerReady"].as_bool().unwrap(),
            settings,
        );
        assert_eq!(json!(result), case["result"]["doctor"], "{input}");
    }
}
