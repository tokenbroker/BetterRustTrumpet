//! Number/volume/save results from real .NET cultures and original C# methods.
use bt_core::{
    cli_responses::{ErrorResponse, SetVolumeResponse, Target},
    cli_rules,
    cli_validation::{self as cli, AppCandidate, CommandCulture, Volume},
    numbers::IntegerFormat,
    text::ordinal_contains,
};
use serde_json::{Value, json};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/cli_validation_reference.json")).unwrap()
}
fn format(value: &Value) -> IntegerFormat<'_> {
    IntegerFormat {
        positive_sign: value["positiveSign"].as_str().unwrap(),
        negative_sign: value["negativeSign"].as_str().unwrap(),
    }
}

struct CapturedCulture<'a> {
    format: IntegerFormat<'a>,
    case: &'a Value,
}
impl CommandCulture for CapturedCulture<'_> {
    fn integer_format(&self) -> IntegerFormat<'_> {
        self.format
    }
    fn starts_with(&self, value: &str, prefix: &str) -> bool {
        assert_eq!(
            Some(value),
            self.case["input"][0].as_str(),
            "culture prefix adapter must receive the original token"
        );
        self.case[if prefix == "+" {
            "startsPlus"
        } else {
            assert_eq!(prefix, "-");
            "startsMinus"
        }]
        .as_bool()
        .unwrap()
    }
}

#[test]
fn dotnet_integer_styles_sign_precedence_whitespace_nuls_and_overflow_match() {
    for group in reference()["formats"].as_array().unwrap() {
        let format = format(group);
        for case in group["numbers"].as_array().unwrap() {
            let actual = case["input"]
                .as_str()
                .and_then(|value| format.parse_i32(value));
            assert_eq!(
                json!(actual.is_some()),
                case["parsed"],
                "{} / {}",
                group["input"],
                case["input"]
            );
            assert_eq!(json!(actual.unwrap_or_default()), case["value"]);
        }
    }
}

#[test]
fn rule_targets_use_current_number_signs_after_invariant_lower_and_percent_trim() {
    for group in reference()["formats"].as_array().unwrap() {
        for case in group["numbers"].as_array().unwrap() {
            let args = vec![
                "--keep".into(),
                format!("music={}", case["input"].as_str().unwrap_or_default()),
            ];
            let actual = match cli_rules::parse_with_format(&args, format(group)) {
                Ok(spec) => json!({"spec":spec}),
                Err(error) => json!({"error":error}),
            };
            assert_eq!(actual, case["rule"], "{} / {args:?}", group["input"]);
        }
    }
}

#[test]
fn set_volume_validation_matching_and_unchecked_addition_match_original_methods() {
    let reference = reference();
    let devices = reference["collection"]["AllDevices"].as_array().unwrap();
    let apps: Vec<&Value> = devices
        .iter()
        .flat_map(|d| d["Apps"].as_array().unwrap())
        .collect();
    let candidates: Vec<_> = apps
        .iter()
        .map(|a| AppCandidate {
            exe_name: a["ExeName"].as_str(),
            display_name: a["DisplayName"].as_str(),
        })
        .collect();
    for group in reference["formats"].as_array().unwrap() {
        for case in group["volumes"].as_array().unwrap() {
            let args: Vec<String> = serde_json::from_value(case["input"].clone()).unwrap();
            let mut device_volumes: Vec<_> = devices
                .iter()
                .map(|d| d["Volume"].as_i64().unwrap() as i32)
                .collect();
            let mut app_volumes: Vec<_> = apps
                .iter()
                .map(|a| a["Volume"].as_i64().unwrap() as i32)
                .collect();
            let result = cli::set_volume(
                &args,
                &CapturedCulture {
                    format: format(group),
                    case,
                },
            )
            .and_then(|request| {
                let (target, current) = if let Some(query) = request.app.as_deref() {
                    let index = cli::find_app(&candidates, query)
                        .ok_or_else(|| format!("app not found: {query}"))?;
                    let volume = request.volume.apply(app_volumes[index]);
                    app_volumes[index] = volume;
                    (
                        Target::App(apps[index]["DisplayName"].as_str().map(str::to_owned)),
                        volume,
                    )
                } else {
                    let index = if let Some(query) = request.device.as_deref() {
                        // Snapshot resolver follows original ID equality/name matching;
                        // null-view/native error paths are accepted separately.
                        devices
                            .iter()
                            .position(|d| d["Id"].as_str() == Some(query))
                            .or_else(|| {
                                devices.iter().position(|d| {
                                    ordinal_contains(d["DisplayName"].as_str().unwrap(), query)
                                })
                            })
                    } else {
                        Some(0)
                    }
                    .ok_or("device not found")?;
                    let volume = request.volume.apply(device_volumes[index]);
                    device_volumes[index] = volume;
                    (
                        Target::Device(devices[index]["DisplayName"].as_str().map(str::to_owned)),
                        volume,
                    )
                };
                Ok(SetVolumeResponse {
                    ok: true,
                    target,
                    volume: current,
                    delta: match request.volume {
                        Volume::Relative(n) => Some(n),
                        Volume::Absolute(_) => None,
                    },
                })
            });
            let actual = match result {
                Ok(value) => json!(value),
                Err(error) => json!(ErrorResponse::from(error)),
            };
            assert_eq!(
                actual, case["result"]["response"],
                "{} / {args:?}",
                group["input"]
            );
            assert_eq!(
                json!(device_volumes),
                case["result"]["deviceVolumes"],
                "{args:?}"
            );
            assert_eq!(json!(app_volumes), case["result"]["appVolumes"], "{args:?}");
        }
    }
}

#[test]
fn save_flag_removal_names_and_usage_errors_match_original_prefix() {
    for case in reference()["saves"].as_array().unwrap() {
        let args = serde_json::from_value::<Vec<String>>(case["input"].clone()).unwrap();
        let actual = match cli::save(&args) {
            Ok(value) => json!(value),
            Err(error) => json!({"error":error}),
        };
        assert_eq!(actual, case["result"], "{args:?}");
    }
}
