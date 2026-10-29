//! Typed, decoded AppSettings values. Registry/XML/portable decoding belongs in
//! a separate adapter; constructing this bag never opens or modifies user data.
use std::collections::BTreeMap;

use crate::{models::HotkeyData, text::is_blank};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Float(f64),
    Text(Option<String>),
    Hotkey(Option<HotkeyData>),
}

macro_rules! keys {
    ($($name:ident: $variant:ident($default:expr)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Key { $($name),* }
        impl Key {
            pub const ALL: &'static [Self] = &[$(Self::$name),*];
            pub fn name(self) -> &'static str { match self { $(Self::$name => stringify!($name)),* } }
            pub fn default_value(self) -> Value { match self { $(Self::$name => Value::$variant($default)),* } }
            fn accepts(self, value: &Value) -> bool { matches!((self, value), $((Self::$name, Value::$variant(_)))|*) }
        }
    }
}

keys! {
    UseLegacyIcon: Bool(false), IsExpanded: Bool(false), HasShownFirstRun: Bool(false),
    UseScrollWheelInTray: Bool(true), UseGlobalMouseWheelHook: Bool(false),
    TrayIconShowsDeviceType: Bool(false), ShowAppTooltips: Bool(true),
    // Port-only (D51): the tray playback animation can be turned off.
    AnimateTrayIcon: Bool(true),
    IsTelemetryEnabled: Bool(true), TelemetryAnonymousId: Text(Some(String::new())),
    VoteAnonymousId: Text(Some(String::new())), AnnouncementsEnabled: Bool(true),
    HasCompletedOnboarding: Bool(false), UseLogarithmicVolume: Bool(false),
    UseSmoothVolumeAnimation: Bool(true), VolumeAnimationSpeed: Float(0.08),
    UseVolumeTickSound: Bool(true), MonkeyTickSoundUnlocked: Bool(false),
    UseMonkeyTickSound: Bool(false), ShowDeviceSwitchNotification: Bool(true),
    PeakMeterFps: Int(30), EcoMode: Bool(false), AutoEcoMode: Bool(true),
    UseCustomSliderColors: Bool(false), PeakMeterStyle: Int(0),
    CustomThemesJson: Text(Some("[]".into())), ActiveThemeName: Text(Some(String::new())),
    LastSeenVersion: Text(Some(String::new())), LastSeenAnnouncementId: Text(Some(String::new())),
    WindowBackgroundOpacity: Float(0.7), VolumeProfilesJson: Text(Some("[]".into())),
    ShowQuickTrumpetConfirmation: Bool(true), QuickTrumpetNotificationSeconds: Int(3),
    QuickTrumpetLastAppliedId: Text(Some(String::new())), UseDynamicAlbumArtTheme: Bool(false),
    MixerWindowWidth: Float(0.0), MixerWindowHeight: Float(0.0),
    NotifyOnDefaultDeviceChange: Bool(false), NotifyOnDeviceHotkeyChange: Bool(true),
    UseFocusLostVolume: Bool(false), FocusLostAttenuatePercent: Int(0),
    FocusLostFadeDurationMs: Int(0), FocusLostSelectedAppsOnly: Bool(false),
    MediaPopupEnabled: Bool(true), MediaPopupHoverDelay: Float(2.0), MediaPopupBlurRadius: Float(15.0),
    MediaPopupShowOnlyWhenPlaying: Bool(false), MediaPopupRememberExpanded: Bool(true),
    MediaPopupIsExpanded: Bool(false), AutoCheckForUpdates: Bool(true), UpdateNotifyChannel: Int(0),
    FlyoutHotkey: Hotkey(Some(HotkeyData {modifiers: 0, key: 0})),
    MixerHotkey: Hotkey(Some(HotkeyData {modifiers: 0, key: 0})),
    SettingsHotkey: Hotkey(Some(HotkeyData {modifiers: 0, key: 0})),
    AbsoluteVolumeUpHotkey: Hotkey(Some(HotkeyData {modifiers: 0, key: 0})),
    AbsoluteVolumeDownHotkey: Hotkey(Some(HotkeyData {modifiers: 0, key: 0})),
    SwitchDeviceHotkey: Hotkey(Some(HotkeyData {modifiers: 0, key: 0})),
    QuickTrumpetNextHotkey: Hotkey(Some(HotkeyData {modifiers: 0, key: 0})),
    QuickTrumpetPreviousHotkey: Hotkey(Some(HotkeyData {modifiers: 0, key: 0})),
}

impl Key {
    pub fn storage_key(self) -> &'static str {
        match self {
            Self::FlyoutHotkey => "Hotkey",
            Self::HasShownFirstRun => "hasShownFirstRun",
            _ => self.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    UseLegacyIconChanged,
    TrayIconShowsDeviceTypeChanged,
    AppTooltipsChanged,
    TelemetryConsentChanged,
    AnnouncementsEnabledChanged,
    EcoModeChanged,
    CustomSliderColorsChanged,
    PeakMeterStyleChanged,
    MediaPopupSettingsChanged,
    RegisterQuickTrumpetHotkeys,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub wrote: bool,
    pub events: Vec<Event>,
    /// The app unregisters the previous hotkey before writing, then registers
    /// the new one. Native registration stays outside this pure model.
    pub hotkey_replacement: Option<(Option<HotkeyData>, Option<HotkeyData>)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    pub region_code: &'a str,
    pub on_battery: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("wrong decoded value type for {0}")]
pub struct TypeError(pub &'static str);

#[derive(Clone, Debug, Default)]
pub struct Settings {
    // Retain decoded keys unknown to this version. The migration adapter must
    // additionally preserve raw XML/JSON fields it cannot decode.
    pub values: BTreeMap<String, Value>,
    batch: bool,
}

/// Preserve the original exclusion list, including GB/ME/overseas territories.
pub fn telemetry_default(region_code: &str) -> bool {
    ![
        "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "GR", "HU", "IE", "IT",
        "LV", "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE", "GB", "GF", "GP",
        "MQ", "ME", "YT", "RE", "MF", "GI", "AX", "PM", "GL", "BL", "SX", "AW", "CW", "WF", "PF",
        "NC", "TF", "AI", "BM", "IO", "VG", "KY", "FK", "MS", "PN", "SH", "GS", "TC", "AD", "LI",
        "MC", "SM", "VA", "JE", "GG",
    ]
    .contains(&region_code)
}

// Rust's min/max skip NaN; .NET Math.Min/Max propagate it.
fn float_clamp(value: f64, min: f64, max: f64) -> f64 {
    if value.is_nan() {
        value
    } else {
        value.clamp(min, max)
    }
}

impl Settings {
    pub fn has_telemetry_consent(&self) -> bool {
        self.values.contains_key("IsTelemetryEnabled")
    }

    pub fn get(&self, key: Key, context: Context<'_>) -> Value {
        use Key::*;
        if key == HasShownFirstRun {
            return Value::Bool(self.values.contains_key(key.storage_key()));
        }
        let mut value = self
            .values
            .get(key.storage_key())
            .filter(|v| key.accepts(v))
            .cloned()
            .unwrap_or_else(|| match key {
                IsTelemetryEnabled => Value::Bool(telemetry_default(context.region_code)),
                _ => key.default_value(),
            });
        if key == UseLegacyIcon {
            value = Value::Bool(
                matches!(self.values.get(key.storage_key()), Some(Value::Text(Some(s))) if s.trim_matches(|c: char| c.is_whitespace() || c == '\0').eq_ignore_ascii_case("true")),
            );
        }
        match (&mut value, key) {
            (Value::Bool(v), UseMonkeyTickSound) => {
                *v &= self.get(MonkeyTickSoundUnlocked, context) == Value::Bool(true)
            }
            (Value::Int(v), PeakMeterStyle) => *v = (*v).min(4),
            (Value::Int(v), QuickTrumpetNotificationSeconds) => *v = (*v).clamp(1, 10),
            (Value::Int(v), FocusLostAttenuatePercent) => *v = (*v).clamp(0, 100),
            (Value::Int(v), FocusLostFadeDurationMs) => *v = (*v).clamp(0, 5000),
            (Value::Int(v), UpdateNotifyChannel) if !(0..=3).contains(v) => *v = 0,
            (Value::Float(v), WindowBackgroundOpacity) => *v = float_clamp(*v, 0.05, 1.0),
            _ => {}
        }
        value
    }

    pub fn set(
        &mut self,
        key: Key,
        mut value: Value,
        context: Context<'_>,
    ) -> Result<Change, TypeError> {
        use Event as E;
        use Key::*;
        if !key.accepts(&value) {
            return Err(TypeError(key.name()));
        }
        let old = self.get(key, context);
        if key == AnnouncementsEnabled && old == value {
            return Ok(Change {
                wrote: false,
                events: vec![],
                hotkey_replacement: None,
            });
        }
        match (&mut value, key) {
            (Value::Bool(v), UseMonkeyTickSound) => {
                *v &= self.get(MonkeyTickSoundUnlocked, context) == Value::Bool(true)
            }
            (Value::Int(v), QuickTrumpetNotificationSeconds) => *v = (*v).clamp(1, 10),
            (Value::Int(v), FocusLostAttenuatePercent) => *v = (*v).clamp(0, 100),
            (Value::Int(v), FocusLostFadeDurationMs) => *v = (*v).clamp(0, 5000),
            (Value::Float(v), WindowBackgroundOpacity) => *v = float_clamp(*v, 0.05, 1.0),
            (Value::Float(v), MediaPopupHoverDelay) => *v = float_clamp(*v, 0.5, 5.0),
            (Value::Float(v), MediaPopupBlurRadius) => *v = float_clamp(*v, 0.0, 30.0),
            (Value::Text(v), VolumeProfilesJson) => {
                v.get_or_insert_with(|| "[]".into());
            }
            (Value::Text(v), QuickTrumpetLastAppliedId) => {
                v.get_or_insert_default();
            }
            _ => {}
        }
        let events = match key {
            UseLegacyIcon => vec![E::UseLegacyIconChanged],
            TrayIconShowsDeviceType => vec![E::TrayIconShowsDeviceTypeChanged],
            ShowAppTooltips => vec![E::AppTooltipsChanged],
            IsTelemetryEnabled => vec![E::TelemetryConsentChanged],
            AnnouncementsEnabled => vec![E::AnnouncementsEnabledChanged],
            EcoMode => vec![E::EcoModeChanged],
            UseCustomSliderColors | CustomThemesJson | UseDynamicAlbumArtTheme => {
                vec![E::CustomSliderColorsChanged]
            }
            PeakMeterStyle if self.batch => vec![E::PeakMeterStyleChanged],
            PeakMeterStyle => vec![E::PeakMeterStyleChanged, E::CustomSliderColorsChanged],
            WindowBackgroundOpacity if !self.batch => vec![E::CustomSliderColorsChanged],
            MediaPopupEnabled
            | MediaPopupHoverDelay
            | MediaPopupBlurRadius
            | MediaPopupShowOnlyWhenPlaying => vec![E::MediaPopupSettingsChanged],
            VolumeProfilesJson => vec![E::RegisterQuickTrumpetHotkeys],
            _ => vec![],
        };
        let hotkey_replacement = match (&old, &value) {
            (Value::Hotkey(old), Value::Hotkey(new)) => Some((*old, *new)),
            _ => None,
        };
        let stored = if let (UseLegacyIcon, Value::Bool(v)) = (key, &value) {
            Value::Text(Some(if *v { "True" } else { "False" }.into()))
        } else {
            value
        };
        self.values.insert(key.storage_key().into(), stored);
        Ok(Change {
            wrote: true,
            events,
            hotkey_replacement,
        })
    }

    pub fn device_hotkey(&self, device_id: Option<&str>) -> Option<HotkeyData> {
        let id = device_id.unwrap_or_default();
        if !id.is_empty() {
            if let Some(Value::Hotkey(hotkey)) = self.values.get(&format!("DeviceHotkey:{id}")) {
                return *hotkey;
            }
        }
        Some(HotkeyData::default())
    }

    pub fn set_device_hotkey(
        &mut self,
        device_id: Option<&str>,
        hotkey: Option<HotkeyData>,
    ) -> Change {
        let id = device_id.unwrap_or_default();
        if id.is_empty() {
            return Change {
                wrote: false,
                events: vec![],
                hotkey_replacement: None,
            };
        }
        let old = self.device_hotkey(Some(id));
        self.values
            .insert(format!("DeviceHotkey:{id}"), Value::Hotkey(hotkey));
        Change {
            wrote: true,
            events: vec![],
            hotkey_replacement: Some((old, hotkey)),
        }
    }

    pub fn poll_vote(&self, announcement_id: Option<&str>) -> Option<String> {
        let id = announcement_id.unwrap_or_default();
        if !id.is_empty() {
            if let Some(Value::Text(value)) = self.values.get(&format!("PollVote:{id}")) {
                return value.clone();
            }
        }
        Some(String::new())
    }

    pub fn set_poll_vote(
        &mut self,
        announcement_id: Option<&str>,
        answers_json: Option<&str>,
    ) -> bool {
        let id = announcement_id.unwrap_or_default();
        if id.is_empty() {
            return false;
        }
        self.values.insert(
            format!("PollVote:{id}"),
            Value::Text(Some(answers_json.unwrap_or_default().into())),
        );
        true
    }

    pub fn begin_batch(&mut self) {
        self.batch = true;
    }
    pub fn end_batch(&mut self) -> Event {
        self.batch = false;
        Event::CustomSliderColorsChanged
    }
    pub fn effective_eco_mode(&self, context: Context<'_>) -> bool {
        self.get(Key::EcoMode, context) == Value::Bool(true)
            || (context.on_battery && self.get(Key::AutoEcoMode, context) == Value::Bool(true))
    }
    pub fn effective_peak_fps(&self, context: Context<'_>) -> i32 {
        if self.effective_eco_mode(context) {
            20
        } else if let Value::Int(fps) = self.get(Key::PeakMeterFps, context) {
            fps
        } else {
            unreachable!()
        }
    }
    /// The legacy property allocates only when read. Call this on explicit vote
    /// flows, rather than snapshotting every property during settings loading.
    pub fn vote_id(&mut self, allocate_id: &mut impl FnMut() -> String) -> String {
        match self.values.get("VoteAnonymousId") {
            Some(Value::Text(Some(id))) if !is_blank(Some(id)) => id.clone(),
            _ => {
                let id = allocate_id();
                self.values
                    .insert("VoteAnonymousId".into(), Value::Text(Some(id.clone())));
                id
            }
        }
    }
}
