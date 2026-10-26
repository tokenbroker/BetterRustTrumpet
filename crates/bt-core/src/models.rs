//! Persisted DTOs from AppSettings.cs, HotkeyData.cs and VolumeProfileService.cs.
//! Preserve null vs absent/default values and Newtonsoft's case-insensitive keys.
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, Visitor},
    ser::SerializeStruct,
};
use std::fmt;

pub const CURRENT_PROFILE_SCHEMA: i32 = 3;
pub const DOTNET_MIN_DATETIME: &str = "0001-01-01T00:00:00";

// Newtonsoft applies fields in encounter order and ignores unknown fields.
// The macro keeps that behavior without routing objects through a sorted map.
macro_rules! legacy_record {
    ($name:ident { $($field:ident : $ty:ty = $default:expr => $wire:literal),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq, Serialize)]
        pub struct $name { $(#[serde(rename = $wire)] pub $field: $ty),* }
        impl Default for $name { fn default() -> Self { Self { $($field: $default),* } } }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct RecordVisitor;
                impl<'de> Visitor<'de> for RecordVisitor {
                    type Value = $name;
                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(stringify!($name)) }
                    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                        let mut result = $name::default();
                        while let Some(key) = map.next_key::<String>()? {
                            $(if key.eq_ignore_ascii_case($wire) { result.$field = map.next_value()?; continue; })*
                            map.next_value::<de::IgnoredAny>()?;
                        }
                        Ok(result)
                    }
                }
                deserializer.deserialize_map(RecordVisitor)
            }
        }
    };
}

macro_rules! legacy_enum {
    ($name:ident { $($variant:ident = $value:expr => $wire:literal),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub i32);
        impl $name { $(pub const $variant: Self = Self($value);)* }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = serde_json::Value::deserialize(deserializer)?;
                if let Some(number) = value.as_i64().and_then(|n| i32::try_from(n).ok()) { return Ok(Self(number)); }
                if let Some(text) = value.as_str() {
                    if let Ok(number) = text.trim().parse::<i32>() { return Ok(Self(number)); }
                    $(if text.trim().eq_ignore_ascii_case($wire) { return Ok(Self($value)); })*
                }
                Err(de::Error::custom(concat!("invalid ", stringify!($name))))
            }
        }
    };
}

legacy_enum!(CaptureScope { CURRENT_DEVICE = 0 => "CurrentDevice", ALL_DEVICES = 1 => "AllDevices" });
legacy_enum!(DefaultDeviceRole {
    PLAYBACK = 0 => "Playback", PLAYBACK_COMMUNICATIONS = 1 => "PlaybackCommunications",
    RECORDING = 2 => "Recording", RECORDING_COMMUNICATIONS = 3 => "RecordingCommunications"
});
legacy_enum!(VolumeRuleMode { NONE = 0 => "None", LAUNCH = 1 => "Launch", LOCK = 2 => "Lock" });

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HotkeyData {
    pub modifiers: i32,
    pub key: i32,
}

impl HotkeyData {
    pub fn is_empty(&self) -> bool {
        self.modifiers == 0 && self.key == 0
    }
}

impl Serialize for HotkeyData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("HotkeyData", 3)?;
        out.serialize_field("Modifiers", &self.modifiers)?;
        out.serialize_field("Key", &self.key)?;
        // Public readable .NET property is included in profile JSON, but not
        // in SettingsExportService's explicit two-field hotkey projection.
        out.serialize_field("IsEmpty", &self.is_empty())?;
        out.end()
    }
}

impl<'de> Deserialize<'de> for HotkeyData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        legacy_record!(HotkeyWire { modifiers: i32 = 0 => "Modifiers", key: i32 = 0 => "Key" });
        let wire = HotkeyWire::deserialize(deserializer)?;
        Ok(Self {
            modifiers: wire.modifiers,
            key: wire.key,
        })
    }
}

legacy_record!(HiddenAppEntry {
    device_id: Option<String> = None => "DeviceId", app_id: Option<String> = None => "AppId",
    exe_name: Option<String> = None => "ExeName", display_name: Option<String> = None => "DisplayName",
    hidden_at_utc: String = DOTNET_MIN_DATETIME.into() => "HiddenAtUtc"
});
legacy_record!(HiddenDeviceEntry {
    device_id: Option<String> = None => "DeviceId", display_name: Option<String> = None => "DisplayName",
    hidden_at_utc: String = DOTNET_MIN_DATETIME.into() => "HiddenAtUtc"
});
legacy_record!(AppRuleEntry {
    exe_name: Option<String> = None => "ExeName", display_name: Option<String> = None => "DisplayName",
    icon_path: Option<String> = None => "IconPath", is_desktop_app: bool = false => "IsDesktopApp",
    is_pinned: bool = false => "IsPinned", hard_muted: bool = false => "HardMuted",
    focus_lost_enabled: bool = false => "FocusLostEnabled", volume_mode: VolumeRuleMode = VolumeRuleMode::NONE => "VolumeMode",
    volume_percent: i32 = 0 => "VolumePercent", created_at_utc: String = DOTNET_MIN_DATETIME.into() => "CreatedAtUtc"
});
impl AppRuleEntry {
    pub fn has_volume_rule(&self) -> bool {
        self.volume_mode != VolumeRuleMode::NONE
    }
    pub fn has_active_behavior(&self) -> bool {
        self.hard_muted || self.focus_lost_enabled || self.has_volume_rule()
    }
}
legacy_record!(FolderVolumeRuleEntry {
    id: Option<String> = None => "Id", folder_path: Option<String> = None => "FolderPath",
    volume_percent: i32 = 0 => "VolumePercent", created_at_utc: String = DOTNET_MIN_DATETIME.into() => "CreatedAtUtc"
});
legacy_record!(LegacyHardMutedAppEntry {
    exe_name: Option<String> = None => "ExeName", display_name: Option<String> = None => "DisplayName",
    hard_muted_at_utc: String = DOTNET_MIN_DATETIME.into() => "HardMutedAtUtc"
});
legacy_record!(AppVolumeEntry {
    exe_name: Option<String> = None => "ExeName", app_id: Option<String> = None => "AppId",
    display_name: Option<String> = None => "DisplayName", device_id: Option<String> = None => "DeviceId",
    device_display_name: Option<String> = None => "DeviceDisplayName", volume: i32 = 0 => "Volume", is_muted: bool = false => "IsMuted"
});
legacy_record!(DeviceVolumeEntry {
    device_id: Option<String> = None => "DeviceId", display_name: Option<String> = None => "DisplayName",
    volume: i32 = 0 => "Volume", is_muted: bool = false => "IsMuted", include_volume: bool = true => "IncludeVolume",
    apps: Option<Vec<Option<AppVolumeEntry>>> = Some(Vec::new()) => "Apps"
});
legacy_record!(DefaultDeviceEntry {
    device_id: Option<String> = None => "DeviceId", display_name: Option<String> = None => "DisplayName"
});
legacy_record!(DefaultDeviceSet {
    playback: Option<DefaultDeviceEntry> = None => "Playback",
    playback_communications: Option<DefaultDeviceEntry> = None => "PlaybackCommunications",
    recording: Option<DefaultDeviceEntry> = None => "Recording",
    recording_communications: Option<DefaultDeviceEntry> = None => "RecordingCommunications"
});

impl DefaultDeviceSet {
    pub fn get(&self, role: DefaultDeviceRole) -> Option<&DefaultDeviceEntry> {
        let entry = match role {
            DefaultDeviceRole::PLAYBACK => &self.playback,
            DefaultDeviceRole::PLAYBACK_COMMUNICATIONS => &self.playback_communications,
            DefaultDeviceRole::RECORDING => &self.recording,
            DefaultDeviceRole::RECORDING_COMMUNICATIONS => &self.recording_communications,
            _ => return None,
        };
        entry.as_ref().filter(|e| {
            e.device_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
        })
    }
    pub fn is_empty(&self) -> bool {
        (0..4).all(|role| self.get(DefaultDeviceRole(role)).is_none())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VolumeProfile {
    pub schema_version: i32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub slug: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub capture_scope: CaptureScope,
    pub include_device_volumes: bool,
    pub include_app_volumes: bool,
    pub route_apps: bool,
    pub default_devices: Option<DefaultDeviceSet>,
    pub hotkey: Option<HotkeyData>,
    pub devices: Option<Vec<Option<DeviceVolumeEntry>>>,
}

impl Default for VolumeProfile {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_PROFILE_SCHEMA,
            id: None,
            name: None,
            slug: None,
            created_at: None,
            updated_at: None,
            capture_scope: CaptureScope::CURRENT_DEVICE,
            include_device_volumes: true,
            include_app_volumes: true,
            route_apps: true,
            default_devices: Some(DefaultDeviceSet::default()),
            hotkey: Some(HotkeyData::default()),
            devices: Some(Vec::new()),
        }
    }
}

impl VolumeProfile {
    pub fn apply_apps_only(&self) -> bool {
        !self.include_device_volumes
    }
}

impl Serialize for VolumeProfile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("VolumeProfile", 14)?;
        macro_rules! field {
            ($key:literal, $field:ident) => {
                out.serialize_field($key, &self.$field)?;
            };
        }
        field!("SchemaVersion", schema_version);
        field!("Id", id);
        field!("Name", name);
        field!("Slug", slug);
        field!("CreatedAt", created_at);
        field!("UpdatedAt", updated_at);
        field!("CaptureScope", capture_scope);
        field!("IncludeDeviceVolumes", include_device_volumes);
        field!("IncludeAppVolumes", include_app_volumes);
        field!("RouteApps", route_apps);
        field!("DefaultDevices", default_devices);
        out.serialize_field("ApplyAppsOnly", &self.apply_apps_only())?;
        field!("Hotkey", hotkey);
        field!("Devices", devices);
        out.end()
    }
}

impl<'de> Deserialize<'de> for VolumeProfile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ProfileVisitor;
        impl<'de> Visitor<'de> for ProfileVisitor {
            type Value = VolumeProfile;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("VolumeProfile")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut result = VolumeProfile::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.to_ascii_lowercase().as_str() {
                        "schemaversion" => result.schema_version = map.next_value()?,
                        "id" => result.id = map.next_value()?,
                        "name" => result.name = map.next_value()?,
                        "slug" => result.slug = map.next_value()?,
                        "createdat" => result.created_at = map.next_value()?,
                        "updatedat" => result.updated_at = map.next_value()?,
                        "capturescope" => result.capture_scope = map.next_value()?,
                        "includedevicevolumes" => {
                            result.include_device_volumes = map.next_value()?
                        }
                        "includeappvolumes" => result.include_app_volumes = map.next_value()?,
                        "routeapps" => result.route_apps = map.next_value()?,
                        "defaultdevices" => result.default_devices = map.next_value()?,
                        "hotkey" => result.hotkey = map.next_value()?,
                        "devices" => result.devices = map.next_value()?,
                        "applyappsonly" => {
                            result.include_device_volumes = !map.next_value::<bool>()?
                        }
                        _ => {
                            map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(result)
            }
        }
        deserializer.deserialize_map(ProfileVisitor)
    }
}
