//! Portable CLI validation. Native resource lookup and culture collation are
//! explicit boundaries, with original-source fixtures for their contracts.
use crate::{
    cli::get_arg,
    identity::filename_without_extension,
    numbers::IntegerFormat,
    text::{is_blank, ordinal_contains, ordinal_ignore_case, ordinal_ignore_case_optional},
};
use serde::Serialize;

pub trait CommandCulture {
    fn integer_format(&self) -> IntegerFormat<'_>;
    /// C# string.StartsWith(string) uses CurrentCulture, not ordinal comparison.
    fn starts_with(&self, value: &str, prefix: &str) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Volume {
    Absolute(i32),
    Relative(i32),
}
impl Volume {
    pub fn apply(self, current: i32) -> i32 {
        match self {
            Self::Absolute(value) => value,
            // The original C# project uses unchecked Int32 addition, then clamps.
            Self::Relative(delta) => current.wrapping_add(delta).clamp(0, 100),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetVolume {
    pub volume: Volume,
    pub app: Option<String>,
    pub device: Option<String>,
}
pub fn set_volume(args: &[String], culture: &impl CommandCulture) -> Result<SetVolume, String> {
    let value = args
        .first()
        .ok_or("usage: set-volume VALUE [--device ID] [--app EXENAME]")?;
    let relative = culture.starts_with(value, "+")
        || (culture.starts_with(value, "-") && value.encode_utf16().count() > 1);
    let number = culture.integer_format().parse_i32(value);
    let volume = if relative {
        Volume::Relative(number.ok_or("invalid relative volume value")?)
    } else {
        Volume::Absolute(
            number
                .filter(|n| (0..=100).contains(n))
                .ok_or("volume must be 0-100 (or +N/-N for relative)")?,
        )
    };
    Ok(SetVolume {
        volume,
        app: get_arg(args, "--app").map(str::to_owned),
        device: get_arg(args, "--device").map(str::to_owned),
    })
}

pub const SAVE_USAGE: &str =
    "usage: save NAME [--all-devices] [--apps-only] [--defaults] [--no-apps] [--no-route]";
const SAVE_FLAGS: [&str; 5] = [
    "--all-devices",
    "--apps-only",
    "--defaults",
    "--no-apps",
    "--no-route",
];
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Save {
    pub profile_name: String,
    pub capture_all_devices: bool,
    pub apply_apps_only: bool,
    pub no_apps: bool,
    pub no_route: bool,
    pub defaults: bool,
}
/// Capture uses the legacy options first (including apps). --no-apps disables
/// application only afterwards; it does not remove saved app entries/counts.
pub fn save(args: &[String]) -> Result<Save, String> {
    if args.is_empty() {
        return Err(SAVE_USAGE.into());
    }
    let has = |flag: &str| args.iter().any(|arg| ordinal_ignore_case(arg, flag));
    let profile_name = args
        .iter()
        .filter(|arg| !SAVE_FLAGS.iter().any(|flag| ordinal_ignore_case(arg, flag)))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned();
    if is_blank(Some(&profile_name)) {
        return Err(SAVE_USAGE.into());
    }
    Ok(Save {
        profile_name,
        capture_all_devices: has("--all-devices"),
        apply_apps_only: has("--apps-only"),
        no_apps: has("--no-apps"),
        no_route: has("--no-route"),
        defaults: has("--defaults"),
    })
}

pub struct AppCandidate<'a> {
    pub exe_name: Option<&'a str>,
    pub display_name: Option<&'a str>,
}
/// CLI FindApp differs from AppIdentity: it never strips the query extension,
/// path or quotes. Exact matches across the whole sequence precede partials.
pub fn find_app(apps: &[AppCandidate<'_>], query: &str) -> Option<usize> {
    if is_blank(Some(query)) {
        return None;
    }
    apps.iter()
        .position(|app| {
            let exe = app.exe_name.unwrap_or_default();
            ordinal_ignore_case(exe, query)
                || ordinal_ignore_case(&filename_without_extension(exe), query)
                || ordinal_ignore_case_optional(app.display_name, Some(query))
        })
        .or_else(|| {
            apps.iter().position(|app| {
                ordinal_contains(app.exe_name.unwrap_or_default(), query)
                    || ordinal_contains(app.display_name.unwrap_or_default(), query)
            })
        })
}
