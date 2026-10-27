//! Global hotkeys. Uses the user's existing BetterTrumpet bindings (read
//! read-only from its registry settings) when present, otherwise defaults.
use bt_core::{
    legacy_xml::{Kind, Value},
    models::HotkeyData,
    registry_values::{Read, Request},
};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleFlyout,
    VolumeUp,
    VolumeDown,
}

pub struct Binding {
    pub action: Action,
    pub shortcut: Shortcut,
    pub label: String,
    /// True when taken from the user's BetterTrumpet settings.
    pub from_settings: bool,
}

// Windows Forms Keys modifier flags.
const SHIFT: i32 = 0x0001_0000;
const CONTROL: i32 = 0x0002_0000;
const ALT: i32 = 0x0004_0000;
const LWIN: i32 = 91;

fn code(key: i32) -> Option<(Code, String)> {
    let letter = |i: i32| char::from(b'A' + i as u8).to_string();
    let codes_letters = [
        Code::KeyA,
        Code::KeyB,
        Code::KeyC,
        Code::KeyD,
        Code::KeyE,
        Code::KeyF,
        Code::KeyG,
        Code::KeyH,
        Code::KeyI,
        Code::KeyJ,
        Code::KeyK,
        Code::KeyL,
        Code::KeyM,
        Code::KeyN,
        Code::KeyO,
        Code::KeyP,
        Code::KeyQ,
        Code::KeyR,
        Code::KeyS,
        Code::KeyT,
        Code::KeyU,
        Code::KeyV,
        Code::KeyW,
        Code::KeyX,
        Code::KeyY,
        Code::KeyZ,
    ];
    let digits = [
        Code::Digit0,
        Code::Digit1,
        Code::Digit2,
        Code::Digit3,
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
    ];
    let functions = [
        Code::F1,
        Code::F2,
        Code::F3,
        Code::F4,
        Code::F5,
        Code::F6,
        Code::F7,
        Code::F8,
        Code::F9,
        Code::F10,
        Code::F11,
        Code::F12,
        Code::F13,
        Code::F14,
        Code::F15,
        Code::F16,
        Code::F17,
        Code::F18,
        Code::F19,
        Code::F20,
        Code::F21,
        Code::F22,
        Code::F23,
        Code::F24,
    ];
    let numpad = [
        Code::Numpad0,
        Code::Numpad1,
        Code::Numpad2,
        Code::Numpad3,
        Code::Numpad4,
        Code::Numpad5,
        Code::Numpad6,
        Code::Numpad7,
        Code::Numpad8,
        Code::Numpad9,
    ];
    Some(match key {
        65..=90 => (codes_letters[(key - 65) as usize], letter(key - 65)),
        48..=57 => (digits[(key - 48) as usize], (key - 48).to_string()),
        96..=105 => (numpad[(key - 96) as usize], format!("Num {}", key - 96)),
        112..=135 => (functions[(key - 112) as usize], format!("F{}", key - 111)),
        32 => (Code::Space, "Space".into()),
        33 => (Code::PageUp, "PageUp".into()),
        34 => (Code::PageDown, "PageDown".into()),
        35 => (Code::End, "End".into()),
        36 => (Code::Home, "Home".into()),
        37 => (Code::ArrowLeft, "Left".into()),
        38 => (Code::ArrowUp, "Up".into()),
        39 => (Code::ArrowRight, "Right".into()),
        40 => (Code::ArrowDown, "Down".into()),
        45 => (Code::Insert, "Insert".into()),
        46 => (Code::Delete, "Delete".into()),
        106 => (Code::NumpadMultiply, "Num *".into()),
        107 => (Code::NumpadAdd, "Num +".into()),
        109 => (Code::NumpadSubtract, "Num -".into()),
        111 => (Code::NumpadDivide, "Num /".into()),
        173 => (Code::AudioVolumeMute, "Mute".into()),
        174 => (Code::AudioVolumeDown, "Volume Down".into()),
        175 => (Code::AudioVolumeUp, "Volume Up".into()),
        176 => (Code::MediaTrackNext, "Next Track".into()),
        177 => (Code::MediaTrackPrevious, "Previous Track".into()),
        179 => (Code::MediaPlayPause, "Play/Pause".into()),
        186 => (Code::Semicolon, ";".into()),
        187 => (Code::Equal, "=".into()),
        188 => (Code::Comma, ",".into()),
        189 => (Code::Minus, "-".into()),
        190 => (Code::Period, ".".into()),
        191 => (Code::Slash, "/".into()),
        192 => (Code::Backquote, "`".into()),
        219 => (Code::BracketLeft, "[".into()),
        220 => (Code::Backslash, "\\".into()),
        221 => (Code::BracketRight, "]".into()),
        222 => (Code::Quote, "'".into()),
        _ => return None,
    })
}

/// A BetterTrumpet HotkeyData as a global shortcut and its display label.
fn from_hotkey(hotkey: &HotkeyData) -> Option<(Shortcut, String)> {
    let (code, key_label) = code(hotkey.key)?;
    let mut modifiers = Modifiers::empty();
    let mut label = String::new();
    let m = hotkey.modifiers;
    for (flag, modifier, name) in [
        (CONTROL, Modifiers::CONTROL, "Ctrl+"),
        (ALT, Modifiers::ALT, "Alt+"),
        (SHIFT, Modifiers::SHIFT, "Shift+"),
    ] {
        if m & flag == flag {
            modifiers |= modifier;
            label.push_str(name);
        }
    }
    if m & LWIN == LWIN {
        modifiers |= Modifiers::SUPER;
        label.insert_str(0, "Win+");
    }
    label.push_str(&key_label);
    Some((Shortcut::new(Some(modifiers), code), label))
}

fn defaults(action: Action) -> (Shortcut, String) {
    let ctrl_alt = Some(Modifiers::CONTROL | Modifiers::ALT);
    match action {
        Action::ToggleFlyout => (Shortcut::new(ctrl_alt, Code::KeyV), "Ctrl+Alt+V".into()),
        Action::VolumeUp => (
            Shortcut::new(ctrl_alt, Code::PageUp),
            "Ctrl+Alt+PageUp".into(),
        ),
        Action::VolumeDown => (
            Shortcut::new(ctrl_alt, Code::PageDown),
            "Ctrl+Alt+PageDown".into(),
        ),
    }
}

/// The user's BetterTrumpet hotkey for `name`, read-only from the registry.
fn legacy(name: &str) -> Option<HotkeyData> {
    use bt_app::win32::registry::{RegistryEnvironment, read_legacy_settings};
    let snapshot = read_legacy_settings().ok()?;
    match snapshot.read(
        Some(name),
        Request::Value(Kind::Hotkey),
        &RegistryEnvironment,
    ) {
        Read::Value(Value::Hotkey(Some(h))) if h.key != 0 => Some(h),
        _ => None,
    }
}

/// Bindings for every supported action: the user's BetterTrumpet setting
/// when it maps to a shortcut, otherwise this build's default.
pub fn bindings() -> Vec<Binding> {
    [
        (Action::ToggleFlyout, "Hotkey"),
        (Action::VolumeUp, "AbsoluteVolumeUpHotkey"),
        (Action::VolumeDown, "AbsoluteVolumeDownHotkey"),
    ]
    .into_iter()
    .map(
        |(action, setting)| match legacy(setting).as_ref().and_then(from_hotkey) {
            Some((shortcut, label)) => Binding {
                action,
                shortcut,
                label,
                from_settings: true,
            },
            None => {
                let (shortcut, label) = defaults(action);
                Binding {
                    action,
                    shortcut,
                    label,
                    from_settings: false,
                }
            }
        },
    )
    .collect()
}

pub fn describe(action: Action) -> &'static str {
    match action {
        Action::ToggleFlyout => "Open volume flyout",
        Action::VolumeUp => "Volume up",
        Action::VolumeDown => "Volume down",
    }
}
