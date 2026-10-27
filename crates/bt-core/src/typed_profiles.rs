//! Typed Json.NET profile decoding. Unlike serde's canonical DTO reader,
//! duplicate object properties populate existing objects and arrays append.
use crate::{
    models::*,
    newtonsoft::{self as json, Error, LocalZone, Token},
};

trait Record: Default {
    const NAMES: &'static [&'static str];
    fn assign(&mut self, index: usize, token: &Token, zone: &dyn LocalZone) -> Result<(), Error>;
}

fn populate<T: Record>(value: &mut T, token: &Token, zone: &dyn LocalZone) -> Result<(), Error> {
    let Token::Object(members) = token else {
        return Err(Error);
    };
    for (name, token) in members {
        match json::member_index(T::NAMES, name) {
            Some(index) => value.assign(index, token, zone)?,
            None => json::check_read(token, zone)?,
        }
    }
    Ok(())
}

fn object<T: Record>(
    value: &mut Option<T>,
    token: &Token,
    zone: &dyn LocalZone,
) -> Result<(), Error> {
    if matches!(token, Token::Null | Token::Undefined) {
        *value = None;
        return Ok(());
    }
    populate(value.get_or_insert_default(), token, zone)
}

fn list<T: Record>(
    value: &mut Option<Vec<Option<T>>>,
    token: &Token,
    zone: &dyn LocalZone,
) -> Result<(), Error> {
    if matches!(token, Token::Null | Token::Undefined) {
        *value = None;
        return Ok(());
    }
    let Token::Array(items) = token else {
        return Err(Error);
    };
    let out = value.get_or_insert_default();
    for item in items {
        let mut entry = None;
        object(&mut entry, item, zone)?;
        out.push(entry);
    }
    Ok(())
}

fn boolean(token: &Token) -> Result<bool, Error> {
    json::typed_bool(token)?.ok_or(Error)
}
fn integer(token: &Token) -> Result<i32, Error> {
    json::typed_int32(token)?.ok_or(Error)
}

impl Record for VolumeProfile {
    const NAMES: &'static [&'static str] = &[
        "SchemaVersion",
        "Id",
        "Name",
        "Slug",
        "CreatedAt",
        "UpdatedAt",
        "CaptureScope",
        "IncludeDeviceVolumes",
        "IncludeAppVolumes",
        "RouteApps",
        "DefaultDevices",
        "ApplyAppsOnly",
        "Hotkey",
        "Devices",
        "VolumeDevices",
        "AllApps",
    ];
    fn assign(&mut self, index: usize, token: &Token, zone: &dyn LocalZone) -> Result<(), Error> {
        match index {
            0 => self.schema_version = integer(token)?,
            1 => self.id = json::typed_string(token)?,
            2 => self.name = json::typed_string(token)?,
            3 => self.slug = json::typed_string(token)?,
            4 => self.created_at = json::typed_string(token)?,
            5 => self.updated_at = json::typed_string(token)?,
            6 => {
                self.capture_scope = CaptureScope(json::typed_enum(
                    token,
                    &[("CurrentDevice", 0), ("AllDevices", 1)],
                    zone,
                )?)
            }
            7 => self.include_device_volumes = boolean(token)?,
            8 => self.include_app_volumes = boolean(token)?,
            9 => self.route_apps = boolean(token)?,
            10 => object(&mut self.default_devices, token, zone)?,
            11 => self.include_device_volumes = !boolean(token)?,
            12 => object(&mut self.hotkey, token, zone)?,
            13 => list(&mut self.devices, token, zone)?,
            _ => json::check_read(token, zone)?, // JsonIgnore members.
        }
        Ok(())
    }
}

impl Record for DeviceVolumeEntry {
    const NAMES: &'static [&'static str] = &[
        "DeviceId",
        "DisplayName",
        "Volume",
        "IsMuted",
        "IncludeVolume",
        "Apps",
    ];
    fn assign(&mut self, index: usize, token: &Token, zone: &dyn LocalZone) -> Result<(), Error> {
        match index {
            0 => self.device_id = json::typed_string(token)?,
            1 => self.display_name = json::typed_string(token)?,
            2 => self.volume = integer(token)?,
            3 => self.is_muted = boolean(token)?,
            4 => self.include_volume = boolean(token)?,
            _ => list(&mut self.apps, token, zone)?,
        }
        Ok(())
    }
}

impl Record for AppVolumeEntry {
    const NAMES: &'static [&'static str] = &[
        "ExeName",
        "AppId",
        "DisplayName",
        "DeviceId",
        "DeviceDisplayName",
        "Volume",
        "IsMuted",
    ];
    fn assign(&mut self, index: usize, token: &Token, _: &dyn LocalZone) -> Result<(), Error> {
        match index {
            0 => self.exe_name = json::typed_string(token)?,
            1 => self.app_id = json::typed_string(token)?,
            2 => self.display_name = json::typed_string(token)?,
            3 => self.device_id = json::typed_string(token)?,
            4 => self.device_display_name = json::typed_string(token)?,
            5 => self.volume = integer(token)?,
            _ => self.is_muted = boolean(token)?,
        }
        Ok(())
    }
}

impl Record for DefaultDeviceSet {
    const NAMES: &'static [&'static str] = &[
        "Playback",
        "PlaybackCommunications",
        "Recording",
        "RecordingCommunications",
        "IsEmpty",
    ];
    fn assign(&mut self, index: usize, token: &Token, zone: &dyn LocalZone) -> Result<(), Error> {
        match index {
            0 => object(&mut self.playback, token, zone)?,
            1 => object(&mut self.playback_communications, token, zone)?,
            2 => object(&mut self.recording, token, zone)?,
            3 => object(&mut self.recording_communications, token, zone)?,
            _ => json::check_read(token, zone)?,
        }
        Ok(())
    }
}

impl Record for DefaultDeviceEntry {
    const NAMES: &'static [&'static str] = &["DeviceId", "DisplayName"];
    fn assign(&mut self, index: usize, token: &Token, _: &dyn LocalZone) -> Result<(), Error> {
        if index == 0 {
            self.device_id = json::typed_string(token)?;
        } else {
            self.display_name = json::typed_string(token)?;
        }
        Ok(())
    }
}

impl Record for HotkeyData {
    const NAMES: &'static [&'static str] = &["Modifiers", "Key", "IsEmpty"];
    fn assign(&mut self, index: usize, token: &Token, zone: &dyn LocalZone) -> Result<(), Error> {
        match index {
            0 => self.modifiers = json::typed_keys(token, zone)?,
            1 => self.key = json::typed_keys(token, zone)?,
            _ => {
                json::typed_bool(token)?;
            } // Read-only, conversion still occurs.
        }
        Ok(())
    }
}

/// Raw `DeserializeObject<List<VolumeProfile>>`, before normalization.
pub fn decode(
    json: &str,
    zone: &dyn LocalZone,
) -> Result<Option<Vec<Option<VolumeProfile>>>, Error> {
    let Some(root) = json::parse_typed_document(json, zone)? else {
        return Ok(None);
    };
    let mut out = None;
    list(&mut out, &root, zone)?;
    Ok(out)
}

/// Raw single-profile decoder used by ImportProfile.
pub fn decode_one(json: &str, zone: &dyn LocalZone) -> Result<Option<VolumeProfile>, Error> {
    let Some(root) = json::parse_typed_document(json, zone)? else {
        return Ok(None);
    };
    let mut out = None;
    object(&mut out, &root, zone)?;
    Ok(out)
}
