//! What the flyout and the mixer window share, after the original's
//! DeviceView / AppItemView and the focused app and device popups: row
//! sizes and geometry, the device and app rows, the actions behind their
//! buttons, the peak meters, and the popups' rule tools and menus.
use super::{slider::Slider, theme::Theme};
use crate::core::{AppView, Core, DeviceView, HiddenEntry, RuleKind, View};
use bt_app::i18n::{tr, trf};
use bt_app::win32::{
    gfx::{Align, Canvas, Image, Rect, TextStyle},
    icons,
    menu::Item,
    sound,
};
use bt_core::models::VolumeRuleMode;
use std::{
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

/// A device block's title row (Mutable_DeviceTitleCellHeight).
pub const TITLE_H: f32 = 48.0;
pub const DEVICE_ROW_H: f32 = 60.0;
pub const APP_ROW_H: f32 = 52.0;
/// VolumeAppListMargin above and below the app list.
pub const APPS_PAD: f32 = 6.0;
pub const UI_FONT: &str = "Segoe UI";
/// WindowBlurRadius: the blur of the window behind a popup.
pub const BLUR_RADIUS: f32 = 10.0;
const TICK_WAV: &[u8] = include_bytes!("../../assets/tick.wav");

// Segoe MDL2 / Fluent glyphs.
pub const GLYPH_MUTE: char = '\u{E74F}';
pub const GLYPH_BARS: [char; 4] = ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'];
pub const GLYPH_SYSTEM_SOUNDS: char = '\u{E7F5}';
pub const GLYPH_LOCK: char = '\u{E72E}';
pub const GLYPH_LAUNCH: char = '\u{E768}';
pub const GLYPH_FOCUS: char = '\u{E7F4}';
pub const GLYPH_HIDE: char = '\u{E18B}';
pub const GLYPH_ROUTE: char = '\u{E8AB}';
pub const GLYPH_MORE: char = '\u{E10C}';
pub const GLYPH_CLOSE: char = '\u{E8BB}';

/// Which slider: a device, an app, or one process of an app; `popup` for
/// the copies shown in the popups.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SliderKey {
    pub device: String,
    pub app: Option<String>,
    pub pid: Option<u32>,
    pub popup: bool,
}

impl SliderKey {
    pub fn device(device: &str, popup: bool) -> Self {
        Self {
            device: device.into(),
            app: None,
            pid: None,
            popup,
        }
    }
    pub fn app(device: &str, key: &str, pid: Option<u32>, popup: bool) -> Self {
        Self {
            device: device.into(),
            app: Some(key.into()),
            pid,
            popup,
        }
    }
    /// The other copy of this slider (list or popup).
    pub fn twin(&self) -> Self {
        Self {
            popup: !self.popup,
            ..self.clone()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToolAction {
    Lock,
    Launch,
    Focus,
    HardMute,
    Hide,
    Route,
    More,
    Close,
}

/// A button in a popup's header.
pub struct Tool {
    pub glyph: char,
    pub action: ToolAction,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PopupKind {
    App { device: String, key: String },
    Device { device: String },
}

/// Commands chosen from the flyout's and the mixer's menus.
#[derive(Clone, Debug)]
pub enum Command {
    Rule(String, String, RuleKind),
    Route(String, Option<String>),
    SetDefault(String),
    HideDevice(String, String),
    Unhide(String, HiddenEntry),
    UnhideAll(String),
    Nothing,
}

/// What a popup tool asks for.
pub enum ToolStep {
    Close,
    /// Hide this app (device, key), with its animation.
    Hide(String, String),
    /// Lock or unlock at once (device, app, rule).
    Rule(String, Box<AppView>, RuleKind),
    /// A menu below the button.
    Menu(Vec<Item>, Vec<Command>),
}

pub fn percent(volume: f32) -> i32 {
    (volume * 100.0).round() as i32
}

pub fn exe_file(app: &AppView) -> &str {
    let path = app.app.executable.as_deref().unwrap_or("");
    match path.rsplit(['\\', '/']).next() {
        Some(name) if !name.is_empty() => name,
        _ => &app.app.display_name,
    }
}

/// Apps by executable name, ordinal (AppItemViewModel's comparer); System
/// sounds last.
pub fn sorted_apps(device: &DeviceView) -> Vec<&AppView> {
    let mut apps: Vec<&AppView> = device.apps.iter().filter(|a| !a.hidden).collect();
    apps.sort_by(|a, b| {
        a.app
            .is_system_sounds
            .cmp(&b.app.is_system_sounds)
            .then_with(|| exe_file(a).cmp(exe_file(b)))
    });
    apps
}

pub fn glyph(c: char) -> String {
    c.to_string()
}

fn first_letter(name: &str) -> String {
    name.trim()
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or("?".into(), |c| c.to_uppercase().collect())
}

// ---------- row geometry ----------

pub fn mute_rect(row: Rect) -> Rect {
    Rect::new(row.x + 24.5, row.y + row.h / 2.0 - 14.0, 28.0, 28.0)
}
pub fn slider_rect(row: Rect) -> Rect {
    Rect::new(
        row.x + 72.0,
        row.y + row.h / 2.0 - 16.0,
        row.w - 140.0,
        32.0,
    )
}
pub fn value_rect(row: Rect) -> Rect {
    Rect::new(row.right() - 69.0, row.y, 69.0, row.h)
}

/// What part of a row a point is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowPart {
    Mute,
    Slider,
    Row,
}

pub fn row_part(row: Rect, x: f32, y: f32) -> Option<RowPart> {
    if !row.contains(x, y) {
        None
    } else if mute_rect(row).contains(x, y) {
        Some(RowPart::Mute)
    } else if slider_rect(row).contains(x, y) {
        Some(RowPart::Slider)
    } else {
        Some(RowPart::Row)
    }
}

// ---------- state from the view ----------

pub fn volume_of(view: &View, key: &SliderKey) -> Option<(f32, bool)> {
    let device = view.device(&key.device)?;
    match (&key.app, key.pid) {
        (None, _) => Some((device.device.volume, device.device.muted)),
        (Some(app), None) => {
            let app = device.apps.iter().find(|a| &a.app.key == app)?;
            Some((app.app.volume, app.app.muted))
        }
        (Some(app), Some(pid)) => {
            let app = device.apps.iter().find(|a| &a.app.key == app)?;
            let s = app.app.sessions.iter().find(|s| s.process_id == pid)?;
            Some((s.volume, s.muted))
        }
    }
}

pub fn locked(view: &View, key: &SliderKey) -> bool {
    key.app
        .as_ref()
        .and_then(|app| view.app(&key.device, app))
        .is_some_and(AppView::locked)
}

/// The popup's slider rows: one per process of an app (or the app itself),
/// or the device.
pub fn popup_keys(view: &View, kind: &PopupKind) -> Vec<SliderKey> {
    match kind {
        PopupKind::App { device, key } => {
            let Some(app) = view.app(device, key) else {
                return vec![];
            };
            if app.app.sessions.is_empty() {
                vec![SliderKey::app(device, key, None, true)]
            } else {
                app.app
                    .sessions
                    .iter()
                    .map(|s| SliderKey::app(device, key, Some(s.process_id), true))
                    .collect()
            }
        }
        PopupKind::Device { device } => vec![SliderKey::device(device, true)],
    }
}

/// The popup's app or device went away.
pub fn popup_gone(view: &View, kind: &PopupKind) -> bool {
    match kind {
        PopupKind::App { device, key } => view.app(device, key).is_none(),
        PopupKind::Device { device } => view.device(device).is_none(),
    }
}

/// Keeps the sliders for `keys`, with Windows' values.
pub fn sync_sliders(view: &View, sliders: &mut HashMap<SliderKey, Slider>, keys: Vec<SliderKey>) {
    let now = Instant::now();
    sliders.retain(|k, _| keys.contains(k));
    for key in keys {
        if let Some((volume, _)) = volume_of(view, &key) {
            sliders
                .entry(key)
                .or_insert_with(|| Slider::new(percent(volume)))
                .update(percent(volume), now);
        }
    }
}

// ---------- actions ----------

/// The volume tick (UseVolumeTickSound), at most every 50 ms.
pub struct Ticker(Instant);

impl Ticker {
    pub fn new() -> Self {
        Self(Instant::now())
    }
    pub fn play(&mut self, theme: &Theme, value: i32) {
        let now = Instant::now();
        if !theme.tick_sound || now.duration_since(self.0) < Duration::from_millis(50) {
            return;
        }
        self.0 = now;
        sound::play(TICK_WAV, value as f32 / 100.0);
    }
}

/// A slider value chosen by the user; the view follows at once and the
/// other copy of the slider takes the value.
pub fn apply_value(
    view: &mut View,
    sliders: &mut HashMap<SliderKey, Slider>,
    core: &mut Core,
    key: &SliderKey,
    value: i32,
) {
    let volume = value as f32 / 100.0;
    match (&key.app, key.pid) {
        (None, _) => {
            let Some(device) = view.device_mut(&key.device) else {
                return;
            };
            let previous = percent(device.device.volume);
            device.device.volume = volume;
            let name = device.device.name.clone();
            core.set_device_volume(&key.device, &name, volume, previous);
        }
        (Some(app_key), None) => {
            let Some(app) = view.app_mut(&key.device, app_key) else {
                return;
            };
            let previous = percent(app.app.volume);
            app.app.volume = volume;
            let name = app.app.display_name.clone();
            core.set_app_volume(&key.device, app_key, &name, volume, previous);
        }
        (Some(app_key), Some(pid)) => {
            if let Some(s) = view
                .app_mut(&key.device, app_key)
                .and_then(|a| a.app.sessions.iter_mut().find(|s| s.process_id == pid))
            {
                s.volume = volume;
            }
            core.set_process_volume(&key.device, app_key, pid, volume);
        }
    }
    if let Some(s) = sliders.get_mut(&key.twin()) {
        s.value = value;
    }
}

pub fn toggle_mute(view: &mut View, core: &mut Core, key: &SliderKey) {
    match (&key.app, key.pid) {
        (None, _) => {
            let Some(device) = view.device_mut(&key.device) else {
                return;
            };
            device.device.muted = !device.device.muted;
            let (muted, name) = (device.device.muted, device.device.name.clone());
            core.set_device_mute(&key.device, &name, muted);
        }
        (Some(app_key), None) => {
            let Some(app) = view.app_mut(&key.device, app_key) else {
                return;
            };
            app.app.muted = !app.app.muted;
            let (muted, name) = (app.app.muted, app.app.display_name.clone());
            core.set_app_mute(&key.device, app_key, &name, muted);
        }
        (Some(app_key), Some(pid)) => {
            let Some(session) = view
                .app_mut(&key.device, app_key)
                .and_then(|a| a.app.sessions.iter_mut().find(|s| s.process_id == pid))
            else {
                return;
            };
            session.muted = !session.muted;
            let muted = session.muted;
            core.set_process_mute(&key.device, app_key, pid, muted);
        }
    }
}

/// Mutes every other app on the device and unmutes this one; if it is
/// already soloed, unmutes everything.
pub fn solo(view: &View, core: &mut Core, device: &str, key: &str) {
    let Some(view) = view.device(device) else {
        return;
    };
    let apps: Vec<(String, String, bool)> = view
        .apps
        .iter()
        .filter(|a| !a.hidden)
        .map(|a| (a.app.key.clone(), a.app.display_name.clone(), a.app.muted))
        .collect();
    let soloed = apps.len() > 1
        && apps
            .iter()
            .all(|(k, _, muted)| if k == key { !muted } else { *muted });
    for (k, name, muted) in apps {
        let target = !soloed && k != key;
        if muted != target {
            core.set_app_mute(device, &k, &name, target);
        }
    }
}

/// Runs a menu command; false for `Nothing`.
pub fn run_command(view: &View, core: &mut Core, command: Command) -> bool {
    match command {
        Command::Rule(device, key, kind) => {
            if let Some(app) = view.app(&device, &key).cloned() {
                core.app_rule(&device, &app, kind);
            }
        }
        Command::Route(key, device) => {
            let _ = core.audio.set_app_device(&key, device.as_deref());
        }
        Command::SetDefault(device) => {
            let _ = core.audio.set_default_device(&device);
        }
        Command::HideDevice(device, name) => core.hide_device(&device, &name),
        Command::Unhide(device, entry) => core.unhide_app(&device, &entry),
        Command::UnhideAll(device) => core.unhide_device_apps(&device),
        Command::Nothing => return false,
    }
    true
}

// ---------- peak meters ----------

/// Feeds the device and app meters (and the open popup's) from Windows.
pub fn read_peaks(
    core: &Core,
    view: &View,
    sliders: &mut HashMap<SliderKey, Slider>,
    popup: Option<&PopupKind>,
) {
    let Ok(peaks) = core.audio.peaks() else {
        return;
    };
    for p in peaks {
        let Some(device) = view.device(&p.device_id) else {
            continue;
        };
        let device_volume = device.device.volume;
        match (&p.app_key, p.process_id) {
            (None, _) => {
                for popup_copy in [false, true] {
                    let key = SliderKey::device(&p.device_id, popup_copy);
                    if let Some(s) = sliders.get_mut(&key) {
                        s.set_peaks(p.channels, device_volume);
                    }
                }
            }
            (Some(app_key), pid) => {
                let Some(app) = device.apps.iter().find(|a| &a.app.key == app_key) else {
                    continue;
                };
                let volume = match pid {
                    None => app.app.volume,
                    Some(pid) => app
                        .app
                        .sessions
                        .iter()
                        .find(|s| s.process_id == pid)
                        .map_or(app.app.volume, |s| s.volume),
                };
                let in_popup = matches!(popup, Some(PopupKind::App { device, key })
                    if device == &p.device_id && key == app_key);
                let mut keys = vec![];
                if pid.is_none() {
                    keys.push(SliderKey::app(&p.device_id, app_key, None, false));
                }
                if in_popup {
                    keys.push(SliderKey::app(&p.device_id, app_key, pid, true));
                }
                for key in keys {
                    if let Some(s) = sliders.get_mut(&key) {
                        s.set_peaks(p.channels, volume);
                    }
                }
            }
        }
    }
}

// ---------- popups ----------

fn rule_tools(app: &AppView) -> Vec<Tool> {
    let rule = app.rule.as_ref();
    let mode = rule.map_or(VolumeRuleMode::NONE, |r| r.volume_mode);
    vec![
        Tool {
            glyph: GLYPH_LOCK,
            action: ToolAction::Lock,
            active: mode == VolumeRuleMode::LOCK,
        },
        Tool {
            glyph: GLYPH_LAUNCH,
            action: ToolAction::Launch,
            active: mode == VolumeRuleMode::LAUNCH,
        },
        Tool {
            glyph: GLYPH_FOCUS,
            action: ToolAction::Focus,
            active: rule.is_some_and(|r| r.focus_lost_enabled),
        },
        Tool {
            glyph: GLYPH_MUTE,
            action: ToolAction::HardMute,
            active: rule.is_some_and(|r| r.hard_muted),
        },
    ]
}

/// An app popup's buttons: the rules (apps with an executable), hide,
/// routing (not System sounds) and close.
pub fn app_tools(app: &AppView) -> Vec<Tool> {
    let mut tools = if app.exe_name.is_empty() {
        vec![]
    } else {
        rule_tools(app)
    };
    tools.push(Tool {
        glyph: GLYPH_HIDE,
        action: ToolAction::Hide,
        active: false,
    });
    if !app.app.is_system_sounds {
        tools.push(Tool {
            glyph: GLYPH_ROUTE,
            action: ToolAction::Route,
            active: false,
        });
    }
    tools.push(Tool {
        glyph: GLYPH_CLOSE,
        action: ToolAction::Close,
        active: false,
    });
    tools
}

pub fn device_tools() -> Vec<Tool> {
    vec![
        Tool {
            glyph: GLYPH_MORE,
            action: ToolAction::More,
            active: false,
        },
        Tool {
            glyph: GLYPH_CLOSE,
            action: ToolAction::Close,
            active: false,
        },
    ]
}

/// A device popup's "More" menu: set as default and hide (not for the
/// default, hide only with another device shown), restore hidden apps.
pub fn device_menu(
    view: &View,
    device: &str,
    visible_count: usize,
    commands: &mut Vec<Command>,
) -> Vec<Item> {
    let Some(view) = view.device(device) else {
        return vec![];
    };
    let mut items = vec![];
    let mut add = |items: &mut Vec<Item>, text: &str, command: Command| {
        commands.push(command);
        items.push(Item::command(text, commands.len() as u32));
    };
    if !view.device.is_default {
        add(
            &mut items,
            tr("SetAsDefaultDeviceText"),
            Command::SetDefault(device.into()),
        );
        if visible_count > 1 {
            add(
                &mut items,
                tr("HideDeviceButtonText"),
                Command::HideDevice(device.into(), view.device.name.clone()),
            );
        }
    }
    if !view.hidden_apps.is_empty() {
        let restore = restore_items(view, commands);
        items.push(Item::Submenu(
            tr("RestoreHiddenAppsForDevice").into(),
            restore,
        ));
    }
    items
}

/// The device's hidden apps, then "Restore all".
pub fn restore_items(view: &DeviceView, commands: &mut Vec<Command>) -> Vec<Item> {
    let mut items = vec![];
    for entry in &view.hidden_apps {
        commands.push(Command::Unhide(view.device.id.clone(), entry.clone()));
        items.push(Item::command(entry.label(), commands.len() as u32));
    }
    items.push(Item::Separator);
    commands.push(Command::UnhideAll(view.device.id.clone()));
    items.push(Item::command(
        tr("RestoreHiddenAppsForDeviceAll"),
        commands.len() as u32,
    ));
    items
}

/// What a popup button does: close, hide, lock at once or a menu (launch
/// volume, focus lost, hard mute, routing, the device's "More").
pub fn tool_step(
    view: &View,
    kind: &PopupKind,
    action: ToolAction,
    visible_count: usize,
) -> Option<ToolStep> {
    let mut commands = vec![];
    let mut items = vec![];
    let mut add = |items: &mut Vec<Item>, checked: bool, text: String, command: Command| {
        commands.push(command);
        items.push(Item::check(text, commands.len() as u32, checked));
    };
    match (kind, action) {
        (_, ToolAction::Close) => return Some(ToolStep::Close),
        (PopupKind::App { device, key }, ToolAction::Hide) => {
            return Some(ToolStep::Hide(device.clone(), key.clone()));
        }
        (PopupKind::App { device, key }, ToolAction::Lock) => {
            let app = view.app(device, key)?.clone();
            let current = percent(app.app.volume);
            // Lock is direct: it locks the volume where it is.
            let rule = if app.locked() {
                RuleKind::None
            } else {
                RuleKind::Lock(current)
            };
            return Some(ToolStep::Rule(device.clone(), Box::new(app), rule));
        }
        (PopupKind::App { device, key }, ToolAction::Launch) => {
            let app = view.app(device, key)?;
            let rule = app.rule.as_ref();
            let mode = rule.map_or(VolumeRuleMode::NONE, |r| r.volume_mode);
            let percent_now = percent(app.app.volume);
            for v in [10, 20, 30, 50, 75] {
                let on =
                    mode == VolumeRuleMode::LAUNCH && rule.is_some_and(|r| r.volume_percent == v);
                add(
                    &mut items,
                    on,
                    format!("{v}%"),
                    Command::Rule(device.clone(), key.clone(), RuleKind::Launch(v)),
                );
            }
            items.push(Item::Separator);
            add(
                &mut items,
                false,
                trf(
                    "VolumeRuleUseCurrentFormatText",
                    &[("0", &percent_now.to_string())],
                ),
                Command::Rule(device.clone(), key.clone(), RuleKind::Launch(percent_now)),
            );
            items.push(Item::Separator);
            add(
                &mut items,
                mode == VolumeRuleMode::NONE,
                tr("VolumeRuleMenuNoneText").into(),
                Command::Rule(device.clone(), key.clone(), RuleKind::None),
            );
        }
        (PopupKind::App { device, key }, ToolAction::Focus) => {
            let app = view.app(device, key)?;
            let on = app.rule.as_ref().is_some_and(|r| r.focus_lost_enabled);
            let text = if on {
                tr("FocusLostAppMenuDisableText")
            } else {
                tr("FocusLostAppMenuText")
            };
            add(
                &mut items,
                on,
                text.into(),
                Command::Rule(device.clone(), key.clone(), RuleKind::FocusLost),
            );
        }
        (PopupKind::App { device, key }, ToolAction::HardMute) => {
            let app = view.app(device, key)?;
            let on = app.rule.as_ref().is_some_and(|r| r.hard_muted);
            let text = if on {
                tr("HardMuteAppMenuDisableText")
            } else {
                tr("HardMuteAppMenuText")
            };
            add(
                &mut items,
                on,
                text.into(),
                Command::Rule(device.clone(), key.clone(), RuleKind::HardMute),
            );
        }
        (PopupKind::App { device, key }, ToolAction::Route) => {
            let app = view.app(device, key)?;
            let current = app.app.routed_device_id.clone();
            add(
                &mut items,
                current.is_none(),
                tr("DefaultDeviceText").into(),
                Command::Route(key.clone(), None),
            );
            items.push(Item::Separator);
            for d in &view.devices {
                add(
                    &mut items,
                    current.as_deref() == Some(d.device.id.as_str()),
                    d.device.name.clone(),
                    Command::Route(key.clone(), Some(d.device.id.clone())),
                );
            }
        }
        (PopupKind::Device { device }, ToolAction::More) => {
            items = device_menu(view, device, visible_count, &mut commands);
        }
        _ => return None,
    }
    Some(ToolStep::Menu(items, commands))
}

// ---------- painting ----------

/// Loads an app's 28 DIP icon at `scale` into `icons` once.
pub fn load_icon(icons: &mut HashMap<(String, u32), Option<Rc<Image>>>, path: &str, scale: f32) {
    let size = (28.0 * scale).round() as u32;
    icons.entry((path.into(), size)).or_insert_with(|| {
        icons::bgra(path, size).map(|pixels| Image::from_straight_bgra(size, size, pixels))
    });
}

/// What drawing a row needs from its window.
pub struct RowPaint<'a> {
    pub view: &'a View,
    pub sliders: &'a HashMap<SliderKey, Slider>,
    pub icons: &'a HashMap<(String, u32), Option<Rc<Image>>>,
    pub scale: f32,
    /// The mute button (device) or icon (app) under the pointer, and the
    /// one pressed.
    pub hover: Option<&'a SliderKey>,
    pub pressed: Option<&'a SliderKey>,
}

impl RowPaint<'_> {
    pub fn device_row(&self, canvas: &mut Canvas, theme: &Theme, row: Rect, key: &SliderKey) {
        let Some(device) = self.view.device(&key.device) else {
            return;
        };
        let d = &device.device;
        let mute = mute_rect(row);
        let icons = TextStyle::new(theme.icon_font, 24.0).align(Align::Center);
        let hovered = self.hover == Some(key);
        let (ghost, level) = if d.muted {
            (GLYPH_MUTE, GLYPH_MUTE)
        } else {
            let v = d.volume;
            let level = if v > 0.66 {
                GLYPH_BARS[3]
            } else if v > 0.33 {
                GLYPH_BARS[2]
            } else if v > 0.0 {
                GLYPH_BARS[1]
            } else {
                GLYPH_BARS[0]
            };
            (GLYPH_BARS[3], level)
        };
        let pressed = self.pressed == Some(key) && hovered;
        if pressed {
            let (cx, cy) = mute.center();
            canvas.push_transform(0.0, 0.0, 0.8, cx, cy);
        }
        canvas.text(
            &glyph(ghost),
            mute.inset(-8.0, -8.0),
            &icons,
            theme.text.alpha(0.4),
        );
        canvas.text(
            &glyph(level),
            mute.inset(-8.0, -8.0),
            &icons,
            theme.text.alpha(if hovered { 0.95 } else { 0.9 }),
        );
        if pressed {
            canvas.pop_transform();
        }
        if let Some(slider) = self.sliders.get(key) {
            slider.paint(canvas, slider_rect(row), theme, 1.0);
            let value = TextStyle::new(UI_FONT, 24.0).align(Align::Center);
            canvas.text(
                &slider.value.to_string(),
                value_rect(row),
                &value,
                theme.text,
            );
        }
    }

    pub fn app_row(&self, canvas: &mut Canvas, theme: &Theme, row: Rect, key: &SliderKey) {
        let Some(app_key) = &key.app else {
            return;
        };
        let Some(app) = self.view.app(&key.device, app_key) else {
            return;
        };
        let muted = volume_of(self.view, key).is_some_and(|(_, m)| m);
        let locked = app.locked();
        let faded = if muted { 0.4 } else { 1.0 };
        let cell = mute_rect(row);
        let icon = app
            .app
            .executable
            .as_ref()
            .and_then(|p| {
                self.icons
                    .get(&(p.clone(), (28.0 * self.scale).round() as u32))
            })
            .cloned()
            .flatten();
        match icon {
            Some(image) => canvas.image(&image, cell, faded),
            None => {
                canvas.rect(cell, theme.accent.alpha(faded));
                let (text, style) = if app.app.is_system_sounds {
                    (
                        glyph(GLYPH_SYSTEM_SOUNDS),
                        TextStyle::new(theme.icon_font, 16.0),
                    )
                } else {
                    let name = match key.pid {
                        Some(pid) => app
                            .app
                            .sessions
                            .iter()
                            .find(|s| s.process_id == pid)
                            .map_or(app.app.display_name.as_str(), |s| s.display_name.as_str()),
                        None => app.app.display_name.as_str(),
                    };
                    (
                        first_letter(name),
                        TextStyle::new(UI_FONT, 16.0).weight(600),
                    )
                };
                canvas.text(
                    &text,
                    cell.offset(0.0, -0.5),
                    &style.align(Align::Center),
                    theme.text.alpha(faded),
                );
            }
        }
        if muted {
            let icons = TextStyle::new(theme.icon_font, 24.0).align(Align::Center);
            canvas.text(
                &glyph(GLYPH_MUTE),
                cell.inset(-8.0, -8.0),
                &icons,
                theme.text.alpha(0.8),
            );
        }
        if locked {
            // Lock badge: a 10 DIP accent dot with the lock glyph.
            let (bx, by) = (cell.right() + 1.0 - 5.0, cell.bottom() + 1.0 - 5.0);
            canvas.circle(bx, by, 5.0, theme.accent);
            let badge = TextStyle::new(theme.icon_font, 6.0).align(Align::Center);
            canvas.text(
                &glyph(GLYPH_LOCK),
                Rect::new(bx - 5.0, by - 5.0, 10.0, 10.0),
                &badge,
                theme.text,
            );
        }
        if let Some(slider) = self.sliders.get(key) {
            let opacity = faded * if locked { 0.5 } else { 1.0 };
            slider.paint(canvas, slider_rect(row), theme, opacity);
            let value = TextStyle::new(UI_FONT, 18.0).align(Align::Center);
            canvas.text(
                &slider.value.to_string(),
                value_rect(row),
                &value,
                theme.text.alpha(faded),
            );
        }
    }

    /// A device or app row by its key.
    pub fn row(&self, canvas: &mut Canvas, theme: &Theme, row: Rect, key: &SliderKey) {
        if key.app.is_some() {
            self.app_row(canvas, theme, row, key);
        } else {
            self.device_row(canvas, theme, row, key);
        }
    }
}

/// The lock badge's tooltip area on an app's icon.
pub fn lock_badge(cell: Rect) -> Rect {
    Rect::new(cell.right() - 9.0, cell.bottom() - 9.0, 10.0, 10.0)
}

/// The popup's header buttons, right-aligned in its frame.
pub fn tool_rects(frame: Rect, count: usize) -> Vec<Rect> {
    let n = count as f32;
    (0..count)
        .map(|i| {
            Rect::new(
                frame.right() - 1.0 - (n - i as f32) * 48.0,
                frame.y + 1.0,
                48.0,
                48.0,
            )
        })
        .collect()
}

/// A popup's frame, accent border, header, title and buttons (`hover` and
/// `pressed` are button indexes).
pub fn draw_popup_frame(
    canvas: &mut Canvas,
    theme: &Theme,
    frame: Rect,
    title: &str,
    tools: &[Tool],
    hover: Option<usize>,
    pressed: Option<usize>,
) {
    canvas.rect(frame, theme.popup_bg);
    canvas.stroke_rect(frame, 1.0, theme.accent);
    let header = Rect::new(frame.x + 1.0, frame.y + 1.0, frame.w - 2.0, TITLE_H);
    canvas.rect(header, theme.accent_30);
    let rects = tool_rects(frame, tools.len());
    let title_right = rects.first().map_or(header.right(), |r| r.x);
    canvas.text(
        title,
        Rect::new(
            header.x + 12.0,
            header.y,
            (title_right - header.x - 24.0).max(0.0),
            TITLE_H,
        ),
        &TextStyle::new(UI_FONT, 15.0).ellipsis(),
        theme.text,
    );
    for (i, (tool, rect)) in tools.iter().zip(&rects).enumerate() {
        if tool.active {
            canvas.rect(*rect, theme.accent_22);
        }
        if hover == Some(i) {
            canvas.rect(
                *rect,
                if pressed == Some(i) {
                    theme.pressed
                } else {
                    theme.hover
                },
            );
        }
        let size = if tool.action == ToolAction::Close {
            10.0
        } else {
            16.0
        };
        let style = TextStyle::new(theme.icon_font, size).align(Align::Center);
        canvas.text(&glyph(tool.glyph), *rect, &style, theme.text);
    }
}

/// The keyboard focus ring: 2 px Text over 1 px background, inset.
pub fn draw_focus_ring(canvas: &mut Canvas, theme: &Theme, row: Rect) {
    canvas.stroke_rect(row, 2.0, theme.text);
    canvas.stroke_rect(row.inset(2.0, 2.0), 1.0, theme.popup_bg);
}
