//! Golden output from original CLI parser/planner/mutator/JSON methods.
use bt_core::{
    cli,
    cli_rules::{self as rules, Collection},
};
use serde::Serialize;
use serde_json::{Value, json};

fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/cli_rules_reference.json")).unwrap()
}
fn response<T: Serialize>(result: Result<T, String>) -> Value {
    match result {
        Ok(value) => serde_json::to_value(value).unwrap(),
        Err(error) => json!({"error":error}),
    }
}

#[test]
fn rule_grammar_errors_assignments_and_flags_match_csharp() {
    for case in reference()["parses"].as_array().unwrap() {
        let args: Option<Vec<String>> = serde_json::from_value(case["input"].clone()).unwrap();
        let result = rules::parse(args.as_deref().unwrap_or_default());
        let actual = match result {
            Ok(spec) => json!({"spec":spec}),
            Err(error) => json!({"error":error}),
        };
        assert_eq!(actual, case["result"], "{}", case["input"]);
    }
}

#[test]
fn command_aliases_use_dotnet_invariant_and_ordinal_case_rules() {
    for case in reference()["normalizations"].as_array().unwrap() {
        let args = serde_json::from_value(case["input"].clone()).unwrap();
        assert_eq!(
            json!(cli::normalize(args)),
            case["result"],
            "{}",
            case["input"]
        );
    }
}

#[test]
fn rule_preview_resolve_apply_json_and_mutated_sessions_match_original() {
    for case in reference()["cases"].as_array().unwrap() {
        let input = &case["input"];
        let expected = &case["result"];
        let mut collection: Option<Collection> =
            serde_json::from_value(input["collection"].clone()).unwrap();
        let args: Vec<String> = serde_json::from_value(input["args"].clone()).unwrap();
        let queries: Vec<String> = serde_json::from_value(input["queries"].clone()).unwrap();
        assert_eq!(
            response(rules::preview_command(collection.as_ref(), &args)),
            expected["preview"],
            "{args:?}"
        );
        assert_eq!(
            response(rules::resolve_command(collection.as_ref(), &queries)),
            expected["resolve"],
            "{args:?}"
        );
        let matches: Vec<_> = queries
            .iter()
            .map(|q| rules::find_matches(collection.as_ref(), Some(q)))
            .collect();
        assert_eq!(json!(matches), expected["matches"], "{args:?}");
        assert_eq!(
            response(rules::apply_command(collection.as_mut(), &args)),
            expected["apply"],
            "{args:?}"
        );
        assert_eq!(json!(collection), expected["after"], "{args:?}");
    }
}
