//! The pure half of CliHandler's QuickTrumpet commands (ListProfiles,
//! SaveProfile, DeleteProfile, CreatePresetFromRule and DescribeApplyResult):
//! argument validation and the JSON projections. Capture and apply run in
//! the app, which owns audio and the settings store.
use crate::{
    cli_responses::{AppliedDefaultResponse, DefaultNames, ProfileResponse, SaveResponse},
    models::{CaptureScope, DefaultDeviceRole, VolumeProfile},
    profiles,
    text::{is_blank, ordinal_ignore_case},
};

pub const SAVE_FLAGS: [&str; 5] = [
    "--all-devices",
    "--apps-only",
    "--defaults",
    "--no-apps",
    "--no-route",
];
pub const SAVE_USAGE: &str =
    "usage: save NAME [--all-devices] [--apps-only] [--defaults] [--no-apps] [--no-route]";
pub const CREATE_USAGE: &str =
    "usage: preset-create NAME --keep APP=VOL [--others VOL] [--apps-only] [--all-devices]";

/// AllRoles, in DescribeDefaults' order.
pub const ALL_ROLES: [DefaultDeviceRole; 4] = [
    DefaultDeviceRole::PLAYBACK,
    DefaultDeviceRole::PLAYBACK_COMMUNICATIONS,
    DefaultDeviceRole::RECORDING,
    DefaultDeviceRole::RECORDING_COMMUNICATIONS,
];

/// SaveProfile's arguments: the five flags anywhere (ignoring case and
/// repeats), every other token joined into the name (Q29).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveRequest {
    pub name: String,
    pub all_devices: bool,
    pub apps_only: bool,
    pub defaults: bool,
    pub no_apps: bool,
    pub no_route: bool,
}

pub fn parse_save(args: &[String]) -> Result<SaveRequest, String> {
    if args.is_empty() {
        return Err(SAVE_USAGE.into());
    }
    let has = |flag: &str| args.iter().any(|a| ordinal_ignore_case(a, flag));
    let name = args
        .iter()
        .filter(|a| !SAVE_FLAGS.iter().any(|f| ordinal_ignore_case(a, f)))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned();
    if is_blank(Some(&name)) {
        return Err(SAVE_USAGE.into());
    }
    Ok(SaveRequest {
        name,
        all_devices: has("--all-devices"),
        apps_only: has("--apps-only"),
        defaults: has("--defaults"),
        no_apps: has("--no-apps"),
        no_route: has("--no-route"),
    })
}

/// RoleKey.
pub fn role_key(role: DefaultDeviceRole) -> &'static str {
    match role {
        DefaultDeviceRole::PLAYBACK => "output",
        DefaultDeviceRole::PLAYBACK_COMMUNICATIONS => "outputCalls",
        DefaultDeviceRole::RECORDING => "mic",
        _ => "micCalls",
    }
}

/// DescribeDefaults: each set role's display name, or its id.
pub fn describe_defaults(profile: &VolumeProfile) -> DefaultNames {
    let mut names = DefaultNames::default();
    let Some(set) = &profile.default_devices else {
        return names;
    };
    for role in ALL_ROLES {
        let Some(entry) = set.get(role) else {
            continue;
        };
        let name = entry.display_name.clone().or(entry.device_id.clone());
        match role {
            DefaultDeviceRole::PLAYBACK => names.output = name,
            DefaultDeviceRole::PLAYBACK_COMMUNICATIONS => names.output_calls = name,
            DefaultDeviceRole::RECORDING => names.mic = name,
            _ => names.mic_calls = name,
        }
    }
    names
}

/// ListProfiles' entries; `summary` is QuickTrumpetSummary.DescribeProfile.
pub fn list(
    presets: &[VolumeProfile],
    last_applied: Option<&str>,
    summary: impl Fn(&VolumeProfile) -> String,
) -> Vec<ProfileResponse> {
    presets
        .iter()
        .map(|p| ProfileResponse {
            name: p.name.clone(),
            slug: profiles::get_slug(p),
            devices: p.volume_devices().count(),
            apps: p.all_apps().count(),
            created_at: p.created_at.clone(),
            summary: summary(p),
            include_device_volumes: p.include_device_volumes,
            include_app_volumes: p.include_app_volumes,
            route_apps: p.route_apps,
            defaults: describe_defaults(p),
            last_applied: p
                .id
                .as_deref()
                .filter(|id| !id.is_empty())
                .zip(last_applied)
                .is_some_and(|(id, last)| ordinal_ignore_case(id, last)),
        })
        .collect()
}

/// SaveProfile's answer for the saved preset.
pub fn saved(profile: &VolumeProfile, summary: String) -> SaveResponse {
    SaveResponse {
        ok: true,
        preset: profile.name.clone(),
        slug: profile.slug.clone(),
        capture_scope: scope_name(profile.capture_scope),
        apply_apps_only: profile.apply_apps_only(),
        include_app_volumes: profile.include_app_volumes,
        route_apps: profile.route_apps,
        defaults: describe_defaults(profile),
        devices: profile.volume_devices().count(),
        apps: profile.all_apps().count(),
        summary,
    }
}

/// `CaptureScope.ToString()`: the name, or the number when undefined.
pub fn scope_name(scope: CaptureScope) -> String {
    match scope {
        CaptureScope::CURRENT_DEVICE => "CurrentDevice".into(),
        CaptureScope::ALL_DEVICES => "AllDevices".into(),
        other => other.0.to_string(),
    }
}

/// One DefaultsApplied entry of DescribeApplyResult.
pub fn applied_default(
    role: DefaultDeviceRole,
    name: Option<String>,
    id: Option<String>,
) -> AppliedDefaultResponse {
    AppliedDefaultResponse {
        role: role_key(role).into(),
        name,
        id,
    }
}

/// CreatePresetFromRule's split: the first token is the name, the rest
/// the rule.
pub fn split_create(args: &[String]) -> Result<(&str, &[String]), String> {
    match args.split_first() {
        Some((name, rule)) => Ok((name, rule)),
        None => Err(CREATE_USAGE.into()),
    }
}

/// `$"Profile {DateTime.Now:yyyy-MM-dd HH:mm}"` for blank capture names.
pub fn default_name(now: &str) -> String {
    format!("Profile {now}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split(' ').map(String::from).collect()
    }

    #[test]
    fn save_strips_known_flags_and_joins_the_rest() {
        let r = parse_save(&args("Night --NO-APPS Mode --defaults --defaults --x")).unwrap();
        assert_eq!(r.name, "Night Mode --x");
        assert!(r.no_apps && r.defaults && !r.all_devices && !r.apps_only && !r.no_route);
        assert_eq!(parse_save(&[]), Err(SAVE_USAGE.into()));
        assert_eq!(parse_save(&args("--apps-only")), Err(SAVE_USAGE.into()));
    }

    #[test]
    fn role_keys() {
        let keys: Vec<_> = ALL_ROLES.iter().map(|r| role_key(*r)).collect();
        assert_eq!(keys, ["output", "outputCalls", "mic", "micCalls"]);
    }
}
