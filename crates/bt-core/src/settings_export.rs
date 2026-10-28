//! SettingsExportService: the `.btsettings` file. Export is the original's
//! whitelist (Q05) written as Newtonsoft `Formatting.Indented`; import reads
//! a `Dictionary<string, object>` and converts each field on its own, so a
//! bad field is skipped and the import still succeeds (Q13). RunAtStartup is
//! exported but never imported; metadata and unknown keys are ignored.
//! Hotkeys are only `{Modifiers, Key}` here (Q18). Pinned by
//! settings_export_reference.json (the original service on .NET 8).
//!
//! Culture-dependent conversions (number and date text) use the invariant
//! culture, like the rest of the port's settings code.
use crate::{
    colors::{self, NumberCulture},
    models::HotkeyData,
    newtonsoft::{self, Date, LocalZone, Token},
    numbers::IntegerFormat,
    settings::{Key, Value},
};
use thiserror::Error;

pub const FORMAT_VERSION: i32 = 1;
/// The Save/Open dialogs' filter, title and default extension.
pub const FILE_FILTER: &str = "BetterTrumpet Settings (*.btsettings)|*.btsettings";
pub const EXPORT_TITLE: &str = "Export BetterTrumpet Settings";
pub const IMPORT_TITLE: &str = "Import BetterTrumpet Settings";
pub const EXTENSION: &str = ".btsettings";

/// `BetterTrumpet_Settings_{DateTime.Now:yyyy-MM-dd}`: the Save dialog's
/// file name (its default extension adds `.btsettings`).
pub fn default_file_name(year: i32, month: u32, day: u32) -> String {
    format!("BetterTrumpet_Settings_{year:04}-{month:02}-{day:02}")
}

/// The CLI's path when `export-settings` has none: the desktop folder.
pub fn default_cli_path(desktop: &str, year: i32, month: u32, day: u32) -> String {
    // Path.Combine adds a separator unless the folder already ends in one.
    let name = default_file_name(year, month, day) + EXTENSION;
    if desktop.is_empty() {
        name
    } else if desktop.ends_with(['\\', '/', ':']) {
        format!("{desktop}{name}")
    } else {
        format!("{desktop}\\{name}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Setting(Key),
    Color,
    RunAtStartup,
    AppRules,
    FolderRules,
}

/// CaptureSettings' entries after the metadata, in its order. Each name is
/// the AppSettings property (FlyoutHotkey is stored as "Hotkey").
const EXPORTED: &[(&str, Field)] = {
    use Field::*;
    use Key as K;
    &[
        ("UseLegacyIcon", Setting(K::UseLegacyIcon)),
        (
            "TrayIconShowsDeviceType",
            Setting(K::TrayIconShowsDeviceType),
        ),
        ("IsExpanded", Setting(K::IsExpanded)),
        ("UseScrollWheelInTray", Setting(K::UseScrollWheelInTray)),
        (
            "UseGlobalMouseWheelHook",
            Setting(K::UseGlobalMouseWheelHook),
        ),
        ("ShowAppTooltips", Setting(K::ShowAppTooltips)),
        ("IsTelemetryEnabled", Setting(K::IsTelemetryEnabled)),
        ("RunAtStartup", RunAtStartup),
        ("UseLogarithmicVolume", Setting(K::UseLogarithmicVolume)),
        (
            "UseSmoothVolumeAnimation",
            Setting(K::UseSmoothVolumeAnimation),
        ),
        ("UseVolumeTickSound", Setting(K::UseVolumeTickSound)),
        (
            "MonkeyTickSoundUnlocked",
            Setting(K::MonkeyTickSoundUnlocked),
        ),
        ("UseMonkeyTickSound", Setting(K::UseMonkeyTickSound)),
        ("VolumeAnimationSpeed", Setting(K::VolumeAnimationSpeed)),
        ("PeakMeterFps", Setting(K::PeakMeterFps)),
        ("PeakMeterStyle", Setting(K::PeakMeterStyle)),
        ("EcoMode", Setting(K::EcoMode)),
        ("AutoEcoMode", Setting(K::AutoEcoMode)),
        ("UseCustomSliderColors", Setting(K::UseCustomSliderColors)),
        ("SliderThumbColor", Color),
        ("SliderTrackFillColor", Color),
        ("SliderTrackBackgroundColor", Color),
        ("PeakMeterColor", Color),
        ("WindowBackgroundColor", Color),
        (
            "WindowBackgroundOpacity",
            Setting(K::WindowBackgroundOpacity),
        ),
        ("TextColor", Color),
        ("AccentGlowColor", Color),
        ("CustomThemesJson", Setting(K::CustomThemesJson)),
        ("ActiveThemeName", Setting(K::ActiveThemeName)),
        (
            "UseDynamicAlbumArtTheme",
            Setting(K::UseDynamicAlbumArtTheme),
        ),
        ("MediaPopupEnabled", Setting(K::MediaPopupEnabled)),
        ("MediaPopupHoverDelay", Setting(K::MediaPopupHoverDelay)),
        ("MediaPopupBlurRadius", Setting(K::MediaPopupBlurRadius)),
        (
            "MediaPopupShowOnlyWhenPlaying",
            Setting(K::MediaPopupShowOnlyWhenPlaying),
        ),
        (
            "MediaPopupRememberExpanded",
            Setting(K::MediaPopupRememberExpanded),
        ),
        ("MediaPopupIsExpanded", Setting(K::MediaPopupIsExpanded)),
        ("AutoCheckForUpdates", Setting(K::AutoCheckForUpdates)),
        ("UpdateNotifyChannel", Setting(K::UpdateNotifyChannel)),
        ("FlyoutHotkey", Setting(K::FlyoutHotkey)),
        ("MixerHotkey", Setting(K::MixerHotkey)),
        ("SettingsHotkey", Setting(K::SettingsHotkey)),
        ("AbsoluteVolumeUpHotkey", Setting(K::AbsoluteVolumeUpHotkey)),
        (
            "AbsoluteVolumeDownHotkey",
            Setting(K::AbsoluteVolumeDownHotkey),
        ),
        ("QuickTrumpetNextHotkey", Setting(K::QuickTrumpetNextHotkey)),
        (
            "QuickTrumpetPreviousHotkey",
            Setting(K::QuickTrumpetPreviousHotkey),
        ),
        ("VolumeProfilesJson", Setting(K::VolumeProfilesJson)),
        (
            "ShowQuickTrumpetConfirmation",
            Setting(K::ShowQuickTrumpetConfirmation),
        ),
        (
            "QuickTrumpetNotificationSeconds",
            Setting(K::QuickTrumpetNotificationSeconds),
        ),
        ("MixerWindowWidth", Setting(K::MixerWindowWidth)),
        ("MixerWindowHeight", Setting(K::MixerWindowHeight)),
        (
            "NotifyOnDefaultDeviceChange",
            Setting(K::NotifyOnDefaultDeviceChange),
        ),
        ("UseFocusLostVolume", Setting(K::UseFocusLostVolume)),
        (
            "FocusLostAttenuatePercent",
            Setting(K::FocusLostAttenuatePercent),
        ),
        (
            "FocusLostFadeDurationMs",
            Setting(K::FocusLostFadeDurationMs),
        ),
        (
            "FocusLostSelectedAppsOnly",
            Setting(K::FocusLostSelectedAppsOnly),
        ),
        ("AppRulesJson", AppRules),
        ("FolderVolumeRulesJson", FolderRules),
    ]
};

/// What the export reads: the AppSettings getters.
pub trait Source {
    /// The typed getter (defaults and clamps applied).
    fn setting(&mut self, key: Key) -> Value;
    /// `Color.ToString()` of the color getter (Transparent when unset/invalid).
    fn color(&mut self, name: &str) -> String;
    fn run_at_startup(&mut self) -> bool;
    /// AppRulesJson: runs the pending rule migration, then the stored text.
    fn app_rules_json(&mut self) -> Option<String>;
    /// FolderVolumeRulesJson: the loaded rules serialized again.
    fn folder_rules_json(&mut self) -> Option<String>;
}

/// The `_exportedAt` and `_appVersion` metadata.
#[derive(Clone, Copy, Debug)]
pub struct Metadata<'a> {
    /// `DateTime.UtcNow` in .NET ticks.
    pub utc_ticks: i64,
    /// `App.PackageVersion`; `None` exports "unknown".
    pub app_version: Option<&'a str>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ExportError {
    /// SerializeHotkey dereferenced a null hotkey; nothing is written.
    #[error("hotkey {0} is null")]
    NullHotkey(&'static str),
}

/// `DateTime.ToString("o")` for a UTC time: always seven fraction digits.
fn round_trip_utc(ticks: i64) -> String {
    let (year, month, day) = newtonsoft::civil_from_days(ticks / newtonsoft::TICKS_PER_DAY);
    let time = ticks % newtonsoft::TICKS_PER_DAY;
    let seconds = time / 10_000_000;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:07}Z",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        time % 10_000_000
    )
}

/// `JsonConvert.SerializeObject(double)` with FloatFormatHandling.String.
fn json_double(value: f64) -> String {
    if value.is_finite() {
        newtonsoft::format_double(value)
    } else {
        newtonsoft::quote(&newtonsoft::format_double(value))
    }
}

fn json_text(value: Option<&str>) -> String {
    value.map_or_else(|| "null".into(), newtonsoft::quote)
}

/// CaptureSettings, then `SerializeObject(data, Formatting.Indented)`. The
/// text uses CRLF line breaks and is written as UTF-8 without a BOM.
pub fn export(source: &mut impl Source, metadata: Metadata<'_>) -> Result<String, ExportError> {
    let mut entries = vec![
        ("_formatVersion", FORMAT_VERSION.to_string()),
        (
            "_exportedAt",
            newtonsoft::quote(&round_trip_utc(metadata.utc_ticks)),
        ),
        (
            "_appVersion",
            newtonsoft::quote(metadata.app_version.unwrap_or("unknown")),
        ),
    ];
    for &(name, field) in EXPORTED {
        let text = match field {
            Field::Setting(key) => match source.setting(key) {
                Value::Bool(v) => v.to_string(),
                Value::Int(v) => v.to_string(),
                Value::Float(v) => json_double(v),
                Value::Text(v) => json_text(v.as_deref()),
                Value::Hotkey(Some(h)) => format!(
                    "{{\r\n    \"Modifiers\": {},\r\n    \"Key\": {}\r\n  }}",
                    h.modifiers, h.key
                ),
                Value::Hotkey(None) => return Err(ExportError::NullHotkey(name)),
            },
            Field::Color => newtonsoft::quote(&source.color(name)),
            Field::RunAtStartup => source.run_at_startup().to_string(),
            Field::AppRules => json_text(source.app_rules_json().as_deref()),
            Field::FolderRules => json_text(source.folder_rules_json().as_deref()),
        };
        entries.push((name, text));
    }
    let body: Vec<String> = entries
        .iter()
        .map(|(name, value)| format!("  {}: {value}", newtonsoft::quote(name)))
        .collect();
    Ok(format!("{{\r\n{}\r\n}}", body.join(",\r\n")))
}

// ---- import -------------------------------------------------------------------

/// How TrySet/TrySetColor/TrySetHotkey convert a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Conversion {
    Bool,
    Int,
    Double,
    Text,
    Color,
    Hotkey,
}

/// Where an imported value goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Setting(Key),
    /// A color setting stored as `Color.ToString()` text under this name.
    Color(&'static str),
    /// AppRulesJson's setter (blank stores "[]", the cache reloads).
    AppRules,
    /// LegacyHardMutedAppsJson: merges hard mutes into the rules.
    LegacyHardMutedApps,
    /// FolderVolumeRulesJson's setter.
    FolderRules,
}

/// ApplySettings' TrySet calls in its order: file key, conversion, target.
const IMPORTED: &[(&str, Conversion, Target)] = {
    use Conversion as C;
    use Key as K;
    use Target::*;
    &[
        ("UseLegacyIcon", C::Bool, Setting(K::UseLegacyIcon)),
        (
            "TrayIconShowsDeviceType",
            C::Bool,
            Setting(K::TrayIconShowsDeviceType),
        ),
        ("IsExpanded", C::Bool, Setting(K::IsExpanded)),
        (
            "UseScrollWheelInTray",
            C::Bool,
            Setting(K::UseScrollWheelInTray),
        ),
        (
            "UseGlobalMouseWheelHook",
            C::Bool,
            Setting(K::UseGlobalMouseWheelHook),
        ),
        ("ShowAppTooltips", C::Bool, Setting(K::ShowAppTooltips)),
        (
            "IsTelemetryEnabled",
            C::Bool,
            Setting(K::IsTelemetryEnabled),
        ),
        // RunAtStartup is deliberately not imported.
        (
            "UseLogarithmicVolume",
            C::Bool,
            Setting(K::UseLogarithmicVolume),
        ),
        (
            "UseSmoothVolumeAnimation",
            C::Bool,
            Setting(K::UseSmoothVolumeAnimation),
        ),
        (
            "UseVolumeTickSound",
            C::Bool,
            Setting(K::UseVolumeTickSound),
        ),
        (
            "MonkeyTickSoundUnlocked",
            C::Bool,
            Setting(K::MonkeyTickSoundUnlocked),
        ),
        (
            "UseMonkeyTickSound",
            C::Bool,
            Setting(K::UseMonkeyTickSound),
        ),
        (
            "VolumeAnimationSpeed",
            C::Double,
            Setting(K::VolumeAnimationSpeed),
        ),
        ("PeakMeterFps", C::Int, Setting(K::PeakMeterFps)),
        ("PeakMeterStyle", C::Int, Setting(K::PeakMeterStyle)),
        ("EcoMode", C::Bool, Setting(K::EcoMode)),
        ("AutoEcoMode", C::Bool, Setting(K::AutoEcoMode)),
        (
            "UseCustomSliderColors",
            C::Bool,
            Setting(K::UseCustomSliderColors),
        ),
        ("SliderThumbColor", C::Color, Color("SliderThumbColor")),
        (
            "SliderTrackFillColor",
            C::Color,
            Color("SliderTrackFillColor"),
        ),
        (
            "SliderTrackBackgroundColor",
            C::Color,
            Color("SliderTrackBackgroundColor"),
        ),
        ("PeakMeterColor", C::Color, Color("PeakMeterColor")),
        (
            "WindowBackgroundColor",
            C::Color,
            Color("WindowBackgroundColor"),
        ),
        (
            "WindowBackgroundOpacity",
            C::Double,
            Setting(K::WindowBackgroundOpacity),
        ),
        ("TextColor", C::Color, Color("TextColor")),
        ("AccentGlowColor", C::Color, Color("AccentGlowColor")),
        ("CustomThemesJson", C::Text, Setting(K::CustomThemesJson)),
        ("ActiveThemeName", C::Text, Setting(K::ActiveThemeName)),
        (
            "UseDynamicAlbumArtTheme",
            C::Bool,
            Setting(K::UseDynamicAlbumArtTheme),
        ),
        ("MediaPopupEnabled", C::Bool, Setting(K::MediaPopupEnabled)),
        (
            "MediaPopupHoverDelay",
            C::Double,
            Setting(K::MediaPopupHoverDelay),
        ),
        (
            "MediaPopupBlurRadius",
            C::Double,
            Setting(K::MediaPopupBlurRadius),
        ),
        (
            "MediaPopupShowOnlyWhenPlaying",
            C::Bool,
            Setting(K::MediaPopupShowOnlyWhenPlaying),
        ),
        (
            "MediaPopupRememberExpanded",
            C::Bool,
            Setting(K::MediaPopupRememberExpanded),
        ),
        (
            "MediaPopupIsExpanded",
            C::Bool,
            Setting(K::MediaPopupIsExpanded),
        ),
        (
            "AutoCheckForUpdates",
            C::Bool,
            Setting(K::AutoCheckForUpdates),
        ),
        (
            "UpdateNotifyChannel",
            C::Int,
            Setting(K::UpdateNotifyChannel),
        ),
        (
            "ShowQuickTrumpetConfirmation",
            C::Bool,
            Setting(K::ShowQuickTrumpetConfirmation),
        ),
        (
            "QuickTrumpetNotificationSeconds",
            C::Int,
            Setting(K::QuickTrumpetNotificationSeconds),
        ),
        ("MixerWindowWidth", C::Double, Setting(K::MixerWindowWidth)),
        (
            "MixerWindowHeight",
            C::Double,
            Setting(K::MixerWindowHeight),
        ),
        (
            "NotifyOnDefaultDeviceChange",
            C::Bool,
            Setting(K::NotifyOnDefaultDeviceChange),
        ),
        (
            "UseFocusLostVolume",
            C::Bool,
            Setting(K::UseFocusLostVolume),
        ),
        (
            "FocusLostAttenuatePercent",
            C::Int,
            Setting(K::FocusLostAttenuatePercent),
        ),
        (
            "FocusLostFadeDurationMs",
            C::Int,
            Setting(K::FocusLostFadeDurationMs),
        ),
        (
            "FocusLostSelectedAppsOnly",
            C::Bool,
            Setting(K::FocusLostSelectedAppsOnly),
        ),
        ("FlyoutHotkey", C::Hotkey, Setting(K::FlyoutHotkey)),
        ("MixerHotkey", C::Hotkey, Setting(K::MixerHotkey)),
        ("SettingsHotkey", C::Hotkey, Setting(K::SettingsHotkey)),
        (
            "AbsoluteVolumeUpHotkey",
            C::Hotkey,
            Setting(K::AbsoluteVolumeUpHotkey),
        ),
        (
            "AbsoluteVolumeDownHotkey",
            C::Hotkey,
            Setting(K::AbsoluteVolumeDownHotkey),
        ),
        (
            "QuickTrumpetNextHotkey",
            C::Hotkey,
            Setting(K::QuickTrumpetNextHotkey),
        ),
        (
            "QuickTrumpetPreviousHotkey",
            C::Hotkey,
            Setting(K::QuickTrumpetPreviousHotkey),
        ),
        (
            "VolumeProfilesJson",
            C::Text,
            Setting(K::VolumeProfilesJson),
        ),
        // New rules first; the legacy hard mutes merge into them.
        ("AppRulesJson", C::Text, AppRules),
        ("HardMutedAppsJson", C::Text, LegacyHardMutedApps),
        ("FolderVolumeRulesJson", C::Text, FolderRules),
    ]
};

/// A converted value, as the AppSettings setter receives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Imported {
    Bool(bool),
    Int(i32),
    Float(f64),
    Text(Option<String>),
    /// `Color.ToString()` of the parsed color (what the setter stores).
    Color(String),
    Hotkey(HotkeyData),
}

/// One setter call ApplySettings makes.
#[derive(Clone, Debug, PartialEq)]
pub struct Assignment {
    /// The file key (`HardMutedAppsJson` for the legacy rules).
    pub key: &'static str,
    pub target: Target,
    pub value: Imported,
}

impl Assignment {
    /// The AppSettings property the original sets.
    pub fn property(&self) -> &'static str {
        match self.target {
            Target::LegacyHardMutedApps => "LegacyHardMutedAppsJson",
            _ => self.key,
        }
    }
    /// The value for a [`Target::Setting`].
    pub fn setting_value(&self) -> Value {
        match &self.value {
            Imported::Bool(v) => Value::Bool(*v),
            Imported::Int(v) => Value::Int(*v),
            Imported::Float(v) => Value::Float(*v),
            Imported::Text(v) => Value::Text(v.clone()),
            Imported::Color(v) => Value::Text(Some(v.clone())),
            Imported::Hotkey(v) => Value::Hotkey(Some(*v)),
        }
    }
    /// The text for string targets (`None` is a null string).
    pub fn text(&self) -> Option<&str> {
        match &self.value {
            Imported::Text(v) => v.as_deref(),
            Imported::Color(v) => Some(v),
            _ => None,
        }
    }
}

/// ImportFromFile answers false: the JSON did not parse, or was null/empty.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("Invalid settings file: empty or malformed JSON.")]
pub struct ImportError;

/// `File.ReadAllText`, `DeserializeObject<Dictionary<string, object>>`, then
/// ApplySettings' conversions. The assignments are applied in order; any
/// field that does not convert is skipped, as the original catches it.
pub fn import(
    bytes: &[u8],
    zone: &(impl LocalZone + ?Sized),
) -> Result<Vec<Assignment>, ImportError> {
    let text = crate::legacy_store::decode_text(bytes);
    let members = newtonsoft::parse_object_document(&text, zone)
        .map_err(|_| ImportError)?
        .ok_or(ImportError)?;
    // Dictionary keys are ordinal; a repeated key keeps its last value.
    let get = |key: &str| members.iter().rev().find(|(k, _)| k == key).map(|e| &e.1);
    let mut assignments = vec![];
    for &(key, conversion, target) in IMPORTED {
        let Some(token) = get(key) else {
            continue;
        };
        let value = match conversion {
            Conversion::Bool => change_bool(token).map(Imported::Bool),
            Conversion::Int => change_int(token).map(Imported::Int),
            Conversion::Double => change_double(token).map(Imported::Float),
            Conversion::Text => change_text(token).map(Imported::Text),
            Conversion::Color => color(token).map(Imported::Color),
            Conversion::Hotkey => hotkey(token).map(Imported::Hotkey),
        };
        if let Some(value) = value {
            assignments.push(Assignment { key, target, value });
        }
    }
    Ok(assignments)
}

/// `Convert.ToInt32(double)`: banker's rounding; NaN and overflow throw.
fn double_to_i32(value: f64) -> Option<i32> {
    if !(-2_147_483_648.5..2_147_483_647.5).contains(&value) {
        return None;
    }
    let truncated = value.trunc();
    let difference = value - truncated;
    let odd = truncated as i64 & 1 != 0;
    let rounded = if difference > 0.5 || difference == 0.5 && odd {
        truncated + 1.0
    } else if difference < -0.5 || difference == -0.5 && odd {
        truncated - 1.0
    } else {
        truncated
    };
    Some(rounded as i32)
}

/// `double.ToString()` under the invariant culture.
fn double_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        newtonsoft::round_trip(value)
    }
}

/// `DateTime.ToString()` under the invariant culture ("G").
fn date_text(date: &Date) -> String {
    let (year, month, day) = newtonsoft::civil_from_days(date.ticks / newtonsoft::TICKS_PER_DAY);
    let seconds = date.ticks % newtonsoft::TICKS_PER_DAY / 10_000_000;
    format!(
        "{month:02}/{day:02}/{year:04} {:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

// `Convert.ChangeType(value, typeof(T))` on the boxed values a dictionary
// holds: bool, long, double, string, DateTime, BigInteger (not IConvertible,
// so it always throws), JObject/JArray (also throw) and null (throws for
// value types, stays null for string).

fn change_bool(token: &Token) -> Option<bool> {
    match token {
        Token::Bool(v) => Some(*v),
        Token::Integer(v) => Some(*v != 0),
        Token::Float(v) => Some(*v != 0.0),
        Token::String(s) => newtonsoft::parse_bool(s),
        _ => None,
    }
}

fn change_int(token: &Token) -> Option<i32> {
    match token {
        Token::Bool(v) => Some(i32::from(*v)),
        Token::Integer(v) => i32::try_from(*v).ok(),
        Token::Float(v) => double_to_i32(*v),
        Token::String(s) => IntegerFormat::INVARIANT.parse_i32(s),
        _ => None,
    }
}

fn change_double(token: &Token) -> Option<f64> {
    match token {
        Token::Bool(v) => Some(f64::from(u8::from(*v))),
        Token::Integer(v) => Some(*v as f64),
        Token::Float(v) => Some(*v),
        Token::String(s) => colors::parse_double(s, &NumberCulture::INVARIANT),
        _ => None,
    }
}

fn change_text(token: &Token) -> Option<Option<String>> {
    Some(Some(match token {
        Token::Null | Token::Undefined => return Some(None),
        Token::Bool(true) => "True".into(),
        Token::Bool(false) => "False".into(),
        Token::Integer(v) => v.to_string(),
        Token::Float(v) => double_text(*v),
        Token::String(s) => s.clone(),
        Token::Date(d) => date_text(d),
        _ => return None,
    }))
}

/// TrySetColor: `ToString()` of the value through ColorConverter. Only
/// string values can name a color; empty text is skipped.
fn color(token: &Token) -> Option<String> {
    let Token::String(text) = token else {
        return None;
    };
    if text.is_empty() {
        return None;
    }
    let culture = NumberCulture::INVARIANT;
    colors::parse(text, &culture)
        .ok()
        .map(|c| c.to_text(&culture))
}

/// `(int)JToken`: numbers, numeric strings and booleans.
fn jtoken_int(token: &Token) -> Option<i32> {
    match token {
        Token::Integer(v) => i32::try_from(*v).ok(),
        Token::Float(v) => double_to_i32(*v),
        Token::String(s) => IntegerFormat::INVARIANT.parse_i32(s),
        Token::Bool(v) => Some(i32::from(*v)),
        _ => None,
    }
}

/// TrySetHotkey: a JSON object's exact-case `Modifiers` and `Key`.
fn hotkey(token: &Token) -> Option<HotkeyData> {
    let Token::Object(members) = token else {
        return None;
    };
    let get = |name: &str| members.iter().rev().find(|(k, _)| k == name).map(|e| &e.1);
    let modifiers = jtoken_int(get("Modifiers")?)?;
    let key = jtoken_int(get("Key")?)?;
    Some(HotkeyData { modifiers, key })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_hold_exactly_the_original_fields() {
        assert_eq!(EXPORTED.len(), 57);
        assert_eq!(IMPORTED.len(), 57);
        assert!(!IMPORTED.iter().any(|e| e.0 == "RunAtStartup"));
    }

    #[test]
    fn paths() {
        assert_eq!(
            default_cli_path(r"C:\Users\a\Desktop", 2026, 3, 4),
            r"C:\Users\a\Desktop\BetterTrumpet_Settings_2026-03-04.btsettings"
        );
        assert_eq!(
            default_file_name(999, 12, 31),
            "BetterTrumpet_Settings_0999-12-31"
        );
    }

    #[test]
    fn bankers_rounding() {
        for (v, r) in [
            (0.5, Some(0)),
            (1.5, Some(2)),
            (-1.5, Some(-2)),
            (-2.5, Some(-2)),
        ] {
            assert_eq!(double_to_i32(v), r, "{v}");
        }
        assert_eq!(double_to_i32(f64::NAN), None);
    }
}
