//! Legacy command grammar from CLI/CliHandler.cs, not a shell grammar.
use crate::text::{invariant_lower, ordinal_ignore_case};

fn alias_keyword(value: Option<&String>) -> Option<&'static str> {
    let value = value?;
    [
        "preview",
        "apply",
        "list",
        "default",
        "set-default",
        "default-to",
        "check",
        "export",
        "import",
        "create",
        "load",
        "save",
        "delete",
        "next",
        "prev",
        "previous",
    ]
    .into_iter()
    .find(|keyword| ordinal_ignore_case(value, keyword))
}

/// Tokenize exactly on literal spaces outside double quotes. Quotes toggle
/// grouping and are removed; empty tokens disappear, tabs and backslashes stay.
/// An unmatched quote is accepted by the original handler.
pub fn tokenize(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

pub fn trim_command_prefix(value: &str) -> &str {
    value.trim_start_matches(['-', '/'])
}

fn with_command(name: &str, args: &[String]) -> Vec<String> {
    std::iter::once(name.to_owned())
        .chain(args.iter().cloned())
        .collect()
}

/// Normalize aliases while preserving unknown command spelling and arguments.
/// Unknown commands are later interpreted by the app as preset aliases.
pub fn normalize(mut parts: Vec<String>) -> Vec<String> {
    let Some(first) = parts.first() else {
        return parts;
    };
    let first = invariant_lower(trim_command_prefix(first));
    let rest = &parts[1..];
    let sub = alias_keyword(rest.first());
    match first.as_str() {
        "mode" => return with_command("apply", rest),
        "volume" => return normalize_volume(rest),
        "mute" | "unmute" | "toggle-mute" => {
            if rest.len() == 1 && !rest[0].starts_with(['-', '/']) {
                return vec![first, "--app".into(), rest[0].clone()];
            }
            return with_command(&first, rest);
        }
        "profile" | "preset" => return normalize_profile(rest),
        "rule" => {
            return match sub {
                Some("preview") => with_command("rule-preview", &rest[1..]),
                Some("apply") => with_command("rule-apply", &rest[1..]),
                _ => with_command("rule-preview", rest),
            };
        }
        "device" => {
            return match sub {
                _ if rest.is_empty() => vec!["list-devices".into()],
                Some("list") => vec!["list-devices".into()],
                Some("default") => vec!["get-default".into()],
                Some("set-default" | "default-to") => with_command("set-default", &rest[1..]),
                _ => with_command("set-default", rest),
            };
        }
        "apps" if rest.is_empty() || sub == Some("list") => return vec!["list-apps".into()],
        "update" if sub == Some("check") => return vec!["check-update".into()],
        "settings" if sub == Some("export") => return with_command("export-settings", &rest[1..]),
        "settings" if sub == Some("import") => return with_command("import-settings", &rest[1..]),
        _ => {}
    }
    parts[0] = trim_command_prefix(&parts[0]).to_owned();
    parts
}

fn normalize_volume(mut args: &[String]) -> Vec<String> {
    if args.first().is_some_and(|v| ordinal_ignore_case(v, "get")) {
        return with_command("get-volume", &args[1..]);
    }
    if args.first().is_some_and(|v| ordinal_ignore_case(v, "set")) {
        args = &args[1..];
    }
    match args {
        [] => vec!["get-volume".into()],
        [value] => vec!["set-volume".into(), value.clone()],
        [app, value, tail @ ..] => {
            let mut result = vec![
                "set-volume".into(),
                value.clone(),
                "--app".into(),
                app.clone(),
            ];
            result.extend_from_slice(tail);
            result
        }
    }
}

fn normalize_profile(args: &[String]) -> Vec<String> {
    let sub = alias_keyword(args.first());
    match sub {
        _ if args.is_empty() => vec!["presets".into()],
        Some("list") => vec!["presets".into()],
        Some("create") => with_command("preset-create", &args[1..]),
        Some("load" | "apply") => with_command("apply", &args[1..]),
        Some("save") => with_command("save", &args[1..]),
        Some("delete") => with_command("delete", &args[1..]),
        Some("next") if args.len() == 1 => vec!["preset-next".into()],
        Some("prev" | "previous") if args.len() == 1 => vec!["preset-prev".into()],
        _ => with_command("apply", args),
    }
}

/// Mirrors IsBatchCommandStart, including its omissions (e.g. ping/export).
pub fn is_batch_command_start(value: &str) -> bool {
    matches!(
        invariant_lower(trim_command_prefix(value)).as_str(),
        "list-devices"
            | "list-apps"
            | "resolve-apps"
            | "get-volume"
            | "set-volume"
            | "mute"
            | "unmute"
            | "toggle-mute"
            | "get-default"
            | "set-default"
            | "set-device"
            | "presets"
            | "apply"
            | "apply-profile"
            | "save"
            | "preset-next"
            | "preset-prev"
            | "preset-previous"
            | "delete"
            | "rule-preview"
            | "rule-apply"
            | "preset-create"
            | "watch"
            | "check-update"
            | "doctor"
            | "volume"
            | "mode"
    )
}

pub fn split_batch(args: &[String]) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut current = Vec::new();
    for arg in args {
        if is_batch_command_start(arg) {
            if !current.is_empty() {
                commands.push(std::mem::take(&mut current));
            }
            current.push(trim_command_prefix(arg).to_owned());
        } else {
            current.push(arg.clone());
        }
    }
    if !current.is_empty() {
        commands.push(current);
    }
    commands
}

/// Quoting used when sending a token list through the old line protocol.
/// Embedded quotes are deliberately not escaped by the legacy implementation.
pub fn quote_if_needed(value: &str) -> String {
    if value.is_empty() || value.contains(' ') {
        format!("\"{value}\"")
    } else {
        value.to_owned()
    }
}

pub fn get_arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|v| ordinal_ignore_case(v, flag))
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

pub fn is_flag(value: &str, flag: &str) -> bool {
    ordinal_ignore_case(value, flag) || ordinal_ignore_case(value, flag.trim_start_matches('-'))
}
