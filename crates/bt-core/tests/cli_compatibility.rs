//! Source-derived fixtures: CLI/CliHandler.cs ParseCommandLine,
//! NormalizeCommand and SplitBatchCommands at upstream 1dd17269.
//! These are static expectation captures, not results from executing C#.
use bt_core::cli;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    input: String,
    tokens: Vec<String>,
    normalized: Vec<String>,
}

#[test]
fn source_derived_tokenization_and_alias_fixtures() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/cli_cases.json")).unwrap();
    for case in cases {
        let tokens = cli::tokenize(&case.input);
        assert_eq!(tokens, case.tokens, "tokenize {:?}", case.input);
        assert_eq!(
            cli::normalize(tokens),
            case.normalized,
            "normalize {:?}",
            case.input
        );
    }
}

#[test]
fn batch_boundaries_retain_legacy_command_omissions() {
    let args = cli::tokenize("--set-volume -5 --app discord ping --set-volume 30 --app music");
    assert_eq!(
        cli::split_batch(&args),
        vec![
            vec!["set-volume", "-5", "--app", "discord", "ping"],
            vec!["set-volume", "30", "--app", "music"],
        ]
    );
    for value in [
        "ping",
        "export-settings",
        "import-settings",
        "list-profiles",
        "batch",
        "preset",
    ] {
        assert!(!cli::is_batch_command_start(value), "{value}");
    }
    assert!(cli::is_batch_command_start("///--DoCtOr"));
}

#[test]
fn command_argument_quoting_and_first_flag_lookup_match_source() {
    assert_eq!(cli::quote_if_needed(""), "\"\"");
    assert_eq!(cli::quote_if_needed("Night Mode"), "\"Night Mode\"");
    assert_eq!(cli::quote_if_needed("a\"b"), "a\"b");
    let args = cli::tokenize("--APP First --app Second --device");
    assert_eq!(cli::get_arg(&args, "--app"), Some("First"));
    assert_eq!(cli::get_arg(&args, "--device"), None);
    assert!(cli::is_flag("APPS-ONLY", "--apps-only"));
    assert!(!cli::is_flag("/apps-only", "--apps-only"));
}
