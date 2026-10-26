use bt_core::colors::{self, Color, NumberCulture};
use serde_json::{Value, json};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/colors_reference.json")).unwrap()
}
const GERMAN: NumberCulture<'static> = NumberCulture {
    decimal_separator: ",",
    group_separator: ".",
    positive_sign: "+",
    negative_sign: "-",
    nan_symbol: "NaN",
    positive_infinity_symbol: "\u{221e}",
    negative_infinity_symbol: "-\u{221e}",
};
fn culture(name: &str) -> NumberCulture<'static> {
    match name {
        "" => NumberCulture::INVARIANT,
        "de-DE" => GERMAN,
        other => panic!("{other}"),
    }
}
fn project(color: Color, culture: &NumberCulture<'_>) -> Value {
    json!({"a": color.a, "r": color.r, "g": color.g, "b": color.b, "text": color.to_text(culture)})
}

#[test]
fn color_converter_hex_names_sc_rgb_and_context_colors_match_wpf() {
    let mut failures = vec![];
    for run in reference()["cultures"].as_array().unwrap() {
        let culture = culture(run["culture"].as_str().unwrap());
        for case in run["colors"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            let mut expected = case["result"].clone();
            if let Some(object) = expected.as_object_mut() {
                object.remove("scA");
            }
            let actual = match colors::parse(input, &culture) {
                Ok(color) => project(color, &culture),
                Err(_) => json!({"errorType": "FormatException"}),
            };
            let matches = match expected.get("errorType") {
                Some(_) => actual.get("errorType").is_some(),
                None => actual == expected,
            };
            if !matches {
                failures.push(format!(
                    "{:?} {input:?}: {actual} != {expected}",
                    run["culture"]
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn sc_rgb_channels_and_culture_float_text_match_wpf() {
    let mut failures = vec![];
    for run in reference()["cultures"].as_array().unwrap() {
        let culture = culture(run["culture"].as_str().unwrap());
        for case in run["scFloats"].as_array().unwrap() {
            let value = f32::from_bits(case["bits"].as_i64().unwrap() as u32);
            let actual = project(Color::from_sc_rgb(value, 0.5, 0.25, 1.0), &culture);
            let expected = json!({"a": case["a"], "r": case["r"], "g": case["g"], "b": case["b"], "text": case["text"]});
            if actual != expected {
                failures.push(format!(
                    "{:?} {}: {actual} != {expected}",
                    run["culture"], case["input"]
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn color_settings_fall_back_to_transparent() {
    let invariant = NumberCulture::INVARIANT;
    assert_eq!(colors::parse_setting(None, &invariant), Color::TRANSPARENT);
    assert_eq!(
        colors::parse_setting(Some(""), &invariant),
        Color::TRANSPARENT
    );
    assert_eq!(
        colors::parse_setting(Some("bogus"), &invariant),
        Color::TRANSPARENT
    );
    assert_eq!(Color::TRANSPARENT.to_text(&invariant), "#00FFFFFF");
    assert_eq!(
        colors::parse_setting(Some("#80112233"), &invariant).to_text(&invariant),
        "#80112233"
    );
}
