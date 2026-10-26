//! Newtonsoft `DeserializeObject<List<T>>` decoding of the AppSettings rule
//! and hidden-entry lists into their DTOs, plus the loaders' catch-to-empty
//! boundaries. Normalization itself lives in [`crate::rules`]. Pinned by
//! typed_lists_reference.json (raw, hand-edited inputs through the original
//! loaders).
use crate::{
    dotnet_datetime,
    models::{
        AppRuleEntry, HiddenAppEntry, HiddenDeviceEntry, LegacyHardMutedAppEntry, VolumeRuleMode,
    },
    newtonsoft::{self, Error, LocalZone, Token},
    rules::{self, LoadedRules},
};

const VOLUME_RULE_MODES: &[(&str, i32)] = &[("None", 0), ("Launch", 1), ("Lock", 2)];

/// Per-field typed readers shared by the record decoders.
struct Fields<'a, Z: ?Sized> {
    zone: &'a Z,
    today_ticks: i64,
}
impl<Z: LocalZone + ?Sized> Fields<'_, Z> {
    fn string(&self, token: &Token) -> Result<Option<String>, Error> {
        newtonsoft::typed_string(token)
    }
    fn bool(&self, token: &Token) -> Result<bool, Error> {
        newtonsoft::typed_bool(token)?.ok_or(Error)
    }
    fn int(&self, token: &Token) -> Result<i32, Error> {
        newtonsoft::typed_int32(token)?.ok_or(Error)
    }
    fn date(&self, token: &Token) -> Result<String, Error> {
        let date = newtonsoft::typed_datetime(token, self.zone, |text| {
            dotnet_datetime::try_parse(text, self.today_ticks, self.zone)
        })?
        .ok_or(Error)?;
        Ok(newtonsoft::format_date(&date))
    }
    fn skip(&self, token: &Token) -> Result<(), Error> {
        newtonsoft::check_read(token, self.zone)
    }
}

fn decode<T, Z: LocalZone + ?Sized>(
    json: &str,
    zone: &Z,
    today_ticks: i64,
    names: &[&str],
    mut assign: impl FnMut(&Fields<'_, Z>, &mut T, usize, &Token) -> Result<(), Error>,
) -> Result<Option<Vec<Option<T>>>, Error>
where
    T: Default,
{
    let fields = Fields { zone, today_ticks };
    let Some(root) = newtonsoft::parse_typed_document(json, zone)? else {
        return Ok(None);
    };
    let Token::Array(items) = root else {
        return Err(Error);
    };
    let mut out = Vec::with_capacity(items.len());
    for item in &items {
        match item {
            Token::Null | Token::Undefined => out.push(None),
            Token::Object(members) => {
                let mut value = T::default();
                for (name, member) in members {
                    match newtonsoft::member_index(names, name) {
                        Some(i) => assign(&fields, &mut value, i, member)?,
                        None => fields.skip(member)?,
                    }
                }
                out.push(Some(value));
            }
            _ => return Err(Error),
        }
    }
    Ok(Some(out))
}

pub fn decode_app_rules(
    json: &str,
    zone: &(impl LocalZone + ?Sized),
    today_ticks: i64,
) -> Result<Option<Vec<Option<AppRuleEntry>>>, Error> {
    let names = [
        "ExeName",
        "DisplayName",
        "IconPath",
        "IsDesktopApp",
        "IsPinned",
        "HardMuted",
        "FocusLostEnabled",
        "VolumeMode",
        "VolumePercent",
        "CreatedAtUtc",
    ];
    decode(
        json,
        zone,
        today_ticks,
        &names,
        |f, e: &mut AppRuleEntry, i, t| {
            match i {
                0 => e.exe_name = f.string(t)?,
                1 => e.display_name = f.string(t)?,
                2 => e.icon_path = f.string(t)?,
                3 => e.is_desktop_app = f.bool(t)?,
                4 => e.is_pinned = f.bool(t)?,
                5 => e.hard_muted = f.bool(t)?,
                6 => e.focus_lost_enabled = f.bool(t)?,
                7 => {
                    e.volume_mode =
                        VolumeRuleMode(newtonsoft::typed_enum(t, VOLUME_RULE_MODES, f.zone)?)
                }
                8 => e.volume_percent = f.int(t)?,
                _ => e.created_at_utc = f.date(t)?,
            }
            Ok(())
        },
    )
}

pub fn decode_legacy_hard_mutes(
    json: &str,
    zone: &(impl LocalZone + ?Sized),
    today_ticks: i64,
) -> Result<Option<Vec<Option<LegacyHardMutedAppEntry>>>, Error> {
    let names = ["ExeName", "DisplayName", "HardMutedAtUtc"];
    decode(
        json,
        zone,
        today_ticks,
        &names,
        |f, e: &mut LegacyHardMutedAppEntry, i, t| {
            match i {
                0 => e.exe_name = f.string(t)?,
                1 => e.display_name = f.string(t)?,
                _ => e.hard_muted_at_utc = f.date(t)?,
            }
            Ok(())
        },
    )
}

pub fn decode_hidden_apps(
    json: &str,
    zone: &(impl LocalZone + ?Sized),
    today_ticks: i64,
) -> Result<Option<Vec<Option<HiddenAppEntry>>>, Error> {
    let names = ["DeviceId", "AppId", "ExeName", "DisplayName", "HiddenAtUtc"];
    decode(
        json,
        zone,
        today_ticks,
        &names,
        |f, e: &mut HiddenAppEntry, i, t| {
            match i {
                0 => e.device_id = f.string(t)?,
                1 => e.app_id = f.string(t)?,
                2 => e.exe_name = f.string(t)?,
                3 => e.display_name = f.string(t)?,
                _ => e.hidden_at_utc = f.date(t)?,
            }
            Ok(())
        },
    )
}

pub fn decode_hidden_devices(
    json: &str,
    zone: &(impl LocalZone + ?Sized),
    today_ticks: i64,
) -> Result<Option<Vec<Option<HiddenDeviceEntry>>>, Error> {
    let names = ["DeviceId", "DisplayName", "HiddenAtUtc"];
    decode(
        json,
        zone,
        today_ticks,
        &names,
        |f, e: &mut HiddenDeviceEntry, i, t| {
            match i {
                0 => e.device_id = f.string(t)?,
                1 => e.display_name = f.string(t)?,
                _ => e.hidden_at_utc = f.date(t)?,
            }
            Ok(())
        },
    )
}

/// `EnsureAppRulesLoaded` with its legacy migration. `current` is the bag's
/// value for AppRuleEntriesJson (absent reads as "[]"); `legacy` is `None`
/// when HardMutedAppEntriesJson does not exist. Read errors become empty
/// lists, and a failed legacy read skips the migration.
pub fn load_app_rules(
    current: Option<&str>,
    legacy: Option<&str>,
    zone: &(impl LocalZone + ?Sized),
    today_ticks: i64,
) -> LoadedRules {
    let current = decode_app_rules(current.unwrap_or("[]"), zone, today_ticks)
        .ok()
        .flatten()
        .unwrap_or_default();
    let legacy = legacy.and_then(|json| {
        decode_legacy_hard_mutes(json, zone, today_ticks)
            .ok()
            .map(Option::unwrap_or_default)
    });
    rules::load_apps(current, legacy)
}

/// `EnsureHiddenAppsLoaded`.
pub fn load_hidden_apps(
    stored: Option<&str>,
    zone: &(impl LocalZone + ?Sized),
    today_ticks: i64,
) -> Vec<HiddenAppEntry> {
    let entries = decode_hidden_apps(stored.unwrap_or("[]"), zone, today_ticks)
        .ok()
        .flatten()
        .unwrap_or_default();
    rules::normalize_hidden_apps(entries)
}

/// `EnsureHiddenDevicesLoaded`.
pub fn load_hidden_devices(
    stored: Option<&str>,
    zone: &(impl LocalZone + ?Sized),
    today_ticks: i64,
) -> Vec<HiddenDeviceEntry> {
    let entries = decode_hidden_devices(stored.unwrap_or("[]"), zone, today_ticks)
        .ok()
        .flatten()
        .unwrap_or_default();
    rules::normalize_hidden_devices(entries)
}
