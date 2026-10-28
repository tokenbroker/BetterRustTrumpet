//! The flyout, after the original's FlyoutWindow / DeviceView /
//! AppItemView: every output device (or only the default one, collapsed)
//! with its apps, sliders, mute buttons and peak meters; the app and device
//! popups with the rule buttons; keyboard control; undo and redo.
use super::{
    MenuRequest,
    anim::{self, BACK_OUT, Curve, EASE_IN, EASE_OUT, FADE_OUT_QUAD},
    slider::Slider,
    theme::Theme,
};
use crate::core::{AppView, Core, DeviceView, HiddenEntry, RuleKind, View};
use bt_app::win32::{
    gfx::{Align, Canvas, Gfx, Image, Rect, Surface, TextStyle},
    icons,
    menu::Item,
    sound,
    wnd::{self, Button, Event, Modifiers, Window},
};
use bt_core::{
    models::VolumeRuleMode,
    settings::{Key, Value},
};
use std::{
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

/// The original's flyout width (`WindowAndItemSize`).
pub const WIDTH: f32 = 360.0;
const TITLE_H: f32 = 48.0;
const DEVICE_ROW_H: f32 = 60.0;
const APP_ROW_H: f32 = 52.0;
const APPS_PAD: f32 = 6.0;
const FRAME: usize = 1;
const TICK_WAV: &[u8] = include_bytes!("../../assets/tick.wav");
const UI_FONT: &str = "Segoe UI";

// Segoe MDL2 / Fluent glyphs.
const GLYPH_MUTE: char = '\u{E74F}';
const GLYPH_BARS: [char; 4] = ['\u{E992}', '\u{E993}', '\u{E994}', '\u{E995}'];
const GLYPH_SYSTEM_SOUNDS: char = '\u{E7F5}';
const GLYPH_CHEVRON: char = '\u{E010}';
const GLYPH_RESTORE: char = '\u{E18B}';
const GLYPH_PIN: char = '\u{E840}';
const GLYPH_PINNED: char = '\u{E841}';
const GLYPH_LOCK: char = '\u{E72E}';
const GLYPH_LAUNCH: char = '\u{E768}';
const GLYPH_FOCUS: char = '\u{E7F4}';
const GLYPH_HIDE: char = '\u{E18B}';
const GLYPH_ROUTE: char = '\u{E8AB}';
const GLYPH_MORE: char = '\u{E10C}';
const GLYPH_CLOSE: char = '\u{E8BB}';

/// Which slider: a device, an app, or one process of an app; `popup` for
/// the copies shown in the popups.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SliderKey {
    device: String,
    app: Option<String>,
    pid: Option<u32>,
    popup: bool,
}

impl SliderKey {
    fn device(device: &str, popup: bool) -> Self {
        Self {
            device: device.into(),
            app: None,
            pid: None,
            popup,
        }
    }
    fn app(device: &str, key: &str, pid: Option<u32>, popup: bool) -> Self {
        Self {
            device: device.into(),
            app: Some(key.into()),
            pid,
            popup,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Expand,
    Pin,
    Restore,
    /// A row's mute button (device) or icon (app).
    Mute(SliderKey),
    Slider(SliderKey),
    /// A row outside its controls.
    Row(SliderKey),
    /// The device block's title area.
    DeviceTitle(String),
    Tool(usize),
    Dismiss,
    /// Inside the popup but on nothing.
    PopupBody,
    Scrollbar,
    Header,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ToolAction {
    Lock,
    Launch,
    Focus,
    HardMute,
    Hide,
    Route,
    More,
    Close,
}

struct Tool {
    glyph: char,
    action: ToolAction,
    active: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum PopupKind {
    App { device: String, key: String },
    Device { device: String },
}

struct Popup {
    kind: PopupKind,
    title: String,
    tools: Vec<Tool>,
    /// The row the popup opened over (window coordinates at opening).
    anchor_top: f32,
    offset: f32,
    opened: Instant,
}

/// Commands chosen from the flyout's menus.
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

/// A row in the list or a popup, in window coordinates.
#[derive(Clone, Debug)]
struct Row {
    rect: Rect,
    key: SliderKey,
    /// Cascade order on open (device blocks and app rows, top to bottom).
    cascade: usize,
}

struct Block {
    rect: Rect,
    device: String,
    title: Option<String>,
    cascade: usize,
}

#[derive(Default)]
struct Layout {
    blocks: Vec<Block>,
    rows: Vec<Row>,
    content: f32,
    /// The top device block's title row (the header overlays it).
    header: Option<Rect>,
}

pub struct Flyout {
    pub window: Window,
    gfx: Rc<Gfx>,
    /// Taken out while drawing.
    surface: Option<Surface>,
    pub visible: bool,
    closing: Option<Instant>,
    opened: Instant,
    view: View,
    expanded: bool,
    chevron: Option<Instant>,
    pub pinned: bool,
    sliders: HashMap<SliderKey, Slider>,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    capture: Option<(SliderKey, Rect)>,
    scroll_drag: Option<(f32, f32)>,
    focus: Option<usize>,
    keyboard: bool,
    scroll: f32,
    popup: Option<Popup>,
    dismiss_out: Option<Instant>,
    hiding_app: Option<(String, String, Instant)>,
    pub hidden_at: Option<Instant>,
    icons: HashMap<(String, u32), Option<Rc<Image>>>,
    /// Window height (DIPs), position (pixels) and scale.
    height: f32,
    position: (i32, i32),
    scale: f32,
    max_height: f32,
    tray: Option<(i32, i32, i32, i32)>,
    last_tick: Instant,
    last_peaks: Instant,
    pointer: Option<(f32, f32)>,
}

fn percent(volume: f32) -> i32 {
    (volume * 100.0).round() as i32
}

fn exe_file(app: &AppView) -> &str {
    let path = app.app.executable.as_deref().unwrap_or("");
    match path.rsplit(['\\', '/']).next() {
        Some(name) if !name.is_empty() => name,
        _ => &app.app.display_name,
    }
}

/// Apps by executable name, ordinal (AppItemViewModel's comparer); System
/// sounds last.
fn sorted_apps(device: &DeviceView) -> Vec<&AppView> {
    let mut apps: Vec<&AppView> = device.apps.iter().filter(|a| !a.hidden).collect();
    apps.sort_by(|a, b| {
        a.app
            .is_system_sounds
            .cmp(&b.app.is_system_sounds)
            .then_with(|| exe_file(a).cmp(exe_file(b)))
    });
    apps
}

fn glyph(c: char) -> String {
    c.to_string()
}

fn first_letter(name: &str) -> String {
    name.trim()
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or("?".into(), |c| c.to_uppercase().collect())
}

impl Flyout {
    pub fn new(gfx: Rc<Gfx>, store: &crate::store::Store) -> windows::core::Result<Self> {
        let window = wnd::create(
            "BetterTrumpet",
            wnd::Options {
                activate: true,
                click_through: false,
            },
        )?;
        window.round_corners();
        let now = Instant::now();
        Ok(Self {
            window,
            surface: Some(Surface::new(gfx.clone())?),
            gfx,
            visible: false,
            closing: None,
            opened: now,
            view: View::default(),
            expanded: store.bool(Key::IsExpanded),
            chevron: None,
            pinned: std::env::args().any(|a| a == "--pinned"),
            sliders: HashMap::new(),
            hover: None,
            pressed: None,
            capture: None,
            scroll_drag: None,
            focus: None,
            keyboard: false,
            scroll: 0.0,
            popup: None,
            dismiss_out: None,
            hiding_app: None,
            hidden_at: None,
            icons: HashMap::new(),
            height: 48.0,
            position: (0, 0),
            scale: 1.0,
            max_height: 900.0,
            tray: None,
            last_tick: now,
            last_peaks: now,
            pointer: None,
        })
    }

    /// Shown and not closing.
    pub fn showing(&self) -> bool {
        self.visible && self.closing.is_none()
    }

    // ---------- devices and layout ----------

    /// Collapsed: the default device only. Expanded: every visible device,
    /// the others in reverse name order and the default at the bottom,
    /// nearest the taskbar.
    fn shown(&self) -> Vec<&DeviceView> {
        let visible: Vec<&DeviceView> = self.view.devices.iter().filter(|d| !d.hidden).collect();
        let default = visible.iter().copied().find(|d| d.device.is_default);
        if !self.expanded {
            return default.or(visible.first().copied()).into_iter().collect();
        }
        let mut others: Vec<&DeviceView> = visible
            .iter()
            .copied()
            .filter(|d| !d.device.is_default)
            .collect();
        others.sort_by(|a, b| b.device.name.cmp(&a.device.name));
        others.extend(default);
        others
    }

    fn visible_count(&self) -> usize {
        self.view.devices.iter().filter(|d| !d.hidden).count()
    }

    fn layout(&self) -> Layout {
        let mut out = Layout::default();
        let mut y = -self.scroll;
        let mut cascade = 0;
        for (index, device) in self.shown().into_iter().enumerate() {
            let id = &device.device.id;
            out.blocks.push(Block {
                rect: Rect::new(0.0, y, WIDTH, TITLE_H + DEVICE_ROW_H),
                device: id.clone(),
                // The top device's name is in the header instead.
                title: (index > 0).then(|| device.device.name.clone()),
                cascade,
            });
            if index == 0 {
                out.header = Some(Rect::new(0.0, y, WIDTH, TITLE_H));
            }
            out.rows.push(Row {
                rect: Rect::new(0.0, y + TITLE_H, WIDTH, DEVICE_ROW_H),
                key: SliderKey::device(id, false),
                cascade,
            });
            cascade += 1;
            y += TITLE_H + DEVICE_ROW_H + APPS_PAD;
            for app in sorted_apps(device) {
                out.rows.push(Row {
                    rect: Rect::new(0.0, y, WIDTH, APP_ROW_H),
                    key: SliderKey::app(id, &app.app.key, None, false),
                    cascade,
                });
                cascade += 1;
                y += APP_ROW_H;
            }
            y += APPS_PAD;
        }
        out.content = y + self.scroll;
        out
    }

    fn no_devices_height(&self) -> f32 {
        self.gfx.measure_wrapped(
            NO_DEVICES,
            &TextStyle::new(UI_FONT, 13.0).align(Align::Center),
            WIDTH - 48.0,
        ) + 48.0
            + 48.0
    }

    fn content_height(&self) -> f32 {
        if self.view.devices.is_empty() {
            self.no_devices_height()
        } else {
            self.layout().content
        }
    }

    fn scrollable(&self) -> bool {
        self.content_height() > self.height + 1.0
    }

    // ---------- popup layout ----------

    fn popup_rows(&self) -> Vec<SliderKey> {
        match &self.popup {
            Some(Popup {
                kind: PopupKind::App { device, key },
                ..
            }) => {
                let Some(app) = self.view.app(device, key) else {
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
            Some(Popup {
                kind: PopupKind::Device { device },
                ..
            }) => vec![SliderKey::device(device, true)],
            None => vec![],
        }
    }

    /// The popup's frame and its rows, in window coordinates.
    fn popup_layout(&self) -> Option<(Rect, Vec<Row>)> {
        let popup = self.popup.as_ref()?;
        let keys = self.popup_rows();
        let row_h = if matches!(popup.kind, PopupKind::Device { .. }) {
            DEVICE_ROW_H
        } else {
            APP_ROW_H
        };
        let height = 1.0 + TITLE_H + APPS_PAD * 2.0 + row_h * keys.len() as f32 + 1.0;
        let top = (popup.anchor_top - popup.offset)
            .min(self.height - height)
            .max(0.0);
        let frame = Rect::new(0.0, top, WIDTH, height);
        let mut y = top + 1.0 + TITLE_H + APPS_PAD;
        let rows = keys
            .into_iter()
            .map(|key| {
                let row = Row {
                    rect: Rect::new(1.0, y, WIDTH - 2.0, row_h),
                    key,
                    cascade: 0,
                };
                y += row_h;
                row
            })
            .collect();
        Some((frame, rows))
    }

    fn tool_rects(&self, frame: Rect) -> Vec<Rect> {
        let Some(popup) = &self.popup else {
            return vec![];
        };
        let n = popup.tools.len() as f32;
        (0..popup.tools.len())
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

    // ---------- row geometry ----------

    fn mute_rect(row: Rect) -> Rect {
        Rect::new(row.x + 24.5, row.y + row.h / 2.0 - 14.0, 28.0, 28.0)
    }
    fn slider_rect(row: Rect) -> Rect {
        Rect::new(
            row.x + 72.0,
            row.y + row.h / 2.0 - 16.0,
            row.w - 140.0,
            32.0,
        )
    }
    fn value_rect(row: Rect) -> Rect {
        Rect::new(row.right() - 69.0, row.y, 69.0, row.h)
    }

    fn header_buttons(&self, header: Rect) -> (Rect, Rect) {
        let bar = if self.scrollable() { 16.0 } else { 0.0 };
        let pin = Rect::new(WIDTH - 24.0 - bar - 30.0, header.y, 30.0, 48.0);
        let restore = Rect::new(WIDTH - 54.0 - bar - 30.0, header.y, 30.0, 48.0);
        (pin, restore)
    }

    fn top_device(&self) -> Option<&DeviceView> {
        self.shown().into_iter().next()
    }

    // ---------- hit testing ----------

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if let Some((frame, rows)) = self.popup_layout() {
            if !frame.contains(x, y) {
                return Some(Hit::Dismiss);
            }
            for (i, rect) in self.tool_rects(frame).into_iter().enumerate() {
                if rect.contains(x, y) {
                    return Some(Hit::Tool(i));
                }
            }
            for row in rows {
                if let Some(hit) = Self::row_hit(&row, x, y) {
                    return Some(hit);
                }
            }
            return Some(Hit::PopupBody);
        }
        if self.scrollable() && x >= WIDTH - 16.0 {
            return Some(Hit::Scrollbar);
        }
        let layout = self.layout();
        if let Some(header) = layout.header.filter(|h| h.contains(x, y)) {
            let (pin, restore) = self.header_buttons(header);
            if pin.contains(x, y) {
                return Some(Hit::Pin);
            }
            if restore.contains(x, y)
                && self.top_device().is_some_and(|d| !d.hidden_apps.is_empty())
            {
                return Some(Hit::Restore);
            }
            return Some(if self.visible_count() > 1 {
                Hit::Expand
            } else {
                Hit::Header
            });
        }
        for row in &layout.rows {
            if let Some(hit) = Self::row_hit(row, x, y) {
                return Some(hit);
            }
        }
        for block in &layout.blocks {
            if block.rect.contains(x, y) {
                return Some(Hit::DeviceTitle(block.device.clone()));
            }
        }
        None
    }

    fn row_hit(row: &Row, x: f32, y: f32) -> Option<Hit> {
        if !row.rect.contains(x, y) {
            return None;
        }
        if Self::mute_rect(row.rect).contains(x, y) {
            return Some(Hit::Mute(row.key.clone()));
        }
        if Self::slider_rect(row.rect).contains(x, y) {
            return Some(Hit::Slider(row.key.clone()));
        }
        Some(Hit::Row(row.key.clone()))
    }

    // ---------- state from the view ----------

    fn volume_of(&self, key: &SliderKey) -> Option<(f32, bool)> {
        let device = self.view.device(&key.device)?;
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

    fn locked(&self, key: &SliderKey) -> bool {
        key.app
            .as_ref()
            .and_then(|app| self.view.app(&key.device, app))
            .is_some_and(AppView::locked)
    }

    fn slider(&mut self, key: &SliderKey) -> &mut Slider {
        let value = self.volume_of(key).map_or(0, |(v, _)| percent(v));
        self.sliders
            .entry(key.clone())
            .or_insert_with(|| Slider::new(value))
    }

    /// Re-reads devices and apps; sliders in use keep their value.
    pub fn refresh(&mut self, core: &mut Core) {
        self.view = core.view();
        self.sync();
    }

    fn sync(&mut self) {
        let now = Instant::now();
        let mut keys: Vec<SliderKey> = self.layout().rows.into_iter().map(|r| r.key).collect();
        if let Some((_, rows)) = self.popup_layout() {
            keys.extend(rows.into_iter().map(|r| r.key));
        }
        self.sliders.retain(|k, _| keys.contains(k));
        for key in keys {
            if let Some((volume, _)) = self.volume_of(&key) {
                self.slider(&key).update(percent(volume), now);
            }
        }
        // A popup whose app or device went away closes.
        let gone = match self.popup.as_ref().map(|p| &p.kind) {
            Some(PopupKind::App { device, key }) => self.view.app(device, key).is_none(),
            Some(PopupKind::Device { device }) => self.view.device(device).is_none(),
            None => false,
        };
        if gone {
            self.popup = None;
            self.dismiss_out = None;
        }
        let rows = self.focus_rows().len();
        if self.focus.is_some_and(|f| f >= rows) {
            self.focus = rows.checked_sub(1);
        }
        self.fit();
    }

    // ---------- show / hide / placement ----------

    /// Opens the flyout (tray click, hotkey, second launch).
    pub fn show(&mut self, core: &mut Core, theme: &Theme, tray: Option<(i32, i32, i32, i32)>) {
        self.tray = tray.or(self.tray);
        self.closing = None;
        self.popup = None;
        self.dismiss_out = None;
        self.hiding_app = None;
        self.scroll = 0.0;
        self.keyboard = false;
        self.view = core.view();
        self.sync();
        for slider in self.sliders.values_mut() {
            slider.reset_peaks();
        }
        // Opening focuses the first device without a focus ring.
        self.focus = (!self.focus_rows().is_empty()).then_some(0);
        self.opened = Instant::now();
        self.window.set_acrylic(theme.acrylic);
        self.place();
        self.paint(theme);
        self.window.show(true);
        self.window.focus();
        self.visible = true;
        self.window.set_timer(FRAME, 15);
    }

    /// Plays the close animation (8 DIP toward the taskbar and a fade,
    /// 105 ms), then hides.
    pub fn hide(&mut self) {
        if !self.visible || self.closing.is_some() {
            return;
        }
        self.closing = Some(Instant::now());
        if let Some((key, _)) = self.capture.take() {
            self.slider(&key).release(Instant::now());
            self.window.release_capture();
        }
        self.window.set_timer(FRAME, 15);
    }

    fn finish_hide(&mut self) {
        self.window.kill_timer(FRAME);
        self.window.hide();
        self.visible = false;
        self.closing = None;
        self.popup = None;
        self.hover = None;
        self.pressed = None;
        for slider in self.sliders.values_mut() {
            slider.reset_peaks();
        }
        // A hidden window keeps no pixels.
        if let Some(surface) = &mut self.surface {
            surface.release();
        }
        self.icons.clear();
    }

    /// The flyout's height hugs its content, capped at the work area minus
    /// the 12 DIP margins.
    fn fit(&mut self) {
        let mut wanted = self.content_height();
        if let Some((frame, _)) = self.popup_layout() {
            wanted = wanted.max(frame.h);
        }
        let height = wanted.clamp(48.0, self.max_height.max(48.0));
        if (height - self.height).abs() > 0.5 {
            self.height = height;
            let max_scroll = (self.content_height() - self.height).max(0.0);
            self.scroll = self.scroll.min(max_scroll);
            if self.visible {
                self.place();
            }
        }
    }

    /// The original's placement for a bottom taskbar: the bottom-right
    /// corner of the tray icon's monitor, 12 DIP in from the work area. With
    /// an auto-hidden taskbar the bottom is where the taskbar slides in.
    fn place(&mut self) {
        let (cx, cy, top) = match self.tray {
            Some((x, y, w, h)) => (x + w / 2, y + h / 2, Some((y, h))),
            None => {
                let (x, y) = wnd::cursor_pos();
                (x, y, None)
            }
        };
        let monitor = wnd::monitor_at(cx, cy);
        self.scale = monitor.scale;
        let (wx, wy, ww, wh) = monitor.work;
        self.max_height = wh as f32 / self.scale - 24.0;
        let mut height = self.content_height();
        if let Some((frame, _)) = self.popup_layout() {
            height = height.max(frame.h);
        }
        self.height = height.clamp(48.0, self.max_height.max(48.0));
        let margin = (12.0 * self.scale) as i32;
        let width = (WIDTH * self.scale).round() as i32;
        let pixels_h = (self.height * self.scale).round() as i32;
        let mut bottom = wy + wh;
        if let Some((icon_y, icon_h)) = top {
            let screen_bottom = monitor.bounds.1 + monitor.bounds.3;
            bottom = bottom.min(icon_y.min(screen_bottom - icon_h));
        }
        let x = wx + ww - width - margin;
        let y = (bottom - pixels_h - margin).max(wy + margin);
        self.position = (x, y);
    }

    pub fn dpi_changed(&mut self, theme: &Theme) {
        if self.visible {
            self.place();
            self.paint(theme);
        }
    }

    // ---------- frames ----------

    fn animating(&self, now: Instant) -> bool {
        anim::running(Some(self.opened), 24.0 * 28.0 + 260.0, now)
            || self.closing.is_some()
            || anim::running(self.chevron, 220.0, now)
            || self
                .popup
                .as_ref()
                .is_some_and(|p| anim::running(Some(p.opened), 150.0, now))
            || anim::running(self.dismiss_out, 150.0, now)
            || self.hiding_app.is_some()
            || self.sliders.values().any(Slider::animating)
    }

    /// The frame timer: animations, the peak meters at PeakMeterFps.
    pub fn timer(&mut self, id: usize, core: &mut Core, theme: &Theme) {
        if id != FRAME || !self.visible {
            return;
        }
        let now = Instant::now();
        if let Some(start) = self.closing {
            if anim::progress(start, 0.0, 105.0, now) >= 1.0 {
                self.finish_hide();
                return;
            }
        }
        if let Some((device, key, start)) = self.hiding_app.clone() {
            if anim::progress(start, 0.0, 140.0, now) >= 1.0 {
                self.hiding_app = None;
                self.finish_hide_app(core, &device, &key);
            }
        }
        let keys: Vec<SliderKey> = self.sliders.keys().cloned().collect();
        for key in keys {
            if let Some(value) = self.slider(&key).tick(now) {
                self.apply_value(core, theme, &key, value);
            }
        }
        let frame_ms = 1000.0 / theme.peak_fps as f32;
        let mut showing = false;
        if now.duration_since(self.last_peaks).as_secs_f32() * 1000.0 >= frame_ms - 2.0 {
            self.last_peaks = now;
            self.read_peaks(core);
            for slider in self.sliders.values_mut() {
                showing |= slider.animate_peaks();
            }
        }
        let animating = self.animating(now);
        if animating || showing {
            self.paint(theme);
        }
        // Fast frames only while animating; meters at their own rate.
        let interval = if animating {
            15
        } else {
            frame_ms.round() as u32
        };
        self.window.set_timer(FRAME, interval);
    }

    fn read_peaks(&mut self, core: &Core) {
        let Ok(peaks) = core.audio.peaks() else {
            return;
        };
        let popup = self.popup.as_ref().map(|p| p.kind.clone());
        for p in peaks {
            let Some(device) = self.view.device(&p.device_id) else {
                continue;
            };
            let device_volume = device.device.volume;
            match (&p.app_key, p.process_id) {
                (None, _) => {
                    for popup_copy in [false, true] {
                        let key = SliderKey::device(&p.device_id, popup_copy);
                        if let Some(s) = self.sliders.get_mut(&key) {
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
                    let in_popup = matches!(&popup, Some(PopupKind::App { device, key })
                        if device == &p.device_id && key == app_key);
                    let mut keys = vec![];
                    if pid.is_none() {
                        keys.push(SliderKey::app(&p.device_id, app_key, None, false));
                    }
                    if in_popup {
                        keys.push(SliderKey::app(&p.device_id, app_key, pid, true));
                    }
                    for key in keys {
                        if let Some(s) = self.sliders.get_mut(&key) {
                            s.set_peaks(p.channels, volume);
                        }
                    }
                }
            }
        }
    }

    // ---------- actions ----------

    fn play_tick(&mut self, theme: &Theme, value: i32) {
        let now = Instant::now();
        if !theme.tick_sound || now.duration_since(self.last_tick) < Duration::from_millis(50) {
            return;
        }
        self.last_tick = now;
        sound::play(TICK_WAV, value as f32 / 100.0);
    }

    /// A slider value chosen by the user.
    fn apply_value(&mut self, core: &mut Core, theme: &Theme, key: &SliderKey, value: i32) {
        let volume = value as f32 / 100.0;
        self.play_tick(theme, value);
        match (&key.app, key.pid) {
            (None, _) => {
                let Some(device) = self.view.device_mut(&key.device) else {
                    return;
                };
                let previous = percent(device.device.volume);
                device.device.volume = volume;
                let name = device.device.name.clone();
                core.set_device_volume(&key.device, &name, volume, previous);
            }
            (Some(app_key), None) => {
                let Some(app) = self.view.app_mut(&key.device, app_key) else {
                    return;
                };
                let previous = percent(app.app.volume);
                app.app.volume = volume;
                let name = app.app.display_name.clone();
                core.set_app_volume(&key.device, app_key, &name, volume, previous);
            }
            (Some(app_key), Some(pid)) => {
                if let Some(s) = self
                    .view
                    .app_mut(&key.device, app_key)
                    .and_then(|a| a.app.sessions.iter_mut().find(|s| s.process_id == pid))
                {
                    s.volume = volume;
                }
                core.set_process_volume(&key.device, app_key, pid, volume);
            }
        }
        // The other copy of this slider (list or popup) follows.
        let twin = SliderKey {
            popup: !key.popup,
            ..key.clone()
        };
        if let Some(s) = self.sliders.get_mut(&twin) {
            s.value = value;
        }
    }

    fn toggle_mute(&mut self, core: &mut Core, key: &SliderKey) {
        match (&key.app, key.pid) {
            (None, _) => {
                let Some(device) = self.view.device_mut(&key.device) else {
                    return;
                };
                device.device.muted = !device.device.muted;
                let (muted, name) = (device.device.muted, device.device.name.clone());
                core.set_device_mute(&key.device, &name, muted);
            }
            (Some(app_key), None) => {
                let Some(app) = self.view.app_mut(&key.device, app_key) else {
                    return;
                };
                app.app.muted = !app.app.muted;
                let (muted, name) = (app.app.muted, app.app.display_name.clone());
                core.set_app_mute(&key.device, app_key, &name, muted);
            }
            (Some(app_key), Some(pid)) => {
                let Some(session) = self
                    .view
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
        self.refresh(core);
    }

    /// Mutes every other app on the device and unmutes this one; if it is
    /// already soloed, unmutes everything.
    fn solo(&mut self, core: &mut Core, device: &str, key: &str) {
        let Some(view) = self.view.device(device) else {
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
        self.refresh(core);
    }

    fn hide_app(&mut self, device: &str, key: &str) {
        self.hiding_app = Some((device.into(), key.into(), Instant::now()));
        self.window.set_timer(FRAME, 15);
    }

    fn finish_hide_app(&mut self, core: &mut Core, device: &str, key: &str) {
        if let Some(app) = self.view.app(device, key).cloned() {
            core.hide_app(device, &app);
        }
        self.refresh(core);
    }

    pub fn command(&mut self, core: &mut Core, theme: &Theme, command: Command) {
        match command {
            Command::Rule(device, key, kind) => {
                if let Some(app) = self.view.app(&device, &key).cloned() {
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
            Command::Nothing => return,
        }
        self.close_popup();
        self.refresh(core);
        if self.visible {
            self.paint(theme);
        }
    }

    // ---------- popups ----------

    fn rule_tools(&self, app: &AppView) -> Vec<Tool> {
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

    fn open_app_popup(&mut self, device: &str, key: &str) {
        let Some(app) = self.view.app(device, key).cloned() else {
            return;
        };
        let Some(row) = self
            .layout()
            .rows
            .into_iter()
            .find(|r| r.key == SliderKey::app(device, key, None, false))
        else {
            return;
        };
        let mut tools = if app.exe_name.is_empty() {
            vec![]
        } else {
            self.rule_tools(&app)
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
        self.popup = Some(Popup {
            kind: PopupKind::App {
                device: device.into(),
                key: key.into(),
            },
            title: app.app.display_name.clone(),
            tools,
            anchor_top: row.rect.y,
            // Shifted up so its first row lies over the original row.
            offset: TITLE_H + APPS_PAD + 1.0,
            opened: Instant::now(),
        });
        self.after_popup_change();
    }

    fn device_menu(&self, device: &str, commands: &mut Vec<Command>) -> Vec<Item> {
        let Some(view) = self.view.device(device) else {
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
                "Set as default device",
                Command::SetDefault(device.into()),
            );
            if self.visible_count() > 1 {
                add(
                    &mut items,
                    "Hide this device",
                    Command::HideDevice(device.into(), view.device.name.clone()),
                );
            }
        }
        if !view.hidden_apps.is_empty() {
            let restore = Self::restore_items(view, commands);
            items.push(Item::Submenu(
                "Restore hidden app entries on this device".into(),
                restore,
            ));
        }
        items
    }

    fn restore_items(view: &DeviceView, commands: &mut Vec<Command>) -> Vec<Item> {
        let mut items = vec![];
        for entry in &view.hidden_apps {
            commands.push(Command::Unhide(view.device.id.clone(), entry.clone()));
            items.push(Item::command(entry.label(), commands.len() as u32));
        }
        items.push(Item::Separator);
        commands.push(Command::UnhideAll(view.device.id.clone()));
        items.push(Item::command(
            "Restore all on this device",
            commands.len() as u32,
        ));
        items
    }

    fn open_device_popup(&mut self, device: &str) {
        // With nothing to offer the original does not open the popup.
        if self.device_menu(device, &mut vec![]).is_empty() {
            return;
        }
        let Some(view) = self.view.device(device) else {
            return;
        };
        let Some(row) = self
            .layout()
            .rows
            .into_iter()
            .find(|r| r.key == SliderKey::device(device, false))
        else {
            return;
        };
        self.popup = Some(Popup {
            kind: PopupKind::Device {
                device: device.into(),
            },
            title: view.device.name.clone(),
            tools: vec![
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
            ],
            anchor_top: row.rect.y,
            offset: TITLE_H + APPS_PAD,
            opened: Instant::now(),
        });
        self.after_popup_change();
    }

    fn after_popup_change(&mut self) {
        self.dismiss_out = None;
        self.focus = Some(0);
        self.sync();
        self.place();
        self.window.set_timer(FRAME, 15);
    }

    fn close_popup(&mut self) {
        if self.popup.take().is_some() {
            self.dismiss_out = Some(Instant::now());
            self.focus = Some(0);
            self.sync();
            self.window.set_timer(FRAME, 15);
        }
    }

    /// Screen point below a rectangle (for menus that open under a button).
    fn screen_below(&self, rect: Rect) -> (i32, i32) {
        (
            self.position.0 + (rect.x * self.scale).round() as i32,
            self.position.1 + (rect.bottom() * self.scale).round() as i32,
        )
    }

    fn tool(&mut self, core: &mut Core, index: usize) -> Option<MenuRequest> {
        let popup = self.popup.as_ref()?;
        let action = popup.tools.get(index)?.action;
        let (frame, _) = self.popup_layout()?;
        let button = *self.tool_rects(frame).get(index)?;
        let (x, y) = self.screen_below(button);
        let mut commands = vec![];
        let mut items = vec![];
        let mut add = |items: &mut Vec<Item>,
                       item: fn(String, u32) -> Item,
                       text: String,
                       command: Command| {
            commands.push(command);
            items.push(item(text, commands.len() as u32));
        };
        let checked = |checked: bool| -> fn(String, u32) -> Item {
            if checked {
                |t, id| Item::check(t, id, true)
            } else {
                |t, id| Item::check(t, id, false)
            }
        };
        match (&popup.kind, action) {
            (_, ToolAction::Close) => {
                self.close_popup();
                return None;
            }
            (PopupKind::App { device, key }, ToolAction::Hide) => {
                let (device, key) = (device.clone(), key.clone());
                self.close_popup();
                self.hide_app(&device, &key);
                return None;
            }
            (PopupKind::App { device, key }, ToolAction::Lock) => {
                let app = self.view.app(device, key)?.clone();
                let current = percent(app.app.volume);
                let kind = if app.locked() {
                    RuleKind::None
                } else {
                    RuleKind::Lock(current)
                };
                // Lock is direct: it locks the volume where it is.
                let device = device.clone();
                self.close_popup();
                core.app_rule(&device, &app, kind);
                self.refresh(core);
                return None;
            }
            (PopupKind::App { device, key }, ToolAction::Launch) => {
                let app = self.view.app(device, key)?;
                let rule = app.rule.as_ref();
                let mode = rule.map_or(VolumeRuleMode::NONE, |r| r.volume_mode);
                let percent_now = percent(app.app.volume);
                for v in [10, 20, 30, 50, 75] {
                    let on = mode == VolumeRuleMode::LAUNCH
                        && rule.is_some_and(|r| r.volume_percent == v);
                    add(
                        &mut items,
                        checked(on),
                        format!("{v}%"),
                        Command::Rule(device.clone(), key.clone(), RuleKind::Launch(v)),
                    );
                }
                items.push(Item::Separator);
                add(
                    &mut items,
                    checked(false),
                    format!("Current volume ({percent_now}%)"),
                    Command::Rule(device.clone(), key.clone(), RuleKind::Launch(percent_now)),
                );
                items.push(Item::Separator);
                add(
                    &mut items,
                    checked(mode == VolumeRuleMode::NONE),
                    "No volume rule".into(),
                    Command::Rule(device.clone(), key.clone(), RuleKind::None),
                );
            }
            (PopupKind::App { device, key }, ToolAction::Focus) => {
                let app = self.view.app(device, key)?;
                let on = app.rule.as_ref().is_some_and(|r| r.focus_lost_enabled);
                let text = if on {
                    "Exclude this app when focus is lost"
                } else {
                    "Include this app when focus is lost"
                };
                add(
                    &mut items,
                    checked(on),
                    text.into(),
                    Command::Rule(device.clone(), key.clone(), RuleKind::FocusLost),
                );
            }
            (PopupKind::App { device, key }, ToolAction::HardMute) => {
                let app = self.view.app(device, key)?;
                let on = app.rule.as_ref().is_some_and(|r| r.hard_muted);
                let text = if on {
                    "Stop keeping muted"
                } else {
                    "Keep muted (survives restart)"
                };
                add(
                    &mut items,
                    checked(on),
                    text.into(),
                    Command::Rule(device.clone(), key.clone(), RuleKind::HardMute),
                );
            }
            (PopupKind::App { device, key }, ToolAction::Route) => {
                let app = self.view.app(device, key)?;
                let current = app.app.routed_device_id.clone();
                add(
                    &mut items,
                    checked(current.is_none()),
                    "Always use the default playback device".into(),
                    Command::Route(key.clone(), None),
                );
                items.push(Item::Separator);
                for d in &self.view.devices {
                    add(
                        &mut items,
                        checked(current.as_deref() == Some(d.device.id.as_str())),
                        d.device.name.clone(),
                        Command::Route(key.clone(), Some(d.device.id.clone())),
                    );
                }
            }
            (PopupKind::Device { device }, ToolAction::More) => {
                let device = device.clone();
                items = self.device_menu(&device, &mut commands);
            }
            _ => return None,
        }
        Some(MenuRequest {
            owner: self.window,
            items,
            commands: commands
                .into_iter()
                .map(super::MenuCommand::Flyout)
                .collect(),
            x,
            y,
            above: false,
        })
    }

    fn restore_menu(&self) -> Option<MenuRequest> {
        let device = self.top_device()?;
        let header = self.layout().header?;
        let (_, restore) = self.header_buttons(header);
        let mut commands = vec![];
        let items = Self::restore_items(device, &mut commands);
        let (x, y) = self.screen_below(restore);
        Some(MenuRequest {
            owner: self.window,
            items,
            commands: commands
                .into_iter()
                .map(super::MenuCommand::Flyout)
                .collect(),
            x,
            y,
            above: false,
        })
    }

    // ---------- keyboard ----------

    /// Rows the keyboard moves through: the popup's while one is open.
    fn focus_rows(&self) -> Vec<Row> {
        match self.popup_layout() {
            Some((_, rows)) => rows,
            None => self.layout().rows,
        }
    }

    fn focused(&self) -> Option<Row> {
        self.focus
            .and_then(|f| self.focus_rows().into_iter().nth(f))
    }

    /// Scrolls so the focused row is in view.
    fn reveal(&mut self) {
        if self.popup.is_some() {
            return;
        }
        let Some(row) = self.focused() else {
            return;
        };
        if row.rect.y < 0.0 {
            self.scroll += row.rect.y;
        } else if row.rect.bottom() > self.height {
            self.scroll += row.rect.bottom() - self.height;
        }
        self.scroll = self
            .scroll
            .clamp(0.0, (self.content_height() - self.height).max(0.0));
    }

    fn key(
        &mut self,
        core: &mut Core,
        theme: &Theme,
        vk: u16,
        m: Modifiers,
    ) -> Option<MenuRequest> {
        const VK_TAB: u16 = 0x09;
        const VK_ESCAPE: u16 = 0x1b;
        const VK_SPACE: u16 = 0x20;
        const VK_LEFT: u16 = 0x25;
        const VK_UP: u16 = 0x26;
        const VK_RIGHT: u16 = 0x27;
        const VK_DOWN: u16 = 0x28;
        const VK_ADD: u16 = 0x6b;
        const VK_SUBTRACT: u16 = 0x6d;
        const VK_OEM_PLUS: u16 = 0xbb;
        const VK_OEM_MINUS: u16 = 0xbd;
        const VK_OEM_PERIOD: u16 = 0xbe;
        let now = Instant::now();
        // Ctrl+Z / Ctrl+Y undo and redo the last volume or mute change.
        if m.ctrl && !m.alt && (vk == u16::from(b'Z') || vk == u16::from(b'Y')) {
            if core.replay(vk == u16::from(b'Z')) {
                self.refresh(core);
            }
            return None;
        }
        let rows = self.focus_rows().len();
        match vk {
            VK_ESCAPE => {
                if self.popup.is_some() {
                    self.close_popup();
                } else {
                    self.hide();
                }
            }
            VK_TAB | VK_UP | VK_DOWN if rows > 0 => {
                self.keyboard = true;
                let back = vk == VK_UP || (vk == VK_TAB && m.shift);
                let current = self.focus.unwrap_or(0);
                self.focus = Some(if back {
                    (current + rows - 1) % rows
                } else if self.focus.is_none() {
                    0
                } else {
                    (current + 1) % rows
                });
                self.reveal();
            }
            VK_RIGHT | VK_LEFT | VK_ADD | VK_SUBTRACT | VK_OEM_PLUS | VK_OEM_MINUS => {
                if vk == VK_LEFT || vk == VK_RIGHT {
                    self.keyboard = true;
                }
                let row = self.focused()?;
                if self.locked(&row.key) {
                    return None;
                }
                let delta = if matches!(vk, VK_RIGHT | VK_ADD | VK_OEM_PLUS) {
                    1
                } else {
                    -1
                };
                if let Some(value) = self.slider(&row.key).step(delta, now) {
                    self.apply_value(core, theme, &row.key, value);
                }
            }
            VK_OEM_PERIOD => {
                if let Some(row) = self.focused() {
                    self.toggle_mute(core, &row.key);
                }
            }
            _ if vk == u16::from(b'M') && !m.ctrl => {
                if let Some(row) = self.focused() {
                    self.toggle_mute(core, &row.key);
                }
            }
            VK_SPACE if self.popup.is_none() => {
                if let Some(row) = self.focused() {
                    match &row.key.app {
                        Some(app) => self.open_app_popup(&row.key.device, app),
                        None => self.open_device_popup(&row.key.device),
                    }
                }
            }
            _ => return None,
        }
        None
    }

    // ---------- events ----------

    /// Handles a window event; may ask for a menu.
    pub fn event(&mut self, core: &mut Core, theme: &Theme, event: Event) -> Option<MenuRequest> {
        if !self.visible {
            return None;
        }
        let now = Instant::now();
        let mut menu = None;
        match event {
            Event::MouseMove(x, y) => {
                self.pointer = Some((x, y));
                if let Some((key, rect)) = self.capture.clone() {
                    if !self.locked(&key) {
                        if let Some(value) = self.slider(&key).drag(x - rect.x, rect.w) {
                            self.apply_value(core, theme, &key, value);
                        }
                    }
                } else if let Some((start_y, start_scroll)) = self.scroll_drag {
                    let content = self.content_height();
                    let range = (content - self.height).max(1.0);
                    let track = self.height - self.height * self.height / content;
                    self.scroll =
                        (start_scroll + (y - start_y) * range / track.max(1.0)).clamp(0.0, range);
                } else {
                    let hit = self.hit(x, y);
                    if hit == self.hover {
                        return None;
                    }
                    self.hover = hit;
                }
            }
            Event::MouseLeave => {
                self.pointer = None;
                self.hover = None;
            }
            Event::MouseDown(Button::Left, x, y, _) => {
                self.keyboard = false;
                let hit = self.hit(x, y);
                self.pressed = hit.clone();
                match hit {
                    Some(Hit::Slider(key)) if !self.locked(&key) => {
                        let rect = self.slider_rect_of(&key);
                        if let Some(rect) = rect {
                            if let Some(index) = self.focus_rows().iter().position(|r| r.key == key)
                            {
                                self.focus = Some(index);
                            }
                            let value =
                                self.slider(&key)
                                    .press(x - rect.x, y - rect.y, rect.w, theme, now);
                            if let Some(value) = value {
                                self.apply_value(core, theme, &key, value);
                            }
                            self.capture = Some((key, rect));
                            self.window.capture();
                            self.window.set_timer(FRAME, 15);
                        }
                    }
                    Some(Hit::Scrollbar) => {
                        self.scroll_drag = Some((y, self.scroll));
                        self.window.capture();
                    }
                    _ => {}
                }
            }
            Event::MouseUp(Button::Left, x, y, m) => {
                if let Some((key, _)) = self.capture.take() {
                    self.slider(&key).release(now);
                    self.window.release_capture();
                    self.pressed = None;
                    self.paint(theme);
                    return None;
                }
                if self.scroll_drag.take().is_some() {
                    self.window.release_capture();
                    return None;
                }
                let hit = self.hit(x, y);
                let pressed = self.pressed.take();
                if hit.is_some() && hit == pressed {
                    menu = self.click(core, hit.unwrap_or(Hit::Header), m);
                }
            }
            Event::MouseDown(Button::Middle, x, y, _) => {
                // Middle-click on an app's icon hides it on this device.
                if let Some(Hit::Mute(key)) = self.hit(x, y) {
                    if let (Some(app), false) = (&key.app, key.popup) {
                        let (device, app) = (key.device.clone(), app.clone());
                        self.hide_app(&device, &app);
                    }
                }
            }
            Event::MouseUp(Button::Right, x, y, _) => match self.hit(x, y) {
                Some(Hit::Mute(key) | Hit::Slider(key) | Hit::Row(key)) if !key.popup => {
                    match &key.app {
                        Some(app) => self.open_app_popup(&key.device, &app.clone()),
                        None => self.open_device_popup(&key.device),
                    }
                }
                Some(Hit::DeviceTitle(device)) => self.open_device_popup(&device),
                Some(Hit::Expand | Hit::Header) => {
                    // The original's header menu: its handler never matches,
                    // so it does nothing.
                    let (cx, cy) = wnd::cursor_pos();
                    menu = Some(MenuRequest {
                        owner: self.window,
                        items: vec![Item::command("Set as default device", 1)],
                        commands: vec![super::MenuCommand::Flyout(Command::Nothing)],
                        x: cx,
                        y: cy,
                        above: false,
                    });
                }
                _ => {}
            },
            Event::Wheel(delta, x, y) => {
                let notch = if delta > 0 { 1 } else { -1 };
                match self.hit(x, y) {
                    Some(Hit::Slider(key)) if !self.locked(&key) => {
                        if let Some(value) = self.slider(&key).step(2 * notch, now) {
                            self.apply_value(core, theme, &key, value);
                        }
                    }
                    Some(Hit::Slider(_)) => {}
                    _ if self.popup.is_none() && self.scrollable() => {
                        let range = (self.content_height() - self.height).max(0.0);
                        self.scroll =
                            (self.scroll - f32::from(delta) / 120.0 * 48.0).clamp(0.0, range);
                        if let Some((px, py)) = self.pointer {
                            self.hover = self.hit(px, py);
                        }
                    }
                    _ => {}
                }
            }
            Event::Key(vk, m) => menu = self.key(core, theme, vk, m),
            Event::Activate(false) => {
                if !self.pinned && self.closing.is_none() {
                    self.hidden_at = Some(now);
                    self.hide();
                }
            }
            Event::CaptureLost => {
                if let Some((key, _)) = self.capture.take() {
                    self.slider(&key).release(now);
                }
                self.scroll_drag = None;
            }
            _ => return None,
        }
        if self.visible {
            self.paint(theme);
        }
        menu
    }

    fn slider_rect_of(&self, key: &SliderKey) -> Option<Rect> {
        let rows = if key.popup {
            self.popup_layout()?.1
        } else {
            self.layout().rows
        };
        rows.into_iter()
            .find(|r| &r.key == key)
            .map(|r| Self::slider_rect(r.rect))
    }

    fn click(&mut self, core: &mut Core, hit: Hit, m: Modifiers) -> Option<MenuRequest> {
        match hit {
            Hit::Expand => {
                self.expanded = !self.expanded;
                core.store.set(Key::IsExpanded, Value::Bool(self.expanded));
                self.chevron = Some(Instant::now());
                self.opened = Instant::now();
                self.scroll = 0.0;
                self.sync();
                self.place();
                self.window.set_timer(FRAME, 15);
            }
            Hit::Pin => self.pinned = !self.pinned,
            Hit::Restore => return self.restore_menu(),
            Hit::Mute(key) if m.ctrl && key.app.is_some() && !key.popup => {
                self.solo(core, &key.device, key.app.as_deref().unwrap_or_default());
            }
            Hit::Mute(key) => self.toggle_mute(core, &key),
            Hit::Row(key) if m.ctrl && key.app.is_some() && key.pid.is_none() && !key.popup => {
                self.solo(core, &key.device, key.app.as_deref().unwrap_or_default());
            }
            Hit::Dismiss => self.close_popup(),
            Hit::Tool(i) => return self.tool(core, i),
            _ => {}
        }
        None
    }

    // ---------- debug renders ----------

    /// Draws the flyout as it would open (animations finished) without
    /// showing it; with `popup`, the first app's popup is open.
    pub fn render(
        &mut self,
        core: &mut Core,
        theme: &Theme,
        popup: bool,
    ) -> Option<(u32, u32, Vec<u8>)> {
        self.view = core.view();
        self.sync();
        self.place();
        self.visible = true;
        let past = Instant::now() - Duration::from_secs(5);
        self.opened = past;
        if popup {
            let first = self
                .layout()
                .rows
                .into_iter()
                .find_map(|r| r.key.app.map(|a| (r.key.device, a)));
            if let Some((device, app)) = first {
                self.open_app_popup(&device, &app);
                if let Some(p) = &mut self.popup {
                    p.opened = past;
                }
            }
        }
        self.paint(theme);
        self.visible = false;
        self.popup = None;
        self.surface.as_ref()?.rgba()
    }

    // ---------- painting ----------

    fn icon(&mut self, path: &str) -> Option<Rc<Image>> {
        let size = (28.0 * self.scale).round() as u32;
        self.icons
            .entry((path.into(), size))
            .or_insert_with(|| {
                icons::bgra(path, size).map(|pixels| Image::from_straight_bgra(size, size, pixels))
            })
            .clone()
    }

    pub fn paint(&mut self, theme: &Theme) {
        let now = Instant::now();
        let width = (WIDTH * self.scale).round() as i32;
        let height = (self.height * self.scale).round() as i32;
        // Icons are loaded before drawing (the canvas borrows the surface).
        let paths: Vec<String> = self
            .shown()
            .iter()
            .flat_map(|d| d.apps.iter())
            .filter_map(|a| a.app.executable.clone())
            .collect();
        for path in paths {
            self.icon(&path);
        }
        let opacity = match self.closing {
            Some(start) => 1.0 - EASE_IN.at(anim::progress(start, 0.0, 105.0, now)),
            None => FADE_OUT_QUAD.at(anim::progress(self.opened, 0.0, 165.0, now)),
        };
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        surface.draw(width, height, self.scale, |canvas| {
            self.draw(canvas, theme, now)
        });
        surface.present(self.window.raw(), self.position.0, self.position.1, opacity);
        self.surface = Some(surface);
    }

    fn draw(&self, canvas: &mut Canvas, theme: &Theme, now: Instant) {
        let full = Rect::new(0.0, 0.0, WIDTH, self.height);
        // Transparency effects off: the solid DarkChromeLow background.
        // Otherwise the content tint (at least an all-but-invisible fill,
        // which keeps the window clickable) over the acrylic.
        if theme.transparency {
            canvas.rect(full, theme.flyout_tint);
        } else {
            canvas.rect(full, theme.opaque_bg);
        }
        // Open: scale 0.92 -> 1 (220 ms, BackEase); close: 8 DIP down.
        let (dy, scale) = match self.closing {
            Some(start) => (
                8.0 * EASE_IN.at(anim::progress(start, 0.0, 105.0, now)),
                1.0,
            ),
            None => (
                0.0,
                anim::lerp(
                    0.92,
                    1.0,
                    BACK_OUT.at(anim::progress(self.opened, 0.0, 220.0, now)),
                ),
            ),
        };
        canvas.push_transform(0.0, dy, scale, WIDTH / 2.0, self.height);
        if self.view.devices.is_empty() {
            let style = TextStyle::new(UI_FONT, 13.0)
                .align(Align::Center)
                .wrap()
                .top();
            canvas.text(
                NO_DEVICES,
                Rect::new(24.0, 24.0, WIDTH - 48.0, self.height),
                &style,
                theme.text,
            );
        } else {
            self.draw_list(canvas, theme, now);
        }
        self.draw_popup(canvas, theme, now);
        canvas.pop_transform();
    }

    /// Row cascade on open: opacity and +10 -> 0, 260 ms CubicEase EaseOut,
    /// delayed min(i, 24) x 28 ms.
    fn cascade(&self, index: usize, now: Instant) -> f32 {
        let delay = index.min(24) as f32 * 28.0;
        EASE_OUT.at(anim::progress(self.opened, delay, 260.0, now))
    }

    fn push_cascade(&self, canvas: &mut Canvas, index: usize, now: Instant) -> bool {
        let t = self.cascade(index, now);
        if t >= 1.0 {
            return false;
        }
        canvas.push_layer(t, None);
        canvas.push_transform(0.0, 10.0 * (1.0 - t), 1.0, 0.0, 0.0);
        true
    }

    fn pop_cascade(canvas: &mut Canvas, pushed: bool) {
        if pushed {
            canvas.pop_transform();
            canvas.pop();
        }
    }

    fn draw_list(&self, canvas: &mut Canvas, theme: &Theme, now: Instant) {
        let layout = self.layout();
        let title = TextStyle::new(UI_FONT, 15.0).ellipsis();
        for block in &layout.blocks {
            let pushed = self.push_cascade(canvas, block.cascade, now);
            canvas.rect(block.rect, theme.accent_30);
            if let Some(name) = &block.title {
                canvas.text(
                    name,
                    Rect::new(12.0, block.rect.y, WIDTH - 24.0, TITLE_H),
                    &title,
                    theme.text,
                );
            }
            if let Some(row) = layout
                .rows
                .iter()
                .find(|r| r.key == SliderKey::device(&block.device, false))
            {
                self.draw_device_row(canvas, theme, row);
            }
            Self::pop_cascade(canvas, pushed);
        }
        for row in layout.rows.iter().filter(|r| r.key.app.is_some()) {
            let pushed = self.push_cascade(canvas, row.cascade, now);
            let hiding = self
                .hiding_app
                .as_ref()
                .filter(|(d, k, _)| Some(k) == row.key.app.as_ref() && d == &row.key.device);
            if let Some((_, _, start)) = hiding {
                // Hide: fade, 12 DIP left and a slight shrink, 140 ms.
                let t = Curve(0.23, 1.0, 0.32, 1.0).at(anim::progress(*start, 0.0, 140.0, now));
                canvas.push_layer(1.0 - t, None);
                let (cx, cy) = row.rect.center();
                canvas.push_transform(-12.0 * t, 0.0, 1.0 - 0.015 * t, cx, cy);
                self.draw_app_row(canvas, theme, row);
                canvas.pop_transform();
                canvas.pop();
            } else {
                self.draw_app_row(canvas, theme, row);
            }
            Self::pop_cascade(canvas, pushed);
        }
        if let Some(header) = layout.header {
            self.draw_header(canvas, theme, header, now);
        }
        if self.popup.is_none() {
            self.draw_focus(canvas, theme, &layout.rows);
        }
        if self.scrollable() {
            let content = layout.content;
            let thumb_h = (self.height * self.height / content).max(24.0);
            let range = (content - self.height).max(1.0);
            let y = self.scroll / range * (self.height - thumb_h);
            let wide = matches!(self.hover, Some(Hit::Scrollbar)) || self.scroll_drag.is_some();
            let w = if wide { 8.0 } else { 2.0 };
            let x = WIDTH - 8.0 - w / 2.0;
            canvas.round_rect(
                Rect::new(x, y + 4.0, w, thumb_h - 8.0),
                w / 2.0,
                theme.text.alpha(0.4),
            );
        }
    }

    fn draw_focus(&self, canvas: &mut Canvas, theme: &Theme, rows: &[Row]) {
        if !self.keyboard {
            return;
        }
        if let Some(row) = self.focus.and_then(|f| rows.get(f)) {
            // 2 px Text over 3 px background, inset.
            canvas.stroke_rect(row.rect, 2.0, theme.text);
            canvas.stroke_rect(row.rect.inset(2.0, 2.0), 1.0, theme.popup_bg);
        }
    }

    fn draw_header(&self, canvas: &mut Canvas, theme: &Theme, header: Rect, now: Instant) {
        let enabled = self.visible_count() > 1;
        if enabled {
            match (&self.hover, &self.pressed) {
                (Some(Hit::Expand), Some(Hit::Expand)) => canvas.rect(header, theme.pressed),
                (Some(Hit::Expand), _) => canvas.rect(header, theme.hover),
                _ => {}
            }
        }
        let bar = if self.scrollable() { 16.0 } else { 0.0 };
        if let Some(top) = self.top_device() {
            let name = TextStyle::new(UI_FONT, 15.0).ellipsis();
            canvas.text(
                &top.device.name,
                Rect::new(12.0, header.y, WIDTH - 12.0 - 88.0 - bar, TITLE_H),
                &name,
                theme.text,
            );
        }
        let icons = TextStyle::new(theme.icon_font, 12.0).align(Align::Center);
        if enabled {
            // Chevron: rotates to 180 when expanded (220 ms).
            let t = self
                .chevron
                .map_or(1.0, |s| EASE_OUT.at(anim::progress(s, 0.0, 220.0, now)));
            let (from, to) = if self.expanded {
                (0.0, 180.0)
            } else {
                (180.0, 0.0)
            };
            let angle = anim::lerp(from, to, t);
            let rect = Rect::new(WIDTH - 12.0 - 12.0 - bar, header.y, 12.0, TITLE_H);
            let (cx, cy) = rect.center();
            canvas.push_rotation(angle, cx, cy);
            canvas.text(&glyph(GLYPH_CHEVRON), rect, &icons, theme.text);
            canvas.pop_transform();
        }
        let (pin, restore) = self.header_buttons(header);
        let small = TextStyle::new(theme.icon_font, 11.0).align(Align::Center);
        let button = |canvas: &mut Canvas, rect: Rect, hit: Hit, c: char| {
            if self.hover.as_ref() == Some(&hit) {
                canvas.rect(rect, theme.hover);
            }
            canvas.text(&glyph(c), rect, &small, theme.text);
        };
        if self.top_device().is_some_and(|d| !d.hidden_apps.is_empty()) {
            button(canvas, restore, Hit::Restore, GLYPH_RESTORE);
        }
        button(
            canvas,
            pin,
            Hit::Pin,
            if self.pinned { GLYPH_PINNED } else { GLYPH_PIN },
        );
    }

    fn draw_device_row(&self, canvas: &mut Canvas, theme: &Theme, row: &Row) {
        let Some(device) = self.view.device(&row.key.device) else {
            return;
        };
        let d = &device.device;
        let mute = Self::mute_rect(row.rect);
        let icons = TextStyle::new(theme.icon_font, 24.0).align(Align::Center);
        let hovered = self.hover.as_ref() == Some(&Hit::Mute(row.key.clone()));
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
        let pressed = self.pressed.as_ref() == Some(&Hit::Mute(row.key.clone())) && hovered;
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
        if let Some(slider) = self.sliders.get(&row.key) {
            slider.paint(canvas, Self::slider_rect(row.rect), theme, 1.0);
            let value = TextStyle::new(UI_FONT, 24.0).align(Align::Center);
            canvas.text(
                &slider.value.to_string(),
                Self::value_rect(row.rect),
                &value,
                theme.text,
            );
        }
    }

    fn draw_app_row(&self, canvas: &mut Canvas, theme: &Theme, row: &Row) {
        let Some(app_key) = &row.key.app else {
            return;
        };
        let Some(app) = self.view.app(&row.key.device, app_key) else {
            return;
        };
        let muted = self.volume_of(&row.key).is_some_and(|(_, m)| m);
        let locked = app.locked();
        let faded = if muted { 0.4 } else { 1.0 };
        let cell = Self::mute_rect(row.rect);
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
                    let name = match row.key.pid {
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
        if let Some(slider) = self.sliders.get(&row.key) {
            let opacity = faded * if locked { 0.5 } else { 1.0 };
            slider.paint(canvas, Self::slider_rect(row.rect), theme, opacity);
            let value = TextStyle::new(UI_FONT, 18.0).align(Align::Center);
            canvas.text(
                &slider.value.to_string(),
                Self::value_rect(row.rect),
                &value,
                theme.text.alpha(faded),
            );
        }
    }

    fn draw_popup(&self, canvas: &mut Canvas, theme: &Theme, now: Instant) {
        let full = Rect::new(0.0, 0.0, WIDTH, self.height);
        // The dimmed flyout behind the popup fades in and out (150 ms).
        let dismiss = match (&self.popup, self.dismiss_out) {
            (Some(p), _) => anim::progress(p.opened, 0.0, 150.0, now),
            (None, Some(start)) => 1.0 - anim::progress(start, 0.0, 150.0, now),
            _ => 0.0,
        };
        if dismiss > 0.0 {
            canvas.rect(full, theme.dismiss.alpha(dismiss));
        }
        let Some(popup) = &self.popup else {
            return;
        };
        let Some((frame, rows)) = self.popup_layout() else {
            return;
        };
        let fade = anim::progress(popup.opened, 0.0, 150.0, now);
        canvas.push_layer(fade, None);
        canvas.rect(frame, theme.popup_bg);
        canvas.stroke_rect(frame, 1.0, theme.accent);
        let header = Rect::new(frame.x + 1.0, frame.y + 1.0, frame.w - 2.0, TITLE_H);
        canvas.rect(header, theme.accent_30);
        let tools = self.tool_rects(frame);
        let title_right = tools.first().map_or(header.right(), |r| r.x);
        canvas.text(
            &popup.title,
            Rect::new(
                header.x + 12.0,
                header.y,
                (title_right - header.x - 24.0).max(0.0),
                TITLE_H,
            ),
            &TextStyle::new(UI_FONT, 15.0).ellipsis(),
            theme.text,
        );
        for (i, (tool, rect)) in popup.tools.iter().zip(&tools).enumerate() {
            if tool.active {
                canvas.rect(*rect, theme.accent_22);
            }
            match (&self.hover, &self.pressed) {
                (Some(Hit::Tool(h)), Some(Hit::Tool(p))) if *h == i && *p == i => {
                    canvas.rect(*rect, theme.pressed)
                }
                (Some(Hit::Tool(h)), _) if *h == i => canvas.rect(*rect, theme.hover),
                _ => {}
            }
            let size = if tool.action == ToolAction::Close {
                10.0
            } else {
                16.0
            };
            let style = TextStyle::new(theme.icon_font, size).align(Align::Center);
            canvas.text(&glyph(tool.glyph), *rect, &style, theme.text);
        }
        for row in &rows {
            if row.key.app.is_some() {
                self.draw_app_row(canvas, theme, row);
            } else {
                self.draw_device_row(canvas, theme, row);
            }
        }
        self.draw_focus(canvas, theme, &rows);
        canvas.pop();
    }
}

const NO_DEVICES: &str = "It doesn't look like you have any playback devices.";
