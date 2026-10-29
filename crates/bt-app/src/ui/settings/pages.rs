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
use bt_app::i18n::tr;
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

/// The sidebar groups (App.tsx groupPages), titles as resource keys.
pub const GROUPS: [(&str, &[PageId]); 4] = [
    (
        "WebSettingsCategoryEssentials",
        &[PageId::General, PageId::Mouse, PageId::Shortcuts],
    ),
    (
        "WebSettingsCategoryAudio",
        &[PageId::QuickTrumpet, PageId::AppRules, PageId::Media],
    ),
    (
        "WebSettingsCategoryExperience",
        &[PageId::Appearance, PageId::Performance],
    ),
    (
        "AppCategoryTitle",
        &[PageId::Updates, PageId::Privacy, PageId::About],
    ),
];

impl PageId {
    pub fn all() -> impl Iterator<Item = PageId> {
        GROUPS.iter().flat_map(|(_, pages)| pages.iter().copied())
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::General => tr("GeneralSettingsPageText"),
            Self::Mouse => tr("VolumeMouseSettingsPageText"),
            Self::Shortcuts => tr("ShortcutsPageText"),
            Self::QuickTrumpet => tr("QuickTrumpetSettingsPageText"),
            Self::AppRules => tr("AppRulesSettingsPageText"),
            Self::Media => tr("MediaPopupSettingsPageText"),
            Self::Appearance => tr("AppearanceSettingsPageText"),
            Self::Performance => tr("PerformanceSettingsPageText"),
            Self::Updates => tr("UpdatesSettingsPageText"),
            Self::Privacy => tr("PrivacySettingsPageText"),
            Self::About => tr("AboutTitle"),
        }
    }
    pub fn subtitle(self) -> &'static str {
        match self {
            Self::General => tr("GeneralSettingsPageSubtitle"),
            Self::Mouse => tr("VolumeMouseSettingsPageSubtitle"),
            Self::Shortcuts => tr("ShortcutsPageSubtitle"),
            Self::QuickTrumpet => tr("QuickTrumpetSettingsPageSubtitle"),
            Self::AppRules => tr("AppRulesSettingsPageSubtitle"),
            Self::Media => tr("MediaPopupSettingsPageSubtitle"),
            Self::Appearance => tr("AppearanceSettingsPageSubtitle"),
            Self::Performance => tr("PerformanceSettingsPageSubtitle"),
            Self::Updates => tr("UpdatesSettingsPageSubtitle"),
            Self::Privacy => tr("WebSettingsPrivacyPageSubtitle"),
            Self::About => tr("SettingsAboutSubtitle"),
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
    /// Settings > About's "Export diagnostic report" (local files only).
    ExportDiagnostics,
    /// Settings > About's Export & Import buttons (.btsettings).
    SettingsExport,
    SettingsImport,
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
        36 => tr("HomeButtonText").into(),
        37 => "Left".into(),
        38 => "Up".into(),
        39 => "Right".into(),
        40 => "Down".into(),
        44 => "PrintScreen".into(),
        45 => "Insert".into(),
        46 => tr("SettingsProfileDelete").into(),
        106 => "Num *".into(),
        107 => "Num +".into(),
        109 => "Num -".into(),
        110 => "Num .".into(),
        111 => "Num /".into(),
        173 => tr("MuteKind_Mute").into(),
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
            tr("WebSettingsPrivacySection"),
            Some(tr("WebSettingsPrivacySectionDesc")),
            vec![note(tr("PrivacyPreviewNote"))],
        )],
        PageId::About => about(core),
    }
}

fn general(core: &mut Core) -> Vec<Section> {
    let legacy = core.store.bool(Key::UseLegacyIcon);
    let mut sections = vec![
        section(
            tr("SettingsStartup"),
            None,
            vec![row(
                tr("SettingsRunAtStartup"),
                None,
                Control::Toggle(startup::is_enabled(), Set::Startup),
            )],
        ),
        section(
            tr("SettingsTrayIcon"),
            Some(tr("SettingsTrayIconDesc")),
            vec![
                toggle(
                    core,
                    Key::UseLegacyIcon,
                    tr("SettingsUseLegacyEarTrumpetIcon"),
                    None,
                ),
                disabled(
                    toggle(
                        core,
                        Key::TrayIconShowsDeviceType,
                        tr("SettingsTrayIconDeviceType"),
                        Some(if legacy {
                            tr("WebSettingsRequiresModernIcon")
                        } else {
                            tr("SettingsTrayIconDeviceTypeDesc")
                        }),
                    ),
                    legacy,
                ),
                toggle(
                    core,
                    Key::ShowAppTooltips,
                    tr("SettingsShowAppTooltips"),
                    Some(tr("SettingsAppTooltipsDesc")),
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
            .unwrap_or_else(|| tr("DisconnectedDeviceText").into())
    };
    let store = &mut core.store;
    let compare = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase());
    let apps = with_env(|env| store.hidden_apps.sorted(env, compare));
    let devices = with_env(|env| store.hidden_devices.sorted(env, compare));
    let restore =
        |action| Control::Buttons(vec![button(tr("WebSettingsRestoreButton"), None, action)]);
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
        rows.push(note(tr("WebSettingsEmpty")));
    } else {
        rows.push(row(
            "",
            None,
            Control::Buttons(vec![button(
                tr("SettingsRestoreHiddenApps"),
                None,
                Action::RestoreAllApps,
            )]),
        ));
    }
    sections.push(section(
        tr("SettingsHiddenAppsTitle"),
        Some(tr("SettingsHiddenAppsDesc")),
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
                tr("WebSettingsRestoreAllDevices"),
                None,
                Action::RestoreAllDevices,
            )]),
        ));
        sections.push(section(tr("WebSettingsHiddenDevices"), None, rows));
    }
    sections
}

fn mouse(core: &mut Core) -> Vec<Section> {
    let focus_lost = core.store.bool(Key::UseFocusLostVolume);
    let mut focus_rows = vec![toggle(
        core,
        Key::UseFocusLostVolume,
        tr("SettingsUseFocusLostVolume"),
        None,
    )];
    if focus_lost {
        focus_rows.extend([
            row(
                tr("SettingsFocusLostAttenuate"),
                Some(tr("SettingsFocusLostAttenuateHint")),
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
                tr("SettingsFocusLostFade"),
                Some(tr("SettingsFocusLostFadeHint")),
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
                tr("SettingsFocusLostScope"),
                Some(tr("SettingsFocusLostSelectedHint")),
                Control::Choice {
                    options: vec![
                        tr("SettingsFocusLostAllApps").into(),
                        tr("SettingsFocusLostSelectedApps").into(),
                    ],
                    selected: usize::from(core.store.bool(Key::FocusLostSelectedAppsOnly)),
                    set: Set::BoolChoice(Key::FocusLostSelectedAppsOnly),
                },
            ),
        ]);
    }
    vec![
        section(
            tr("SettingsScrollWheel"),
            Some(tr("SettingsScrollWheelDesc")),
            vec![
                toggle(
                    core,
                    Key::UseScrollWheelInTray,
                    tr("SettingsUseScrollWheelInTray"),
                    Some(tr("SettingsScrollWheelTrayTip")),
                ),
                toggle(
                    core,
                    Key::UseGlobalMouseWheelHook,
                    tr("SettingsUseGlobalMouseWheelHook"),
                    Some(tr("SettingsScrollWheelGlobalTip")),
                ),
            ],
        ),
        section(
            tr("SettingsVolumeScale"),
            Some(tr("SettingsVolumeScaleDesc")),
            vec![
                toggle(
                    core,
                    Key::UseLogarithmicVolume,
                    tr("SettingsUseLogarithmicVolume"),
                    None,
                ),
                toggle(
                    core,
                    Key::UseVolumeTickSound,
                    tr("SettingsVolumeTickSound"),
                    Some(tr("SettingsVolumeTickSoundTip")),
                ),
            ],
        ),
        section(
            tr("SettingsDeviceChangeNotify"),
            Some(tr("SettingsDeviceChangeNotifyDesc")),
            vec![toggle(
                core,
                Key::NotifyOnDefaultDeviceChange,
                tr("SettingsNotifyOnDeviceChange"),
                None,
            )],
        ),
        section(
            tr("SettingsFocusLostVolume"),
            Some(tr("SettingsFocusLostVolumeDesc")),
            focus_rows,
        ),
    ]
}

fn shortcuts(core: &mut Core) -> Vec<Section> {
    let hotkeys = [
        (
            Key::FlyoutHotkey,
            tr("SettingsOpenEarTrumpetText"),
            tr("WebSettingsHotkeyFlyoutDesc"),
        ),
        (
            Key::MixerHotkey,
            tr("SettingsOpenMixerText"),
            tr("WebSettingsHotkeyMixerDesc"),
        ),
        (
            Key::SettingsHotkey,
            tr("SettingsOpenSettingsText"),
            tr("WebSettingsHotkeySettingsDesc"),
        ),
        (
            Key::AbsoluteVolumeUpHotkey,
            tr("SettingsAbsoluteVolumeUpText"),
            tr("WebSettingsHotkeyVolumeUpDesc"),
        ),
        (
            Key::AbsoluteVolumeDownHotkey,
            tr("SettingsAbsoluteVolumeDownText"),
            tr("WebSettingsHotkeyVolumeDownDesc"),
        ),
        (
            Key::SwitchDeviceHotkey,
            tr("SettingsSwitchDevice"),
            tr("WebSettingsHotkeySwitchDeviceDesc"),
        ),
        (
            Key::QuickTrumpetNextHotkey,
            tr("SettingsQuickTrumpetNextHotkey"),
            tr("WebSettingsHotkeyNextPresetDesc"),
        ),
        (
            Key::QuickTrumpetPreviousHotkey,
            tr("SettingsQuickTrumpetPreviousHotkey"),
            tr("WebSettingsHotkeyPreviousPresetDesc"),
        ),
    ];
    vec![
        section(
            tr("WebSettingsShortcutsGlobal"),
            None,
            hotkeys
                .iter()
                .map(|(key, label, desc)| hotkey_row(core, *key, label, desc))
                .collect(),
        ),
        section(
            tr("WebSettingsDeviceShortcuts"),
            Some(tr("WebSettingsDeviceShortcutsDesc")),
            lists::device_shortcuts(core)
                .into_iter()
                .chain([toggle(
                    core,
                    Key::ShowDeviceSwitchNotification,
                    tr("WebSettingsShowDeviceSwitchNotification"),
                    Some(tr("WebSettingsShowDeviceSwitchNotificationDesc")),
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
        tr("SettingsQuickTrumpetConfirmation"),
        None,
    )];
    if confirmation {
        rows.push(row(
            tr("WebSettingsQtNotificationDuration"),
            Some(tr("WebSettingsQtNotificationDurationDesc")),
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
            tr("SettingsTabPresets"),
            Some(tr("WebSettingsQtPresetsDesc")),
            lists::presets(core, state),
        ),
        section(
            tr("WebSettingsQtCycleTitle"),
            Some(tr("WebSettingsQtCycleDesc")),
            vec![
                hotkey_row(
                    core,
                    Key::QuickTrumpetNextHotkey,
                    tr("SettingsQuickTrumpetNextHotkey"),
                    tr("WebSettingsHotkeyNextPresetDesc"),
                ),
                hotkey_row(
                    core,
                    Key::QuickTrumpetPreviousHotkey,
                    tr("SettingsQuickTrumpetPreviousHotkey"),
                    tr("WebSettingsHotkeyPreviousPresetDesc"),
                ),
            ],
        ),
        section(tr("WebSettingsQtNotificationTitle"), None, rows),
    ]
}

fn media(core: &mut Core) -> Vec<Section> {
    vec![
        section(
            tr("WebSettingsMediaPopupSection"),
            Some(tr("SettingsMediaPopupDesc")),
            vec![toggle(
                core,
                Key::MediaPopupEnabled,
                tr("SettingsEnableMediaPopup"),
                None,
            )],
        ),
        section(
            tr("SettingsInteraction"),
            None,
            vec![
                row(
                    tr("SettingsHoverDelay"),
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
                    tr("SettingsShowWhenPaused"),
                    None,
                    Control::Toggle(
                        !core.store.bool(Key::MediaPopupShowOnlyWhenPlaying),
                        Set::NotBool(Key::MediaPopupShowOnlyWhenPlaying),
                    ),
                ),
                toggle(
                    core,
                    Key::MediaPopupRememberExpanded,
                    tr("SettingsRememberExpanded"),
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
            name: tr("WebSettingsCustomTheme").into(),
            swatches: [c(0), c(1), c(3), c(4)],
            active: true,
            custom: None,
        });
    }
    let labels = [
        tr("SettingsSliderThumbColor"),
        tr("SettingsSliderTrackFillColor"),
        tr("SettingsSliderTrackBackgroundColor"),
        tr("SettingsPeakMeterColor"),
        tr("SettingsWindowBgColor"),
        tr("SettingsTextColor"),
        tr("SettingsAccentGlowColor"),
    ];
    let mut colors = vec![row(
        tr("SettingsUseCustomSliderColors"),
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
            tr("SettingsWindowOpacity"),
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
            tr("SettingsPeakMeterStyle"),
            None,
            Control::Choice {
                options: [
                    tr("WebSettingsPeakStyleClassic"),
                    tr("WebSettingsPeakStyleDotted"),
                    tr("WebSettingsPeakStyleBlocks"),
                    tr("WebSettingsPeakStyleBars"),
                    tr("WebSettingsPeakStyleWave"),
                ]
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
                button(
                    tr("SettingsRandomizeTooltip"),
                    Some("\u{E8B1}"),
                    Action::Randomize,
                ),
                button(
                    tr("SettingsResetToDefault"),
                    Some("\u{E72C}"),
                    Action::ResetColors,
                ),
            ]),
        ),
        row(
            "",
            None,
            Control::Input {
                field: Field::Theme,
                placeholder: tr("WebSettingsThemeName"),
                buttons: vec![
                    button(tr("WebSettingsSave"), Some("\u{E74E}"), Action::ThemeSave).primary(),
                ],
            },
        ),
    ]);
    vec![
        section(
            tr("SettingsTabPresets"),
            Some(tr("WebSettingsAppearanceDesc")),
            vec![row("", None, Control::Themes(chips))],
        ),
        section(
            tr("SettingsCustomColors"),
            Some(tr("WebSettingsCustomColorsTweakHint")),
            colors,
        ),
        section(
            tr("SettingsDynamicAlbumArt"),
            Some(tr("SettingsDynamicAlbumArtDesc")),
            vec![toggle(
                core,
                Key::UseDynamicAlbumArtTheme,
                tr("SettingsEnableDynamicAlbumArt"),
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
    let legacy = core.store.bool(Key::UseLegacyIcon);
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
            tr("SettingsEcoMode"),
            Some(tr("SettingsEcoModeDesc")),
            vec![
                toggle(core, Key::EcoMode, tr("SettingsEcoModeEnable"), None),
                toggle(core, Key::AutoEcoMode, tr("SettingsAutoEcoMode"), None),
            ],
        ),
        section(
            tr("SettingsAnimations"),
            None,
            vec![
                toggle(
                    core,
                    Key::UseSmoothVolumeAnimation,
                    tr("SettingsSmoothVolumeAnimation"),
                    None,
                ),
                disabled(
                    row(
                        tr("SettingsAnimationSpeedLabel"),
                        (!smooth).then_some(tr("WebSettingsRequiresSmoothAnimation")),
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
                // Port-only (D51); Eco Mode keeps the icon still too.
                disabled(
                    toggle(
                        core,
                        Key::AnimateTrayIcon,
                        tr("SettingsAnimateTrayIcon"),
                        Some(if legacy {
                            tr("WebSettingsRequiresModernIcon")
                        } else if eco {
                            tr("WebSettingsDisabledByEco")
                        } else {
                            tr("SettingsAnimateTrayIconDesc")
                        }),
                    ),
                    legacy || eco,
                ),
            ],
        ),
        section(
            tr("SettingsPeakMeter"),
            None,
            vec![
                disabled(
                    row(
                        tr("SettingsRefreshRate"),
                        eco.then_some(tr("WebSettingsDisabledByEco")),
                        Control::Choice {
                            options: RATES.iter().map(|r| format!("{r} FPS")).collect(),
                            selected: RATES.iter().position(|r| *r == current).unwrap_or(2),
                            set: Set::IntChoice(Key::PeakMeterFps, RATES),
                        },
                    ),
                    eco,
                ),
                row(
                    tr("WebSettingsEffectiveRate"),
                    Some(tr("WebSettingsEffectiveRateHint")),
                    Control::Chip(if eco {
                        format!("{fps} FPS · {}", tr("WebSettingsEcoActive"))
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
        tr("WebSettingsUpdatesSection"),
        Some(tr("SettingsUpdatesDesc")),
        vec![
            toggle(
                core,
                Key::AutoCheckForUpdates,
                tr("AutoUpdateCheckboxText"),
                Some(tr("UpdatesPreviewNote")),
            ),
            disabled(
                row(
                    tr("SettingsNotifyFor"),
                    (!auto).then_some(tr("WebSettingsRequiresAutoCheck")),
                    Control::Choice {
                        options: [
                            tr("SettingsUpdateChannelAllDesc"),
                            tr("SettingsUpdateChannelMinorMajorDesc"),
                            tr("SettingsUpdateChannelMajorOnlyDesc"),
                            tr("SettingsUpdateChannelNoneDesc"),
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
        Some(tr("AboutPreviewCredits")),
        vec![row(
            "",
            None,
            Control::Buttons(vec![
                button(
                    tr("AboutGitHub"),
                    None,
                    Action::OpenUrl("https://github.com/xammen/BetterTrumpet"),
                ),
                // The About page's TroubleshootEarTrumpetText link.
                button(
                    tr("TroubleshootEarTrumpetText"),
                    None,
                    Action::ExportDiagnostics,
                ),
            ]),
        )],
    )];
    sections.push(section(
        tr("SettingsExportImport"),
        Some(tr("SettingsExportImportDesc")),
        vec![row(
            "",
            None,
            Control::Buttons(vec![
                button(tr("SettingsExportSettings"), None, Action::SettingsExport),
                button(tr("SettingsImportSettings"), None, Action::SettingsImport),
            ]),
        )],
    ));
    if core.store.bool(Key::MonkeyTickSoundUnlocked) {
        sections.push(section(
            tr("AlternateVolumeSoundTitle"),
            None,
            vec![toggle(
                core,
                Key::UseMonkeyTickSound,
                tr("SettingsUseMonkeyTickSound"),
                Some(tr("SettingsUseMonkeyTickSoundTip")),
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
    fn group_titles_are_resource_keys() {
        for (key, _) in GROUPS {
            assert!(bt_app::i18n::KEYS.contains(&key), "{key}");
        }
    }

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
