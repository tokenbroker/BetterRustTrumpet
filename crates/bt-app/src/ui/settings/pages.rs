//! The settings pages as data: each page is a list of sections, each
//! section a list of rows with one control bound to a setting. Labels,
//! order and value transforms follow the original's web settings
//! (SettingsPages.tsx and WebSettingsWindow.ApplySetting, D48, Q12).
//!
//! A new page is a `PageId`, an entry in `GROUPS` and a `build` arm; a new
//! row kind is a `Control` variant drawn in `widgets` and laid out in the
//! window module.
use super::{
    super::theme::{self, CHANNEL_KEYS},
    lists,
};
use crate::{core::Core, hotkeys, store::with_env};
use bt_app::win32::{gfx::Rgba, startup};
use bt_audio::Role;
use bt_core::{
    hidden::AppKey,
    models::HotkeyData,
    settings::{Key, Value},
    themes::BUILTIN_THEMES,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageId {
    General,
    Mouse,
    Shortcuts,
    QuickTrumpet,
    AppRules,
    Media,
    Appearance,
    Performance,
    Updates,
    Privacy,
    About,
}

/// The sidebar groups (App.tsx groupPages).
pub const GROUPS: [(&str, &[PageId]); 4] = [
    (
        "Essentials",
        &[PageId::General, PageId::Mouse, PageId::Shortcuts],
    ),
    (
        "Audio",
        &[PageId::QuickTrumpet, PageId::AppRules, PageId::Media],
    ),
    ("Experience", &[PageId::Appearance, PageId::Performance]),
    (
        "Application",
        &[PageId::Updates, PageId::Privacy, PageId::About],
    ),
];

impl PageId {
    pub fn all() -> impl Iterator<Item = PageId> {
        GROUPS.iter().flat_map(|(_, pages)| pages.iter().copied())
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Mouse => "Volume & mouse",
            Self::Shortcuts => "Shortcuts",
            Self::QuickTrumpet => "QuickTrumpet",
            Self::AppRules => "App rules",
            Self::Media => "Media popup",
            Self::Appearance => "Appearance",
            Self::Performance => "Performance",
            Self::Updates => "Updates",
            Self::Privacy => "Privacy & data",
            Self::About => "About",
        }
    }
    pub fn subtitle(self) -> &'static str {
        match self {
            Self::General => "Startup, tray icon, tooltips, and hidden apps.",
            Self::Mouse => "Scroll wheel behavior and volume scaling.",
            Self::Shortcuts => "Assign global hotkeys to quickly control audio.",
            Self::QuickTrumpet => {
                "Switch default devices and volumes in one go, from here, a shortcut, Raycast or the bt CLI."
            }
            Self::AppRules => {
                "Saved app profiles with independent mute, focus-loss, and volume behavior"
            }
            Self::Media => "Control how media controls appear on tray hover.",
            Self::Appearance => "Customize how BetterTrumpet looks on your desktop.",
            Self::Performance => "Tune refresh rates, animations, and battery usage.",
            Self::Updates => "Choose when to be notified and install new versions.",
            Self::Privacy => "Telemetry, announcements and settings backup.",
            Self::About => "Version info, telemetry, and diagnostics.",
        }
    }
    /// Segoe Fluent Icons / MDL2 glyph for the sidebar.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::General => "\u{E713}",
            Self::Mouse => "\u{E962}",
            Self::Shortcuts => "\u{E765}",
            Self::QuickTrumpet => "\u{E74E}",
            Self::AppRules => "\u{E762}",
            Self::Media => "\u{E8D6}",
            Self::Appearance => "\u{E790}",
            Self::Performance => "\u{E9D9}",
            Self::Updates => "\u{E895}",
            Self::Privacy => "\u{E72E}",
            Self::About => "\u{E946}",
        }
    }
}

pub struct Section {
    pub title: String,
    pub desc: Option<String>,
    pub rows: Vec<Row>,
}

pub struct Row {
    pub label: String,
    pub desc: Option<String>,
    pub control: Control,
    pub enabled: bool,
    /// An accent pill after the label ("Last applied", "Default").
    pub badge: Option<String>,
    /// Quiet pills left of the control ("50%", "Keep muted").
    pub chips: Vec<String>,
    /// Buttons right of the control (Apply, mute, remove).
    pub trail: Vec<Button>,
    /// An expandable list item: clicking the line runs the action; the
    /// flag is whether it is open (its chevron points up).
    pub expand: Option<(Action, bool)>,
    /// A detail row of the open list item above it.
    pub indent: bool,
}

impl Default for Row {
    fn default() -> Self {
        Self {
            label: String::new(),
            desc: None,
            control: Control::None,
            enabled: true,
            badge: None,
            chips: vec![],
            trail: vec![],
            expand: None,
            indent: false,
        }
    }
}

/// What a shortcut control records into.
#[derive(Clone, Debug, PartialEq)]
pub enum HotkeyTarget {
    Setting(Key),
    /// A QuickTrumpet preset, by list index (SetProfileHotkey).
    Preset(usize),
    /// DeviceHotkey:<id>.
    Device(String),
}

/// The window's text boxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Theme,
    Preset,
    Rename,
    Exe,
}

/// The open list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expand {
    /// By id (or slug, or name).
    Preset(String),
    /// By exe name.
    Rule(String),
    /// By id.
    Folder(String),
}

/// A preset's three parts (SetProfileIncludes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Include {
    Devices,
    Apps,
    Route,
}

/// The capture form's option lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choose {
    Defaults,
    Devices,
}

/// The QuickTrumpet capture form (PresetCapture's state).
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureForm {
    pub defaults: bool,
    /// Role indexes 0..4 (Output, Calls output, Microphone, Calls microphone).
    pub roles: Vec<usize>,
    pub device_volumes: bool,
    /// `None` follows the default output until devices are picked.
    pub device_ids: Option<Vec<String>>,
    pub app_volumes: bool,
    pub route_apps: bool,
    pub open: Option<Choose>,
}

impl Default for CaptureForm {
    fn default() -> Self {
        Self {
            defaults: false,
            roles: vec![0, 1, 2, 3],
            device_volumes: true,
            device_ids: None,
            app_volumes: true,
            route_apps: false,
            open: None,
        }
    }
}

/// What the text boxes hold.
#[derive(Default)]
pub struct Texts {
    pub theme: String,
    pub preset: String,
    pub rename: String,
    pub exe: String,
}

impl Texts {
    pub fn get(&self, field: Field) -> &str {
        match field {
            Field::Theme => &self.theme,
            Field::Preset => &self.preset,
            Field::Rename => &self.rename,
            Field::Exe => &self.exe,
        }
    }
    pub fn get_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Theme => &mut self.theme,
            Field::Preset => &mut self.preset,
            Field::Rename => &mut self.rename,
            Field::Exe => &mut self.exe,
        }
    }
}

/// Window state the pages are built from.
#[derive(Default)]
pub struct PageState {
    pub expanded: Option<Expand>,
    pub capture: CaptureForm,
}

/// Where a control's value goes (`apply`).
#[derive(Clone, Debug, PartialEq)]
pub enum Set {
    Bool(Key),
    /// Stored inverted (showWhenPaused vs MediaPopupShowOnlyWhenPlaying).
    NotBool(Key),
    /// This build's "Start with Windows" Run value (D46).
    Startup,
    Int(Key),
    Float(Key),
    /// A 0..100 slider over a 0..1 setting.
    Percent(Key),
    /// UI 1..10 to .02 + (n - 1) * .053 (Q12).
    AnimationSpeed,
    /// A choice whose option `i` stores `values[i]`.
    IntChoice(Key, &'static [i32]),
    /// A choice whose second option is `true`.
    BoolChoice(Key),
    // QuickTrumpet presets, by list index.
    PresetInclude(usize, Include),
    PresetDeviceVolume(usize, String),
    PresetAppVolume(usize, String),
    /// A default-device slot; option `i` is endpoint `ids[i]` ("" leaves
    /// the default alone).
    PresetDefault(usize, Role, Vec<String>),
    /// "+ Add a device": option `i` adds `ids[i]` (the first is the label).
    PresetAddDevice(usize, Vec<String>),
    // The capture form.
    CaptureDefaults,
    CaptureRole(usize),
    CaptureDevices,
    CaptureDevice(String),
    CaptureApps,
    CaptureRoute,
    // App rules, by exe name.
    RuleHardMute(String),
    RuleFocusLost(String),
    RuleMode(String),
    RuleVolume(String),
    /// A folder rule's volume, by id.
    FolderVolume(String),
}

impl Set {
    /// Whether the value lives in VolumeProfilesJson (preset shortcuts
    /// are registered again after it changes).
    pub fn presets(&self) -> bool {
        matches!(
            self,
            Set::PresetInclude(..)
                | Set::PresetDeviceVolume(..)
                | Set::PresetAppVolume(..)
                | Set::PresetDefault(..)
                | Set::PresetAddDevice(..)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    Percent,
    Ms,
    Seconds,
    OutOf10,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    ThemeSelect(String),
    ThemeDelete(String),
    Randomize,
    ResetColors,
    RestoreApp {
        device: String,
        app: String,
        exe: String,
    },
    RestoreAllApps,
    RestoreDevice(String),
    RestoreAllDevices,
    OpenUrl(&'static str),
    /// Saves the theme named in the theme box.
    ThemeSave,
    // QuickTrumpet presets, by list index.
    PresetToggle(Expand),
    PresetApply(usize),
    /// Captures a preset from the form and the name box.
    PresetCapture,
    PresetImport,
    PresetExport(usize),
    PresetUpdate(usize),
    PresetDelete(usize),
    /// Renames to the rename box's text.
    PresetRename(usize),
    PresetDeviceMute(usize, String, bool),
    PresetDeviceRemove(usize, String),
    PresetAppMute(usize, String, bool),
    PresetAppRemove(usize, String),
    CaptureChoose(Choose),
    // App rules.
    RuleToggle(Expand),
    /// Adds the exe typed in the exe box.
    RuleAdd,
    RuleBrowse,
    RuleRemove(String),
    RuleClear,
    // Folder rules, by id.
    FolderToggle(Expand),
    FolderAdd,
    FolderBrowse(String),
    FolderRemove(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub label: String,
    pub glyph: Option<&'static str>,
    pub action: Action,
    pub primary: bool,
    /// Two-step buttons (ConfirmButton) show this until clicked again.
    pub confirm: Option<&'static str>,
}

/// A standard button; `label` may be empty for an icon button.
pub fn button(label: &str, glyph: Option<&'static str>, action: Action) -> Button {
    Button {
        label: label.into(),
        glyph,
        action,
        primary: false,
        confirm: None,
    }
}

impl Button {
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }
    pub fn confirm(mut self, text: &'static str) -> Self {
        self.confirm = Some(text);
        self
    }
}

pub struct ThemeChip {
    pub name: String,
    pub swatches: [Rgba; 4],
    pub active: bool,
    /// A saved custom theme (deletable); `None` for the unsaved "Custom".
    pub custom: Option<bool>,
}

pub enum Control {
    None,
    Toggle(bool, Set),
    /// A checkbox left of the label (the capture form).
    Check(bool, Set),
    /// A small overline heading inside a card.
    Heading,
    Slider {
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        unit: Unit,
        set: Set,
    },
    Choice {
        options: Vec<String>,
        selected: usize,
        set: Set,
    },
    /// A shortcut; `text` is the stored shortcut ("" when unset),
    /// `fallback` what is registered instead.
    Hotkey {
        target: HotkeyTarget,
        text: String,
        fallback: Option<String>,
    },
    /// A colour setting; `set` is false when it falls back.
    Color {
        key: &'static str,
        color: Rgba,
        set: bool,
    },
    Buttons(Vec<Button>),
    /// Read-only text on the right ("20 FPS").
    Chip(String),
    /// Full width: the theme palette chips.
    Themes(Vec<ThemeChip>),
    /// A text box with buttons; Enter runs the primary (or first) one.
    /// Full width without a label, else on the right.
    Input {
        field: Field,
        placeholder: &'static str,
        buttons: Vec<Button>,
    },
}

pub fn row(label: &str, desc: Option<&str>, control: Control) -> Row {
    Row {
        label: label.into(),
        desc: desc.map(Into::into),
        control,
        ..Row::default()
    }
}

pub fn toggle(core: &Core, key: Key, label: &str, desc: Option<&str>) -> Row {
    row(
        label,
        desc,
        Control::Toggle(core.store.bool(key), Set::Bool(key)),
    )
}

pub fn section(title: &str, desc: Option<&str>, rows: Vec<Row>) -> Section {
    Section {
        title: title.into(),
        desc: desc.map(Into::into),
        rows,
    }
}

fn disabled(mut row: Row, disabled: bool) -> Row {
    row.enabled = !disabled;
    row
}

// WinForms Keys modifier flags (HotkeyData).
const SHIFT: i32 = 0x0001_0000;
const CONTROL: i32 = 0x0002_0000;
const ALT: i32 = 0x0004_0000;
const LWIN: i32 = 91;

/// A key's display name (HotkeyData.ToString style), `None` for keys the
/// original cannot bind.
pub fn key_name(vk: i32) -> Option<String> {
    Some(match vk {
        65..=90 => char::from(vk as u8).to_string(),
        48..=57 => (vk - 48).to_string(),
        96..=105 => format!("Num {}", vk - 96),
        112..=135 => format!("F{}", vk - 111),
        8 => "Backspace".into(),
        9 => "Tab".into(),
        13 => "Enter".into(),
        19 => "Pause".into(),
        32 => "Space".into(),
        33 => "PageUp".into(),
        34 => "PageDown".into(),
        35 => "End".into(),
        36 => "Home".into(),
        37 => "Left".into(),
        38 => "Up".into(),
        39 => "Right".into(),
        40 => "Down".into(),
        44 => "PrintScreen".into(),
        45 => "Insert".into(),
        46 => "Delete".into(),
        106 => "Num *".into(),
        107 => "Num +".into(),
        109 => "Num -".into(),
        110 => "Num .".into(),
        111 => "Num /".into(),
        173 => "Mute".into(),
        174 => "Volume Down".into(),
        175 => "Volume Up".into(),
        176 => "Next Track".into(),
        177 => "Previous Track".into(),
        178 => "Stop".into(),
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

/// "Win+Ctrl+Alt+Shift+K", or "" for an empty hotkey.
pub fn hotkey_text(hotkey: &HotkeyData) -> String {
    if hotkey.key == 0 {
        return String::new();
    }
    let mut text = String::new();
    let m = hotkey.modifiers;
    if m & LWIN == LWIN {
        text.push_str("Win+");
    }
    for (flag, name) in [(CONTROL, "Ctrl+"), (ALT, "Alt+"), (SHIFT, "Shift+")] {
        if m & flag == flag {
            text.push_str(name);
        }
    }
    text.push_str(&key_name(hotkey.key).unwrap_or_else(|| format!("Key {}", hotkey.key)));
    text
}

/// A recorded key press as HotkeyData (`win` adds LWin to the modifiers,
/// as the original's recorder does).
pub fn hotkey_from(vk: u16, ctrl: bool, alt: bool, shift: bool, win: bool) -> HotkeyData {
    let mut modifiers = 0;
    if ctrl {
        modifiers |= CONTROL;
    }
    if alt {
        modifiers |= ALT;
    }
    if shift {
        modifiers |= SHIFT;
    }
    if win {
        modifiers |= LWIN;
    }
    HotkeyData {
        modifiers,
        key: i32::from(vk),
    }
}

fn hotkey_row(core: &Core, key: Key, label: &str, desc: &str) -> Row {
    let text = match core.store.get(key) {
        Value::Hotkey(Some(h)) => hotkey_text(&h),
        _ => String::new(),
    };
    // Unset shortcuts this build registers fall back to its defaults.
    let action = match key {
        Key::FlyoutHotkey => Some(hotkeys::Action::ToggleFlyout),
        Key::AbsoluteVolumeUpHotkey => Some(hotkeys::Action::VolumeUp),
        Key::AbsoluteVolumeDownHotkey => Some(hotkeys::Action::VolumeDown),
        _ => None,
    };
    let fallback = action.and_then(|action| {
        hotkeys::bindings(&core.store, &[], &[])
            .into_iter()
            .find(|b| b.action == action && !b.from_settings)
            .map(|b| b.label)
    });
    row(
        label,
        Some(desc),
        Control::Hotkey {
            target: HotkeyTarget::Setting(key),
            text,
            fallback,
        },
    )
}

pub fn note(text: &str) -> Row {
    row(text, None, Control::None)
}

/// The effective colour of a theme channel and whether it is set.
pub fn channel_color(core: &Core, index: usize) -> (Rgba, bool) {
    let key = CHANNEL_KEYS[index].1;
    if let Some(c) = theme::setting_color(&core.store, key) {
        return (c, true);
    }
    let named = theme::active_theme(&core.store).map(|t| {
        let c = t.colors[index];
        Rgba::rgb(c.0, c.1, c.2)
    });
    let default = match index {
        2 => Rgba::rgb(80, 80, 80),
        4 => Rgba::rgb(32, 32, 32),
        5 => Rgba::WHITE,
        _ => Rgba::rgb(0, 120, 215),
    };
    (named.unwrap_or(default), false)
}

fn rgb(c: bt_core::themes::Rgb) -> Rgba {
    Rgba::rgb(c.0, c.1, c.2)
}

pub fn build(page: PageId, core: &mut Core, state: &PageState) -> Vec<Section> {
    match page {
        PageId::General => general(core),
        PageId::Mouse => mouse(core),
        PageId::Shortcuts => shortcuts(core),
        PageId::QuickTrumpet => quicktrumpet(core, state),
        PageId::AppRules => lists::rules(core, state),
        PageId::Media => media(core),
        PageId::Appearance => appearance(core),
        PageId::Performance => performance(core),
        PageId::Updates => updates(core),
        PageId::Privacy => vec![section(
            "Telemetry and announcements",
            Some("What BetterTrumpet may send, and what it may show you."),
            vec![note(
                "This preview sends no data and fetches no announcements, so there is nothing to turn off here.",
            )],
        )],
        PageId::About => about(core),
    }
}

fn general(core: &mut Core) -> Vec<Section> {
    let legacy = core.store.bool(Key::UseLegacyIcon);
    let mut sections = vec![
        section(
            "Startup",
            None,
            vec![row(
                "Run at Windows startup",
                None,
                Control::Toggle(startup::is_enabled(), Set::Startup),
            )],
        ),
        section(
            "Tray Icon",
            Some("System tray icon appearance"),
            vec![
                toggle(core, Key::UseLegacyIcon, "Use legacy icon", None),
                disabled(
                    toggle(
                        core,
                        Key::TrayIconShowsDeviceType,
                        "Show the output device type",
                        Some(if legacy {
                            "Turn off the original icon to show the device type."
                        } else {
                            "Headphones or monitor icon based on the default output (headsets show as headphones). Speakers keep the regular icon; not available with the original icon."
                        }),
                    ),
                    legacy,
                ),
                toggle(
                    core,
                    Key::ShowAppTooltips,
                    "Show icon tooltips",
                    Some("Show or hide tooltips when hovering tray and app icons."),
                ),
            ],
        ),
    ];
    // Hidden apps and devices, by device name.
    let names: Vec<(String, String)> = core
        .audio
        .devices()
        .map(|s| {
            s.devices
                .iter()
                .map(|d| (d.id.to_lowercase(), d.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let device_name = |id: &str| {
        names
            .iter()
            .find(|(d, _)| *d == id.to_lowercase())
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| "Disconnected device".into())
    };
    let store = &mut core.store;
    let compare = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase());
    let apps = with_env(|env| store.hidden_apps.sorted(env, compare));
    let devices = with_env(|env| store.hidden_devices.sorted(env, compare));
    let restore = |action| Control::Buttons(vec![button("Restore", None, action)]);
    let mut rows: Vec<Row> = apps
        .iter()
        .map(|entry| {
            let device = entry.device_id.clone().unwrap_or_default();
            let name = entry
                .display_name
                .clone()
                .filter(|n| !n.is_empty())
                .or_else(|| entry.exe_name.clone())
                .unwrap_or_default();
            Row {
                label: name,
                desc: Some(device_name(&device)),
                control: restore(Action::RestoreApp {
                    device,
                    app: entry.app_id.clone().unwrap_or_default(),
                    exe: entry.exe_name.clone().unwrap_or_default(),
                }),
                ..Row::default()
            }
        })
        .collect();
    if rows.is_empty() {
        rows.push(note("Nothing configured yet."));
    } else {
        rows.push(row(
            "",
            None,
            Control::Buttons(vec![button(
                "Restore hidden app entries",
                None,
                Action::RestoreAllApps,
            )]),
        ));
    }
    sections.push(section(
        "Hidden App Entries",
        Some("Hide noisy app duplicates per device and restore them anytime."),
        rows,
    ));
    if !devices.is_empty() {
        let mut rows: Vec<Row> = devices
            .iter()
            .map(|entry| {
                let id = entry.device_id.clone().unwrap_or_default();
                let name = entry
                    .display_name
                    .clone()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| id.clone());
                row(&name, None, restore(Action::RestoreDevice(id)))
            })
            .collect();
        rows.push(row(
            "",
            None,
            Control::Buttons(vec![button(
                "Restore all devices",
                None,
                Action::RestoreAllDevices,
            )]),
        ));
        sections.push(section("Hidden devices", None, rows));
    }
    sections
}

fn mouse(core: &mut Core) -> Vec<Section> {
    let focus_lost = core.store.bool(Key::UseFocusLostVolume);
    let mut focus_rows = vec![toggle(
        core,
        Key::UseFocusLostVolume,
        "Lower volume of apps that lose focus",
        None,
    )];
    if focus_lost {
        focus_rows.extend([
            row(
                "Background volume (0% mutes)",
                Some(
                    "Locked and keep-muted rules are left alone. The original volume comes back when the app is focused again.",
                ),
                Control::Slider {
                    value: f64::from(core.store.int(Key::FocusLostAttenuatePercent)),
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    unit: Unit::Percent,
                    set: Set::Int(Key::FocusLostAttenuatePercent),
                },
            ),
            row(
                "Fade duration",
                Some(
                    "0 ms is immediate. The same duration is used when restoring the original volume.",
                ),
                Control::Slider {
                    value: f64::from(core.store.int(Key::FocusLostFadeDurationMs)),
                    min: 0.0,
                    max: 5000.0,
                    step: 100.0,
                    unit: Unit::Ms,
                    set: Set::Int(Key::FocusLostFadeDurationMs),
                },
            ),
            row(
                "Applications affected",
                Some(
                    "Use the Focus lost checkbox on an app rule, or the app flyout menu, to select an application.",
                ),
                Control::Choice {
                    options: vec![
                        "All applications".into(),
                        "Only applications selected in App rules".into(),
                    ],
                    selected: usize::from(core.store.bool(Key::FocusLostSelectedAppsOnly)),
                    set: Set::BoolChoice(Key::FocusLostSelectedAppsOnly),
                },
            ),
        ]);
    }
    vec![
        section(
            "Scroll Wheel",
            Some("Control volume with your mouse wheel"),
            vec![
                toggle(
                    core,
                    Key::UseScrollWheelInTray,
                    "Use the scroll wheel to change volume while hovering over the tray icon",
                    Some(
                        "Scroll your mouse wheel over the tray icon to quickly adjust system volume.",
                    ),
                ),
                toggle(
                    core,
                    Key::UseGlobalMouseWheelHook,
                    "Use the scroll wheel to change volume while the flyout is open",
                    Some(
                        "Enables volume control via scroll wheel anywhere on screen when holding a modifier key.",
                    ),
                ),
            ],
        ),
        section(
            "Volume Scale",
            Some("How volume steps are distributed"),
            vec![
                toggle(
                    core,
                    Key::UseLogarithmicVolume,
                    "Use logarithmic volume scale",
                    None,
                ),
                toggle(
                    core,
                    Key::UseVolumeTickSound,
                    "Volume tick sound effect",
                    Some("Play a subtle tick sound when adjusting volume"),
                ),
            ],
        ),
        section(
            "Device change",
            Some("Toast when the default playback device switches"),
            vec![toggle(
                core,
                Key::NotifyOnDefaultDeviceChange,
                "Show a notification when the default device changes",
                None,
            )],
        ),
        section(
            "Focus lost",
            Some("Mute or reduce apps when another window is in front"),
            focus_rows,
        ),
    ]
}

fn shortcuts(core: &mut Core) -> Vec<Section> {
    let hotkeys = [
        (
            Key::FlyoutHotkey,
            "Open Taskbar flyout",
            "Show the volume flyout from anywhere.",
        ),
        (
            Key::MixerHotkey,
            "Open Mixer",
            "Open the full volume mixer.",
        ),
        (
            Key::SettingsHotkey,
            "Open settings",
            "Open this settings window.",
        ),
        (
            Key::AbsoluteVolumeUpHotkey,
            "Increase volume for all devices",
            "Raise the volume of every playback device.",
        ),
        (
            Key::AbsoluteVolumeDownHotkey,
            "Decrease volume for all devices",
            "Lower the volume of every playback device.",
        ),
        (
            Key::SwitchDeviceHotkey,
            "Switch Default Device",
            "Cycle to the next playback device.",
        ),
        (
            Key::QuickTrumpetNextHotkey,
            "Next preset",
            "Applies the next QuickTrumpet preset, wrapping around.",
        ),
        (
            Key::QuickTrumpetPreviousHotkey,
            "Previous preset",
            "Applies the previous QuickTrumpet preset, wrapping around.",
        ),
    ];
    vec![
        section(
            "Global shortcuts",
            None,
            hotkeys
                .iter()
                .map(|(key, label, desc)| hotkey_row(core, *key, label, desc))
                .collect(),
        ),
        section(
            "Device shortcuts",
            Some("Switch the default playback device with one shortcut."),
            lists::device_shortcuts(core)
                .into_iter()
                .chain([toggle(
                    core,
                    Key::ShowDeviceSwitchNotification,
                    "Show a notification when switching devices",
                    Some("Display a toast when a device shortcut switches the default device."),
                )])
                .collect(),
        ),
    ]
}

fn quicktrumpet(core: &mut Core, state: &PageState) -> Vec<Section> {
    let confirmation = core.store.bool(Key::ShowQuickTrumpetConfirmation);
    let mut rows = vec![toggle(
        core,
        Key::ShowQuickTrumpetConfirmation,
        "Show a confirmation when a preset is applied",
        None,
    )];
    if confirmation {
        rows.push(row(
            "Notification duration",
            Some("How long the confirmation stays on screen."),
            Control::Slider {
                value: f64::from(core.store.int(Key::QuickTrumpetNotificationSeconds)),
                min: 1.0,
                max: 10.0,
                step: 1.0,
                unit: Unit::Seconds,
                set: Set::Int(Key::QuickTrumpetNotificationSeconds),
            },
        ));
    }
    vec![
        section(
            "Presets",
            Some(
                "A preset can switch your default output and microphone, set device volumes and set app volumes.",
            ),
            lists::presets(core, state),
        ),
        section(
            "Cycle through presets",
            Some("One shortcut steps through your presets in list order and wraps around."),
            vec![
                hotkey_row(
                    core,
                    Key::QuickTrumpetNextHotkey,
                    "Next preset",
                    "Applies the next QuickTrumpet preset, wrapping around.",
                ),
                hotkey_row(
                    core,
                    Key::QuickTrumpetPreviousHotkey,
                    "Previous preset",
                    "Applies the previous QuickTrumpet preset, wrapping around.",
                ),
            ],
        ),
        section("Confirmation", None, rows),
    ]
}

fn media(core: &mut Core) -> Vec<Section> {
    vec![
        section(
            "Display",
            Some("Show media controls on tray hover"),
            vec![toggle(
                core,
                Key::MediaPopupEnabled,
                "Enable media popup",
                None,
            )],
        ),
        section(
            "Interaction",
            None,
            vec![
                row(
                    "Hover delay",
                    None,
                    Control::Slider {
                        value: core.store.float(Key::MediaPopupHoverDelay),
                        min: 0.5,
                        max: 5.0,
                        step: 0.25,
                        unit: Unit::Seconds,
                        set: Set::Float(Key::MediaPopupHoverDelay),
                    },
                ),
                row(
                    "Show controls when media is paused",
                    None,
                    Control::Toggle(
                        !core.store.bool(Key::MediaPopupShowOnlyWhenPlaying),
                        Set::NotBool(Key::MediaPopupShowOnlyWhenPlaying),
                    ),
                ),
                toggle(
                    core,
                    Key::MediaPopupRememberExpanded,
                    "Remember expanded/collapsed state",
                    None,
                ),
            ],
        ),
    ]
}

fn appearance(core: &mut Core) -> Vec<Section> {
    let active = match core.store.get(Key::ActiveThemeName) {
        Value::Text(Some(name)) => name,
        _ => String::new(),
    };
    let custom_on = core.store.bool(Key::UseCustomSliderColors);
    let saved = theme::custom_themes(&core.store);
    let mut chips: Vec<ThemeChip> = BUILTIN_THEMES
        .iter()
        .map(|t| ThemeChip {
            name: t.name.into(),
            swatches: [
                rgb(t.colors[0]),
                rgb(t.colors[1]),
                rgb(t.colors[3]),
                rgb(t.colors[4]),
            ],
            active: custom_on && t.name == active,
            custom: Some(false),
        })
        .collect();
    chips.extend(saved.iter().map(|t| {
        let name = t.name.clone().unwrap_or_default();
        ThemeChip {
            active: custom_on && name == active,
            name,
            swatches: [
                rgb(t.colors[0]),
                rgb(t.colors[1]),
                rgb(t.colors[3]),
                rgb(t.colors[4]),
            ],
            custom: Some(true),
        }
    }));
    // Custom colours without a matching theme show as "Custom".
    if custom_on && !chips.iter().any(|c| c.active) {
        let c = |i| channel_color(core, i).0;
        chips.push(ThemeChip {
            name: "Custom".into(),
            swatches: [c(0), c(1), c(3), c(4)],
            active: true,
            custom: None,
        });
    }
    let labels = [
        "Thumb color",
        "Track fill color",
        "Track background color",
        "Peak meter color",
        "Window background",
        "Text color",
        "Accent glow",
    ];
    let mut colors = vec![row(
        "Use custom slider colors",
        None,
        Control::Toggle(custom_on, Set::Bool(Key::UseCustomSliderColors)),
    )];
    for (i, label) in labels.iter().enumerate() {
        let (color, set) = channel_color(core, i);
        colors.push(row(
            label,
            None,
            Control::Color {
                key: CHANNEL_KEYS[i].1,
                color,
                set,
            },
        ));
    }
    colors.extend([
        row(
            "Window background opacity",
            None,
            Control::Slider {
                value: (core.store.float(Key::WindowBackgroundOpacity) * 100.0).round(),
                min: 5.0,
                max: 100.0,
                step: 1.0,
                unit: Unit::Percent,
                set: Set::Percent(Key::WindowBackgroundOpacity),
            },
        ),
        row(
            "Peak Meter Style",
            None,
            Control::Choice {
                options: ["Classic", "Dotted", "Blocks", "Bars", "Wave"]
                    .map(String::from)
                    .to_vec(),
                selected: core.store.int(Key::PeakMeterStyle).clamp(0, 4) as usize,
                set: Set::IntChoice(Key::PeakMeterStyle, &[0, 1, 2, 3, 4]),
            },
        ),
        row(
            "",
            None,
            Control::Buttons(vec![
                button("Randomize colors", Some("\u{E8B1}"), Action::Randomize),
                button("Reset to Default", Some("\u{E72C}"), Action::ResetColors),
            ]),
        ),
        row(
            "",
            None,
            Control::Input {
                field: Field::Theme,
                placeholder: "Theme name",
                buttons: vec![button("Save", Some("\u{E74E}"), Action::ThemeSave).primary()],
            },
        ),
    ]);
    vec![
        section(
            "Presets",
            Some("Pick a coordinated palette, then fine-tune it below."),
            vec![row("", None, Control::Themes(chips))],
        ),
        section(
            "Custom Colors",
            Some("The selected preset lands here; tweak any channel to make it yours."),
            colors,
        ),
        section(
            "Dynamic Album Art Theme",
            Some("Colors automatically adapt to the album art of currently playing music"),
            vec![toggle(
                core,
                Key::UseDynamicAlbumArtTheme,
                "Enable dynamic album art colors",
                None,
            )],
        ),
    ]
}

/// VolumeAnimationSpeed as the 1..10 UI value (Q12).
fn speed_step(speed: f64) -> f64 {
    (((speed - 0.02) / 0.053).round() + 1.0).clamp(1.0, 10.0)
}

fn performance(core: &mut Core) -> Vec<Section> {
    let smooth = core.store.bool(Key::UseSmoothVolumeAnimation);
    let eco = core
        .store
        .settings
        .effective_eco_mode(crate::store::context());
    let fps = core
        .store
        .settings
        .effective_peak_fps(crate::store::context());
    const RATES: &[i32] = &[5, 20, 30, 60];
    let current = core.store.int(Key::PeakMeterFps);
    vec![
        section(
            "Eco Mode",
            Some("Lower refresh rates to save CPU/GPU, always or only on battery"),
            vec![
                toggle(core, Key::EcoMode, "Always use Eco Mode (20 FPS)", None),
                toggle(core, Key::AutoEcoMode, "Auto-enable when on battery", None),
            ],
        ),
        section(
            "Animations",
            None,
            vec![
                toggle(
                    core,
                    Key::UseSmoothVolumeAnimation,
                    "Smooth volume animation",
                    None,
                ),
                disabled(
                    row(
                        "Animation speed",
                        (!smooth).then_some("Requires smooth volume animation"),
                        Control::Slider {
                            value: speed_step(core.store.float(Key::VolumeAnimationSpeed)),
                            min: 1.0,
                            max: 10.0,
                            step: 1.0,
                            unit: Unit::OutOf10,
                            set: Set::AnimationSpeed,
                        },
                    ),
                    !smooth,
                ),
            ],
        ),
        section(
            "Peak Meter",
            None,
            vec![
                disabled(
                    row(
                        "Refresh rate",
                        eco.then_some("Overridden by eco mode"),
                        Control::Choice {
                            options: RATES.iter().map(|r| format!("{r} FPS")).collect(),
                            selected: RATES.iter().position(|r| *r == current).unwrap_or(2),
                            set: Set::IntChoice(Key::PeakMeterFps, RATES),
                        },
                    ),
                    eco,
                ),
                row(
                    "Effective rate",
                    Some("Actual rendering rate right now."),
                    Control::Chip(if eco {
                        format!("{fps} FPS · Eco")
                    } else {
                        format!("{fps} FPS")
                    }),
                ),
            ],
        ),
    ]
}

fn updates(core: &mut Core) -> Vec<Section> {
    let auto = core.store.bool(Key::AutoCheckForUpdates);
    vec![section(
        "Update checks",
        Some("New version notifications"),
        vec![
            toggle(
                core,
                Key::AutoCheckForUpdates,
                "Check for updates automatically",
                Some("Saved for the original app; this preview never checks for updates itself."),
            ),
            disabled(
                row(
                    "Notify for",
                    (!auto).then_some("Requires automatic checks"),
                    Control::Choice {
                        options: [
                            "All updates",
                            "Minor and major",
                            "Major only",
                            "No update notifications",
                        ]
                        .map(String::from)
                        .to_vec(),
                        selected: core.store.int(Key::UpdateNotifyChannel).clamp(0, 3) as usize,
                        set: Set::IntChoice(Key::UpdateNotifyChannel, &[0, 1, 2, 3]),
                    },
                ),
                !auto,
            ),
        ],
    )]
}

fn about(core: &mut Core) -> Vec<Section> {
    let mut sections = vec![section(
        &format!("BetterTrumpet {}", env!("CARGO_PKG_VERSION")),
        Some("Rust preview. © 2026 xmn. Based on EarTrumpet; MIT License."),
        vec![row(
            "",
            None,
            Control::Buttons(vec![button(
                "GitHub Repository",
                None,
                Action::OpenUrl("https://github.com/xammen/BetterTrumpet"),
            )]),
        )],
    )];
    if core.store.bool(Key::MonkeyTickSoundUnlocked) {
        sections.push(section(
            "Alternate volume sound",
            None,
            vec![toggle(
                core,
                Key::UseMonkeyTickSound,
                "Use the monkey sound instead of the tick",
                Some("Keep this secret sound enabled for volume adjustments"),
            )],
        ));
    }
    sections
}

/// Writes a control's new value; `value` is the toggle state (0/1), the
/// slider value or the chosen option index.
pub fn apply(core: &mut Core, state: &mut PageState, set: &Set, value: f64) {
    let store = &mut core.store;
    match set.clone() {
        Set::Bool(key) => store.set(key, Value::Bool(value != 0.0)),
        Set::NotBool(key) => store.set(key, Value::Bool(value == 0.0)),
        Set::Startup => {
            if let Ok(exe) = std::env::current_exe() {
                let _ = startup::set_enabled(value != 0.0, &exe);
            }
        }
        Set::Int(key) => store.set(key, Value::Int(value.round() as i32)),
        Set::Float(key) => store.set(key, Value::Float(value)),
        Set::Percent(key) => store.set(key, Value::Float(value.round() / 100.0)),
        Set::AnimationSpeed => {
            let n = value.round().clamp(1.0, 10.0);
            store.set(
                Key::VolumeAnimationSpeed,
                Value::Float(0.02 + (n - 1.0) * 0.053),
            );
        }
        Set::IntChoice(key, values) => {
            if let Some(v) = values.get(value as usize) {
                store.set(key, Value::Int(*v));
            }
        }
        Set::BoolChoice(key) => store.set(key, Value::Bool(value != 0.0)),
        other => lists::apply(core, state, other, value),
    }
}

/// Restores hidden entries (General page).
pub fn restore(core: &mut Core, action: &Action) {
    let store = &mut core.store;
    match action {
        Action::RestoreApp { device, app, exe } => with_env(|env| {
            store.hidden_apps.unhide(
                AppKey {
                    device_id: Some(device),
                    app_id: Some(app),
                    exe_name: Some(exe),
                },
                env,
            )
        }),
        Action::RestoreAllApps => with_env(|env| store.hidden_apps.clear(env)),
        Action::RestoreDevice(id) => with_env(|env| store.hidden_devices.unhide(Some(id), env)),
        Action::RestoreAllDevices => with_env(|env| store.hidden_devices.clear(env)),
        _ => return,
    }
    store.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_speed_round_trips_through_the_ui_scale() {
        for n in 1..=10 {
            let speed = 0.02 + f64::from(n - 1) * 0.053;
            assert_eq!(speed_step(speed), f64::from(n));
        }
        // The default (0.08) sits between 2 and 3 and shows as 2.
        assert_eq!(speed_step(0.08), 2.0);
    }

    #[test]
    fn hotkey_text_and_recording_match_the_original_format() {
        let h = hotkey_from(77, true, false, true, false);
        assert_eq!(h.modifiers, CONTROL | SHIFT);
        assert_eq!(hotkey_text(&h), "Ctrl+Shift+M");
        let w = hotkey_from(112, false, true, false, true);
        assert_eq!(hotkey_text(&w), "Win+Alt+F1");
        assert_eq!(hotkey_text(&HotkeyData::default()), "");
    }
}
