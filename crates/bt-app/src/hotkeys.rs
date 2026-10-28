//! Global hotkeys. Uses the user's existing BetterTrumpet bindings (from its
//! settings, see `store`) when present, otherwise defaults for the flyout
//! and volume keys. The other actions (AppSettings.RegisterHotkeys, the
//! QuickTrumpet preset shortcuts and App.RegisterDeviceHotkeys) are only
//! registered when set, as in the original. WinForms `Keys` values (the
//! original's HotkeyData) carry the virtual-key code in their low 16 bits,
//! so they map straight onto RegisterHotKey.
use crate::store::Store;
use bt_core::{
    models::{HotkeyData, VolumeProfile},
    profiles,
    settings::{Key, Value},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleFlyout,
    OpenMixer,
    OpenSettings,
    VolumeUp,
    VolumeDown,
    SwitchDevice,
    PresetNext,
    PresetPrevious,
    /// A QuickTrumpet preset, by slug (or name when it has none).
    Preset(String),
    /// Makes this output the default (DeviceHotkey:<id>).
    Device(String),
}

impl Action {
    /// Device shortcuts have their own handler in the original and fire
    /// alongside whichever AppSettings action shares the keys.
    pub fn is_device(&self) -> bool {
        matches!(self, Action::Device(_))
    }
}

pub struct Binding {
    pub action: Action,
    /// RegisterHotKey MOD_* flags.
    pub modifiers: u32,
    pub vk: u32,
    pub label: String,
    /// True when taken from the user's BetterTrumpet settings.
    pub from_settings: bool,
}

// Windows Forms Keys modifier flags.
const SHIFT: i32 = 0x0001_0000;
const CONTROL: i32 = 0x0002_0000;
const ALT: i32 = 0x0004_0000;
const LWIN: i32 = 91;
// RegisterHotKey modifiers.
const MOD_ALT: u32 = 0x1;
const MOD_CONTROL: u32 = 0x2;
const MOD_SHIFT: u32 = 0x4;
const MOD_WIN: u32 = 0x8;

/// A key's display name, or `None` for keys the original cannot bind.
fn key_name(key: i32) -> Option<String> {
    Some(match key {
        65..=90 => char::from(key as u8).to_string(),
        48..=57 => (key - 48).to_string(),
        96..=105 => format!("Num {}", key - 96),
        112..=135 => format!("F{}", key - 111),
        32 => "Space".into(),
        33 => "PageUp".into(),
        34 => "PageDown".into(),
        35 => "End".into(),
        36 => "Home".into(),
        37 => "Left".into(),
        38 => "Up".into(),
        39 => "Right".into(),
        40 => "Down".into(),
        45 => "Insert".into(),
        46 => "Delete".into(),
        106 => "Num *".into(),
        107 => "Num +".into(),
        109 => "Num -".into(),
        111 => "Num /".into(),
        173 => "Mute".into(),
        174 => "Volume Down".into(),
        175 => "Volume Up".into(),
        176 => "Next Track".into(),
        177 => "Previous Track".into(),
        179 => "Play/Pause".into(),
        186 => ";".into(),
        187 => "=".into(),
        188 => ",".into(),
        189 => "-".into(),
        190 => ".".into(),
        191 => "/".into(),
        192 => "`".into(),
        219 => "[".into(),
        220 => "\\".into(),
        221 => "]".into(),
        222 => "'".into(),
        _ => return None,
    })
}

/// A BetterTrumpet HotkeyData as RegisterHotKey arguments and a label.
fn from_hotkey(hotkey: &HotkeyData) -> Option<(u32, u32, String)> {
    let name = key_name(hotkey.key)?;
    let mut modifiers = 0;
    let mut label = String::new();
    let m = hotkey.modifiers;
    for (flag, modifier, text) in [
        (CONTROL, MOD_CONTROL, "Ctrl+"),
        (ALT, MOD_ALT, "Alt+"),
        (SHIFT, MOD_SHIFT, "Shift+"),
    ] {
        if m & flag == flag {
            modifiers |= modifier;
            label.push_str(text);
        }
    }
    if m & LWIN == LWIN {
        modifiers |= MOD_WIN;
        label.insert_str(0, "Win+");
    }
    label.push_str(&name);
    Some((modifiers, hotkey.key as u32, label))
}

fn defaults(action: &Action) -> Option<(u32, u32, String)> {
    let ctrl_alt = MOD_CONTROL | MOD_ALT;
    Some(match action {
        Action::ToggleFlyout => (ctrl_alt, u32::from(b'V'), "Ctrl+Alt+V".into()),
        Action::VolumeUp => (ctrl_alt, 0x21, "Ctrl+Alt+PageUp".into()),
        Action::VolumeDown => (ctrl_alt, 0x22, "Ctrl+Alt+PageDown".into()),
        _ => return None,
    })
}

/// The user's BetterTrumpet hotkey for `key`, if one is set.
fn legacy(store: &Store, key: Key) -> Option<HotkeyData> {
    match store.get(key) {
        Value::Hotkey(Some(h)) if h.key != 0 => Some(h),
        _ => None,
    }
}

fn binding(action: Action, hotkey: Option<HotkeyData>) -> Option<Binding> {
    let from = hotkey
        .filter(|h| !h.is_empty())
        .as_ref()
        .and_then(from_hotkey);
    let from_settings = from.is_some();
    let (modifiers, vk, label) = from.or_else(|| defaults(&action))?;
    Some(Binding {
        action,
        modifiers,
        vk,
        label,
        from_settings,
    })
}

/// Bindings in the original's dispatch order: the AppSettings chain
/// (flyout, settings, mixer, volume up/down, switch device, next/previous
/// preset, preset shortcuts in list order), then the shortcuts of the
/// active outputs `devices`.
pub fn bindings(store: &Store, presets: &[VolumeProfile], devices: &[String]) -> Vec<Binding> {
    let mut out: Vec<Binding> = [
        (Action::ToggleFlyout, Key::FlyoutHotkey),
        (Action::OpenSettings, Key::SettingsHotkey),
        (Action::OpenMixer, Key::MixerHotkey),
        (Action::VolumeUp, Key::AbsoluteVolumeUpHotkey),
        (Action::VolumeDown, Key::AbsoluteVolumeDownHotkey),
        (Action::SwitchDevice, Key::SwitchDeviceHotkey),
        (Action::PresetNext, Key::QuickTrumpetNextHotkey),
        (Action::PresetPrevious, Key::QuickTrumpetPreviousHotkey),
    ]
    .into_iter()
    .filter_map(|(action, setting)| binding(action, legacy(store, setting)))
    .collect();
    for preset in presets {
        let key = profiles::get_slug(preset);
        let key = if key.trim().is_empty() {
            preset.name.clone().unwrap_or_default()
        } else {
            key
        };
        out.extend(binding(Action::Preset(key), preset.hotkey));
    }
    for id in devices {
        let hotkey = store.hotkey(&format!("DeviceHotkey:{id}"));
        out.extend(binding(Action::Device(id.clone()), hotkey));
    }
    out
}

pub fn describe(action: &Action) -> String {
    match action {
        Action::ToggleFlyout => "Open volume flyout".into(),
        Action::OpenMixer => "Open volume mixer".into(),
        Action::OpenSettings => "Open settings".into(),
        Action::VolumeUp => "Volume up".into(),
        Action::VolumeDown => "Volume down".into(),
        Action::SwitchDevice => "Switch playback device".into(),
        Action::PresetNext => "Next QuickTrumpet preset".into(),
        Action::PresetPrevious => "Previous QuickTrumpet preset".into(),
        Action::Preset(key) => format!("QuickTrumpet preset {key}"),
        Action::Device(id) => format!("Default device {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_winforms_keys_to_hotkeys() {
        let hotkey = HotkeyData {
            modifiers: CONTROL | SHIFT,
            key: 77,
        };
        assert_eq!(
            from_hotkey(&hotkey),
            Some((MOD_CONTROL | MOD_SHIFT, 77, "Ctrl+Shift+M".into()))
        );
        let win = HotkeyData {
            modifiers: LWIN | ALT,
            key: 112,
        };
        assert_eq!(
            from_hotkey(&win),
            Some((MOD_ALT | MOD_WIN, 112, "Win+Alt+F1".into()))
        );
        let unknown = HotkeyData {
            modifiers: 0,
            key: 3,
        };
        assert_eq!(from_hotkey(&unknown), None);
    }
}
