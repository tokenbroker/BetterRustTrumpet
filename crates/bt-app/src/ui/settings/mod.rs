//! The settings window (the original's WebSettingsWindow, native): a
//! normal resizable window with a taskbar button, Mica behind it on
//! Windows 11, a sidebar of pages grouped like the original's and a
//! scrolling page of setting cards. Every change is written through the
//! Store at once. Closing destroys the window and its pixels.
mod lists;
mod pages;
mod themes;
mod widgets;

pub use pages::PageId;

use self::{
    pages::{
        Action, Button as ButtonData, Control, Expand, Field, HotkeyTarget, PageState, Section,
        Texts, Unit,
    },
    widgets::{DISPLAY, Palette, State, label_style},
};
use super::{MenuCommand, MenuRequest};
use crate::core::Core;
use bt_app::i18n::{tr, trf};
use bt_app::win32::{
    gfx::{Align, Canvas, Gfx, Image, Rect, Rgba, Surface, TextStyle},
    icons,
    menu::Item,
    theme as system,
    wnd::{self, Button, Event, Modifiers, Window},
};
use bt_core::models::HotkeyData;
use std::rc::Rc;

/// The original's window size and minimum (WebSettingsWindow.xaml).
const SIZE: (f32, f32) = (960.0, 680.0);
const MIN: (f32, f32) = (700.0, 480.0);
const SIDEBAR: f32 = 260.0;
const PAD: f32 = 40.0;
const MAX_CONTENT: f32 = 860.0;
const PICKER_H: f32 = 196.0;
const CARET: usize = 1;
/// Detail rows of an open list item sit this far in.
const INDENT: f32 = 28.0;
/// The expand chevron's column.
const CHEVRON: f32 = 28.0;

// Virtual keys.
const VK_BACK: u16 = 0x08;
const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0d;
const VK_ESCAPE: u16 = 0x1b;
const VK_SPACE: u16 = 0x20;
const VK_PRIOR: u16 = 0x21;
const VK_NEXT: u16 = 0x22;
const VK_END: u16 = 0x23;
const VK_HOME: u16 = 0x24;
const VK_LEFT: u16 = 0x25;
const VK_UP: u16 = 0x26;
const VK_RIGHT: u16 = 0x27;
const VK_DOWN: u16 = 0x28;
const VK_DELETE: u16 = 0x2e;
/// Shift, Ctrl, Alt (any side), Win and the IME/lock keys never complete
/// a shortcut on their own.
const MODIFIER_KEYS: [u16; 11] = [
    0x10, 0x11, 0x12, 0x5b, 0x5c, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowRef {
    section: usize,
    row: usize,
}

/// A clickable or focusable part of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Main,
    Item(usize),
    Delete(usize),
    Clear,
    Sv,
    Hue,
    Hex,
    /// A text box.
    Name,
    /// A trailing button.
    Trail(usize),
    /// The line of an expandable list item.
    Header,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Nav(PageId),
    Row(RowRef, Part),
    Scrollbar,
}

/// A drop-down choice, delivered through the app's menu commands.
#[derive(Clone, Debug)]
pub struct Command {
    page: PageId,
    row: RowRef,
    index: usize,
}

/// What the app should do after a settings event.
#[derive(Default)]
pub struct Outcome {
    pub menu: Option<MenuRequest>,
    /// Appearance settings changed: reload the flyout theme.
    pub theme: bool,
    /// A shortcut changed: register the hotkeys again.
    pub hotkeys: bool,
    /// Shortcut recording started (true: suspend the app's hotkeys) or
    /// ended.
    pub capture: Option<bool>,
    /// Apply this QuickTrumpet preset (with its confirmation).
    pub apply_preset: Option<usize>,
    pub close: bool,
}

struct Picker {
    row: RowRef,
    key: &'static str,
    h: f32,
    s: f32,
    v: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Editing {
    Field(Field),
    Hex,
}

struct RowLayout {
    at: RowRef,
    /// The row's first line (its picker hangs below).
    line: Rect,
    label: Rect,
    desc: Option<Rect>,
    parts: Vec<(Part, Rect)>,
    badge: Option<Rect>,
    chips: Vec<Rect>,
    chevron: Option<Rect>,
}

#[derive(Default)]
struct Layout {
    view: Rect,
    content: f32,
    title: Rect,
    subtitle: Rect,
    /// Title, description and card of each section.
    sections: Vec<(Rect, Option<Rect>, Rect)>,
    rows: Vec<RowLayout>,
    groups: Vec<(&'static str, Rect)>,
    nav: Vec<(PageId, Rect)>,
}

pub struct Settings {
    pub window: Window,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    icon_font: &'static str,
    app_icon: Option<Rc<Image>>,
    mica: bool,
    palette: Palette,
    scale: f32,
    size: (f32, f32),
    page: PageId,
    sections: Vec<Section>,
    layout: Layout,
    scroll: f32,
    hover: Option<Target>,
    pressed: Option<Target>,
    focus: Option<Target>,
    /// The focus ring shows after keyboard use.
    keyboard: bool,
    /// A slider being dragged and its live value.
    slider: Option<(RowRef, f64)>,
    scroll_drag: Option<(f32, f32)>,
    picker: Option<Picker>,
    picker_drag: Option<Part>,
    recording: Option<HotkeyTarget>,
    editing: Option<Editing>,
    texts: Texts,
    hex: String,
    surrogate: Option<u16>,
    caret: bool,
    /// A custom theme whose delete button was clicked once.
    armed: Option<usize>,
    /// A two-step button clicked once, by its action.
    confirming: Option<Action>,
    /// Open list items and the capture form.
    state: PageState,
}

fn palette(mica: bool) -> Palette {
    let accent = Rgba::hex(&system::accent()).unwrap_or(Rgba::rgb(0, 120, 215));
    let dark1 = Rgba::hex(&system::accent_dark1()).unwrap_or(Rgba::rgb(0, 90, 158));
    Palette::new(!system::apps_light(), mica, accent, dark1)
}

fn hex_of(color: Rgba) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        byte(color.r),
        byte(color.g),
        byte(color.b)
    )
}

fn format_value(value: f64, unit: Unit) -> String {
    match unit {
        Unit::Percent => format!("{value:.0}%"),
        Unit::Ms => format!("{value:.0} ms"),
        Unit::Seconds => {
            let text = format!("{value:.2}");
            format!("{} s", text.trim_end_matches('0').trim_end_matches('.'))
        }
        Unit::OutOf10 => format!("{value:.0}/10"),
    }
}

impl Settings {
    pub fn new(
        gfx: Rc<Gfx>,
        core: &mut Core,
        icon_font: &'static str,
    ) -> windows::core::Result<Self> {
        let window = wnd::create_app(tr("SettingsWindowText"), SIZE, MIN)?;
        let mica = window.set_app_frame(!system::apps_light());
        let app_icon = std::env::current_exe().ok().and_then(|exe| {
            let size = (24.0 * window.scale()).round() as u32;
            icons::bgra(&exe.to_string_lossy(), size)
                .map(|px| Image::from_straight_bgra(size, size, px))
        });
        let mut settings = Self {
            window,
            surface: Some(Surface::new(gfx.clone())?),
            gfx,
            icon_font,
            app_icon,
            mica,
            palette: palette(mica),
            scale: window.scale(),
            size: SIZE,
            page: PageId::General,
            sections: vec![],
            layout: Layout::default(),
            scroll: 0.0,
            hover: None,
            pressed: None,
            focus: None,
            keyboard: false,
            slider: None,
            scroll_drag: None,
            picker: None,
            picker_drag: None,
            recording: None,
            editing: None,
            texts: Texts::default(),
            hex: String::new(),
            surrogate: None,
            caret: true,
            armed: None,
            confirming: None,
            state: PageState::default(),
        };
        settings.rebuild(core);
        Ok(settings)
    }

    /// Shows the window, or brings it back to the front.
    pub fn present(&mut self) {
        self.window.present_app();
    }

    /// Destroys the window and its surface.
    pub fn close(mut self) {
        self.surface = None;
        self.window.destroy();
    }

    pub fn recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Shows values the app changed (an applied preset's badge).
    pub fn refresh(&mut self, core: &mut Core) {
        self.rebuild(core);
        self.paint();
    }

    /// Re-reads every value shown from the settings.
    fn rebuild(&mut self, core: &mut Core) {
        self.sections = pages::build(self.page, core, &self.state);
        if let Some(p) = &self.picker {
            let still = matches!(
                self.control(p.row),
                Some(Control::Color { key, .. }) if *key == p.key
            );
            if !still {
                self.picker = None;
            }
        }
    }

    fn row(&self, at: RowRef) -> Option<&pages::Row> {
        self.sections.get(at.section)?.rows.get(at.row)
    }

    fn control(&self, at: RowRef) -> Option<&Control> {
        self.row(at).map(|r| &r.control)
    }

    fn navigate(&mut self, core: &mut Core, page: PageId) {
        if self.page != page {
            self.page = page;
            self.scroll = 0.0;
            self.picker = None;
            self.editing = None;
            self.armed = None;
            self.confirming = None;
            self.state.expanded = None;
        }
        self.rebuild(core);
    }

    // ---------- layout ----------

    fn measure_wrapped(&self, text: &str, style: &TextStyle, width: f32) -> f32 {
        if text.is_empty() {
            0.0
        } else {
            self.gfx.measure_wrapped(text, style, width.max(1.0)).ceil()
        }
    }

    fn button_width(&self, label: &str, glyph: bool) -> f32 {
        if label.is_empty() {
            return 32.0;
        }
        let (w, _) = self.gfx.measure(label, &label_style(14.0));
        (w + 24.0 + if glyph { 24.0 } else { 0.0 }).ceil().max(96.0)
    }

    /// Whether a two-step button waits for its second click.
    fn is_confirming(&self, b: &ButtonData) -> bool {
        b.confirm.is_some() && self.confirming.as_ref() == Some(&b.action)
    }

    /// A button's label and glyph as drawn now.
    fn button_face<'a>(&self, b: &'a ButtonData) -> (&'a str, Option<&'static str>) {
        match b.confirm {
            Some(text) if self.is_confirming(b) => (text, None),
            _ => (&b.label, b.glyph),
        }
    }

    fn data_button_width(&self, b: &ButtonData) -> f32 {
        let (label, glyph) = self.button_face(b);
        self.button_width(label, glyph.is_some())
    }

    fn buttons_width(&self, buttons: &[ButtonData]) -> f32 {
        buttons
            .iter()
            .map(|b| self.data_button_width(b) + 8.0)
            .sum::<f32>()
    }

    /// A pill's width; long ones are cut off with an ellipsis.
    fn chip_width(&self, text: &str) -> f32 {
        (self.gfx.measure(text, &label_style(12.0)).0 + 20.0)
            .ceil()
            .min(220.0)
    }

    /// Width of a control on the right of its row (0 for full-width ones).
    fn control_width(&self, row: &pages::Row) -> f32 {
        match &row.control {
            Control::None | Control::Themes(_) | Control::Check(..) | Control::Heading => 0.0,
            Control::Input { buttons, .. } => {
                if row.label.is_empty() {
                    0.0
                } else {
                    240.0 + 8.0 + self.buttons_width(buttons)
                }
            }
            Control::Toggle(..) => 40.0,
            Control::Slider { .. } => 272.0,
            Control::Choice { .. } => 240.0,
            Control::Hotkey { .. } => 252.0,
            Control::Color { .. } => 136.0,
            Control::Buttons(buttons) => self.buttons_width(buttons),
            Control::Chip(text) => self.gfx.measure(text, &label_style(14.0)).0 + 24.0,
        }
    }

    fn compute_layout(&self) -> Layout {
        let (w, h) = self.size;
        let mut l = Layout {
            view: Rect::new(SIDEBAR, 0.0, (w - SIDEBAR).max(0.0), h),
            ..Layout::default()
        };
        // Sidebar: the app name, then the page groups.
        let mut y = 72.0;
        for (title, group) in pages::GROUPS {
            l.groups
                .push((tr(title), Rect::new(20.0, y, SIDEBAR - 40.0, 28.0)));
            y += 30.0;
            for &page in group {
                l.nav.push((page, Rect::new(8.0, y, SIDEBAR - 16.0, 36.0)));
                y += 38.0;
            }
            y += 10.0;
        }
        // The page.
        let cw = (l.view.w - 2.0 * PAD).clamp(200.0, MAX_CONTENT);
        let x = SIDEBAR + PAD;
        let mut y = 28.0 - self.scroll;
        l.title = Rect::new(x, y, cw, 40.0);
        y += 44.0;
        let sub = self.measure_wrapped(self.page.subtitle(), &label_style(14.0).wrap(), cw);
        l.subtitle = Rect::new(x, y, cw, sub);
        y += sub + 28.0;
        for (si, section) in self.sections.iter().enumerate() {
            let title = Rect::new(x, y, cw, 22.0);
            y += 24.0;
            let desc = section.desc.as_deref().map(|d| {
                let dh = self.measure_wrapped(d, &label_style(12.0).wrap(), cw);
                let r = Rect::new(x, y, cw, dh);
                y += dh + 2.0;
                r
            });
            y += 8.0;
            let top = y;
            for (ri, row) in section.rows.iter().enumerate() {
                let at = RowRef {
                    section: si,
                    row: ri,
                };
                let (row_layout, height) = self.layout_row(at, row, Rect::new(x, y, cw, 0.0));
                l.rows.push(row_layout);
                y += height;
            }
            l.sections
                .push((title, desc, Rect::new(x, top, cw, y - top)));
            y += 28.0;
        }
        l.content = y + self.scroll;
        l
    }

    fn layout_row(&self, at: RowRef, row: &pages::Row, frame: Rect) -> (RowLayout, f32) {
        let indent = if row.indent { INDENT } else { 0.0 };
        let x = frame.x + 16.0 + indent;
        let end = frame.right() - 16.0;
        let right_edge = end - if row.expand.is_some() { CHEVRON } else { 0.0 };
        let control_w = self.control_width(row);
        let trail_w = self.buttons_width(&row.trail);
        let chip_ws: Vec<f32> = row.chips.iter().map(|c| self.chip_width(c)).collect();
        let chips_w = chip_ws.iter().map(|w| w + 6.0).sum::<f32>();
        let check_w = if matches!(row.control, Control::Check(..)) {
            32.0
        } else {
            0.0
        };
        let badge_w = row
            .badge
            .as_deref()
            .map_or(0.0, |b| self.chip_width(b) + 8.0);
        let text_w = (right_edge
            - x
            - check_w
            - if control_w > 0.0 {
                control_w + 24.0
            } else {
                0.0
            }
            - trail_w
            - if chips_w > 0.0 { chips_w + 6.0 } else { 0.0 })
        .max(80.0);
        let heading = matches!(row.control, Control::Heading);
        let title_style = if heading {
            label_style(12.0).weight(600).wrap().top()
        } else {
            label_style(14.0).weight(600).wrap().top()
        };
        let desc_style = label_style(12.0).wrap().top();
        let label_h = self.measure_wrapped(&row.label, &title_style, (text_w - badge_w).max(40.0));
        let desc_h = row
            .desc
            .as_deref()
            .map_or(0.0, |d| self.measure_wrapped(d, &desc_style, text_w));
        let text_h = label_h + if desc_h > 0.0 { desc_h + 2.0 } else { 0.0 };
        let mut line_h = if heading {
            36.0
        } else {
            (text_h + 26.0).max(if desc_h > 0.0 { 64.0 } else { 52.0 })
        };
        let mut parts = vec![];
        if matches!(row.control, Control::Themes(_)) {
            if let Control::Themes(chips) = &row.control {
                let (cw, ch, gap) = (184.0, 52.0, 8.0);
                let cols = (((frame.w - 32.0 + gap) / (cw + gap)).floor() as usize).max(1);
                let rows = chips.len().div_ceil(cols);
                line_h = 32.0 + rows as f32 * (ch + gap) - gap;
                for (i, chip) in chips.iter().enumerate() {
                    let r = Rect::new(
                        x + (i % cols) as f32 * (cw + gap),
                        frame.y + 16.0 + (i / cols) as f32 * (ch + gap),
                        cw,
                        ch,
                    );
                    parts.push((Part::Item(i), r));
                    if chip.custom == Some(true) {
                        let size = if self.armed == Some(i) { 72.0 } else { 24.0 };
                        parts.push((
                            Part::Delete(i),
                            Rect::new(r.right() - size - 6.0, r.y + 6.0, size, 24.0),
                        ));
                    }
                }
            }
        }
        if matches!(row.control, Control::Input { .. }) && row.label.is_empty() {
            line_h = 64.0;
        }
        let cy = frame.y + line_h / 2.0;
        // Trailing buttons, right to left; the control sits left of them.
        let mut right = right_edge;
        for (i, b) in row.trail.iter().enumerate().rev() {
            let bw = self.data_button_width(b);
            right -= bw;
            parts.push((Part::Trail(i), Rect::new(right, cy - 16.0, bw, 32.0)));
            right -= 8.0;
        }
        match &row.control {
            Control::Toggle(..) => {
                parts.push((Part::Main, Rect::new(right - 40.0, cy - 10.0, 40.0, 20.0)))
            }
            Control::Check(..) => parts.push((Part::Main, Rect::new(x, cy - 10.0, 20.0, 20.0))),
            Control::Slider { .. } => parts.push((
                Part::Main,
                Rect::new(right - 64.0 - 200.0, cy - 16.0, 200.0, 32.0),
            )),
            Control::Choice { .. } => {
                parts.push((Part::Main, Rect::new(right - 240.0, cy - 16.0, 240.0, 32.0)))
            }
            Control::Hotkey { target, text, .. } => {
                let clear = !text.is_empty() && self.recording.as_ref() != Some(target);
                let end = if clear { right - 36.0 } else { right };
                parts.push((Part::Main, Rect::new(end - 212.0, cy - 16.0, 212.0, 32.0)));
                if clear {
                    parts.push((Part::Clear, Rect::new(right - 32.0, cy - 16.0, 32.0, 32.0)));
                }
            }
            Control::Color { .. } => {
                parts.push((Part::Main, Rect::new(right - 48.0, cy - 14.0, 48.0, 28.0)))
            }
            Control::Buttons(buttons) => {
                // Action rows without a label start on the left.
                let mut bx = if row.label.is_empty() {
                    x
                } else {
                    right - control_w + 8.0
                };
                for (i, b) in buttons.iter().enumerate() {
                    let bw = self.data_button_width(b);
                    parts.push((Part::Item(i), Rect::new(bx, cy - 16.0, bw, 32.0)));
                    bx += bw + 8.0;
                }
            }
            Control::Input { buttons, .. } => {
                let buttons_w = self.buttons_width(buttons);
                let (bx, box_w) = if row.label.is_empty() {
                    (x, (right - x - buttons_w).clamp(120.0, 420.0))
                } else {
                    (right - control_w + 8.0, 240.0)
                };
                parts.push((Part::Name, Rect::new(bx, cy - 16.0, box_w, 32.0)));
                let mut bx = bx + box_w + 8.0;
                for (i, b) in buttons.iter().enumerate() {
                    let bw = self.data_button_width(b);
                    parts.push((Part::Item(i), Rect::new(bx, cy - 16.0, bw, 32.0)));
                    bx += bw + 8.0;
                }
            }
            _ => {}
        }
        // Chips sit left of the control.
        let mut chips = vec![];
        let mut cx = right
            - if control_w > 0.0 {
                control_w + 12.0
            } else {
                0.0
            };
        for w in chip_ws.iter().rev() {
            cx -= w;
            chips.push(Rect::new(cx, cy - 12.0, *w, 24.0));
            cx -= 6.0;
        }
        chips.reverse();
        let line = Rect::new(frame.x, frame.y, frame.w, line_h);
        let top = if heading {
            frame.y + 14.0
        } else {
            frame.y + ((line_h - text_h) / 2.0).max(13.0)
        };
        let label_x = x + check_w;
        let label = Rect::new(label_x, top, text_w, label_h);
        let desc = (desc_h > 0.0).then(|| Rect::new(label_x, top + label_h + 2.0, text_w, desc_h));
        let badge = row.badge.as_deref().map(|b| {
            let (w, _) = self.gfx.measure(&row.label, &title_style);
            let bw = self.chip_width(b);
            let bx = (label_x + w + 8.0).min(label_x + text_w - bw);
            Rect::new(bx, top - 1.0, bw, 22.0)
        });
        let chevron = row
            .expand
            .is_some()
            .then(|| Rect::new(end - CHEVRON + 4.0, cy - 12.0, 24.0, 24.0));
        if row.expand.is_some() {
            parts.push((Part::Header, line));
        }
        let mut height = line_h;
        if self.picker.as_ref().is_some_and(|p| p.row == at) {
            let sv = Rect::new(x, line.bottom() + 4.0, 240.0, 140.0);
            parts.push((Part::Sv, sv));
            parts.push((Part::Hue, Rect::new(x, sv.bottom() + 16.0, 240.0, 14.0)));
            parts.push((
                Part::Hex,
                Rect::new(x + 264.0, line.bottom() + 4.0, 140.0, 32.0),
            ));
            height += PICKER_H;
        }
        (
            RowLayout {
                at,
                line,
                label,
                desc,
                parts,
                badge,
                chips,
                chevron,
            },
            height,
        )
    }

    fn row_layout(&self, at: RowRef) -> Option<&RowLayout> {
        self.layout.rows.iter().find(|r| r.at == at)
    }

    fn part_rect(&self, target: Target) -> Option<Rect> {
        match target {
            Target::Nav(page) => self
                .layout
                .nav
                .iter()
                .find(|(p, _)| *p == page)
                .map(|(_, r)| *r),
            Target::Row(at, part) => self
                .row_layout(at)?
                .parts
                .iter()
                .find(|(p, _)| *p == part)
                .map(|(_, r)| *r),
            Target::Scrollbar => self.scrollbar(),
        }
    }

    fn max_scroll(&self) -> f32 {
        (self.layout.content - self.layout.view.h).max(0.0)
    }

    fn scrollbar(&self) -> Option<Rect> {
        let view = self.layout.view;
        let content = self.layout.content;
        if content <= view.h + 0.5 {
            return None;
        }
        let thumb = (view.h * view.h / content).max(32.0);
        let y = view.y + (view.h - thumb) * (self.scroll / self.max_scroll());
        Some(Rect::new(view.right() - 8.0, y, 6.0, thumb))
    }

    fn hit(&self, x: f32, y: f32) -> Option<Target> {
        if let Some(r) = self.scrollbar() {
            if r.inset(-2.0, 0.0).contains(x, y) {
                return Some(Target::Scrollbar);
            }
        }
        if x < SIDEBAR {
            return self
                .layout
                .nav
                .iter()
                .find(|(_, r)| r.contains(x, y))
                .map(|(p, _)| Target::Nav(*p));
        }
        if !self.layout.view.contains(x, y) {
            return None;
        }
        for row in &self.layout.rows {
            let enabled = self.row(row.at).is_some_and(|r| r.enabled);
            if !enabled {
                continue;
            }
            // Delete buttons sit on their chips: test them first.
            let mut parts: Vec<&(Part, Rect)> = row.parts.iter().collect();
            parts.sort_by_key(|(p, _)| !matches!(p, Part::Delete(_)));
            if let Some((part, _)) = parts.into_iter().find(|(_, r)| r.contains(x, y)) {
                return Some(Target::Row(row.at, *part));
            }
            // A toggle or checkbox row toggles anywhere on its line, like a
            // label.
            if row.line.contains(x, y)
                && matches!(
                    self.control(row.at),
                    Some(Control::Toggle(..) | Control::Check(..))
                )
            {
                return Some(Target::Row(row.at, Part::Main));
            }
        }
        None
    }

    /// Focus stops in order: the current page in the sidebar, then every
    /// control of the page.
    fn focusables(&self) -> Vec<Target> {
        let mut list = vec![Target::Nav(self.page)];
        for row in &self.layout.rows {
            if !self.row(row.at).is_some_and(|r| r.enabled) {
                continue;
            }
            for (part, _) in &row.parts {
                if !matches!(part, Part::Sv | Part::Hue) {
                    list.push(Target::Row(row.at, *part));
                }
            }
        }
        list
    }

    /// Scrolls the focused control into view.
    fn reveal(&mut self, target: Target) {
        let Some(r) = self.part_rect(target) else {
            return;
        };
        if !matches!(target, Target::Row(..)) {
            return;
        }
        let view = self.layout.view;
        if r.y < view.y + 16.0 {
            self.scroll = (self.scroll - (view.y + 16.0 - r.y)).max(0.0);
        } else if r.bottom() > view.bottom() - 16.0 {
            self.scroll =
                (self.scroll + r.bottom() - (view.bottom() - 16.0)).min(self.max_scroll());
        }
        self.layout = self.compute_layout();
    }

    // ---------- events ----------

    pub fn event(&mut self, core: &mut Core, event: Event) -> Outcome {
        let mut out = Outcome::default();
        match event {
            Event::Paint | Event::Resized => {}
            Event::Close => {
                out.close = true;
                return out;
            }
            Event::DpiChanged => {
                self.scale = self.window.scale();
            }
            Event::SettingChange => {
                self.palette = palette(self.mica);
                self.mica = self.window.set_app_frame(self.palette.dark);
                self.palette = palette(self.mica);
            }
            Event::Timer(CARET) => {
                if self.editing.is_some() {
                    self.caret = !self.caret;
                } else {
                    self.window.kill_timer(CARET);
                }
            }
            Event::MouseMove(x, y) => self.mouse_move(x, y),
            Event::MouseLeave => {
                if self.slider.is_none() && self.picker_drag.is_none() {
                    self.hover = None;
                }
            }
            Event::MouseDown(Button::Left, x, y, _) => self.mouse_down(x, y, &mut out),
            Event::MouseUp(Button::Left, x, y, _) => self.mouse_up(core, x, y, &mut out),
            Event::Wheel(delta, x, y) => {
                self.scroll =
                    (self.scroll - f32::from(delta) / 120.0 * 64.0).clamp(0.0, self.max_scroll());
                self.layout = self.compute_layout();
                self.hover = self.hit(x, y);
            }
            Event::Key(vk, m) => self.key(core, vk, m, &mut out),
            Event::Char(c) => self.char(core, c, &mut out),
            Event::Activate(false) => {
                if self.recording.take().is_some() {
                    out.capture = Some(false);
                }
            }
            Event::CaptureLost => {
                self.slider = None;
                self.scroll_drag = None;
                self.picker_drag = None;
            }
            _ => return out,
        }
        self.paint();
        out
    }

    /// A drop-down choice was made.
    pub fn command(&mut self, core: &mut Core, command: Command) -> Outcome {
        let mut out = Outcome::default();
        if command.page == self.page {
            if let Some(Control::Choice { set, .. }) = self.control(command.row) {
                let set = set.clone();
                self.set_value(core, &set, command.index as f64, &mut out);
            }
        }
        self.paint();
        out
    }

    /// A setting changed: everything shown is re-read, and the flyout
    /// theme too (it caches appearance and behaviour settings).
    fn changed(&mut self, core: &mut Core, out: &mut Outcome) {
        out.theme = true;
        self.rebuild(core);
    }

    /// Writes a control's value; preset edits register shortcuts again.
    fn set_value(&mut self, core: &mut Core, set: &pages::Set, value: f64, out: &mut Outcome) {
        pages::apply(core, &mut self.state, set, value);
        if set.presets() {
            out.hotkeys = true;
        }
        self.changed(core, out);
    }

    fn mouse_move(&mut self, x: f32, y: f32) {
        if let Some((at, _)) = self.slider {
            if let Some(Target::Row(_, Part::Main)) = self.pressed {
                if let (Some(r), Some(Control::Slider { min, max, step, .. })) = (
                    self.part_rect(Target::Row(at, Part::Main)),
                    self.control(at),
                ) {
                    let value = slider_value(r, x, *min, *max, *step);
                    self.slider = Some((at, value));
                }
            }
            return;
        }
        if let Some(part) = self.picker_drag {
            self.drag_picker(part, x, y);
            return;
        }
        if let Some((start_y, start_scroll)) = self.scroll_drag {
            let view = self.layout.view;
            let thumb = self.scrollbar().map_or(view.h, |r| r.h);
            let range = (view.h - thumb).max(1.0);
            self.scroll = (start_scroll + (y - start_y) / range * self.max_scroll())
                .clamp(0.0, self.max_scroll());
            self.layout = self.compute_layout();
            return;
        }
        self.hover = self.hit(x, y);
    }

    fn drag_picker(&mut self, part: Part, x: f32, y: f32) {
        let Some(picker) = &self.picker else {
            return;
        };
        let Some(r) = self.part_rect(Target::Row(picker.row, part)) else {
            return;
        };
        let fx = ((x - r.x) / r.w).clamp(0.0, 1.0);
        let fy = ((y - r.y) / r.h).clamp(0.0, 1.0);
        if let Some(p) = &mut self.picker {
            match part {
                Part::Sv => {
                    p.s = fx;
                    p.v = 1.0 - fy;
                }
                Part::Hue => p.h = fx.min(0.999),
                _ => {}
            }
            self.hex = hex_of(widgets::hsv(p.h, p.s, p.v));
        }
    }

    fn mouse_down(&mut self, x: f32, y: f32, out: &mut Outcome) {
        self.keyboard = false;
        let target = self.hit(x, y);
        // A click anywhere else ends recording and text entry.
        if self.recording.is_some()
            && !matches!(target, Some(Target::Row(_, Part::Main)) if self.is_recording_target(target))
        {
            self.recording = None;
            out.capture = Some(false);
        }
        if let Some(editing) = self.editing {
            let keep = match editing {
                Editing::Field(_) => {
                    matches!(target, Some(Target::Row(_, Part::Name | Part::Item(_))))
                }
                Editing::Hex => matches!(target, Some(Target::Row(_, Part::Hex))),
            };
            if !keep {
                self.editing = None;
            }
        }
        if !matches!(target, Some(Target::Row(_, Part::Delete(_)))) {
            self.armed = None;
        }
        if self.confirming.is_some()
            && target
                .and_then(|t| self.target_button(t))
                .map(|b| &b.action)
                != self.confirming.as_ref()
        {
            self.confirming = None;
        }
        self.pressed = target;
        if let Some(t) = target {
            if !matches!(t, Target::Scrollbar) {
                self.focus = Some(t);
            }
        }
        match target {
            Some(Target::Scrollbar) => {
                self.scroll_drag = Some((y, self.scroll));
                self.window.capture();
            }
            Some(Target::Row(at, Part::Main))
                if matches!(self.control(at), Some(Control::Slider { .. })) =>
            {
                if let (Some(r), Some(Control::Slider { min, max, step, .. })) = (
                    self.part_rect(Target::Row(at, Part::Main)),
                    self.control(at),
                ) {
                    self.slider = Some((at, slider_value(r, x, *min, *max, *step)));
                    self.window.capture();
                }
            }
            Some(Target::Row(_, part @ (Part::Sv | Part::Hue))) => {
                self.picker_drag = Some(part);
                self.drag_picker(part, x, y);
                self.window.capture();
            }
            _ => {}
        }
    }

    fn is_recording_target(&self, target: Option<Target>) -> bool {
        match target {
            Some(Target::Row(at, Part::Main)) => matches!(
                self.control(at),
                Some(Control::Hotkey { target, .. }) if Some(target) == self.recording.as_ref()
            ),
            _ => false,
        }
    }

    /// The button under a target (row buttons, text box buttons, trailing
    /// buttons).
    fn target_button(&self, target: Target) -> Option<&ButtonData> {
        let Target::Row(at, part) = target else {
            return None;
        };
        let row = self.row(at)?;
        match (&row.control, part) {
            (Control::Buttons(buttons) | Control::Input { buttons, .. }, Part::Item(i)) => {
                buttons.get(i)
            }
            (_, Part::Trail(i)) => row.trail.get(i),
            _ => None,
        }
    }

    fn mouse_up(&mut self, core: &mut Core, x: f32, y: f32, out: &mut Outcome) {
        let pressed = self.pressed.take();
        if let Some((at, value)) = self.slider.take() {
            self.window.release_capture();
            if let Some(Control::Slider { set, .. }) = self.control(at) {
                let set = set.clone();
                self.set_value(core, &set, value, out);
            }
            return;
        }
        if self.picker_drag.take().is_some() {
            self.window.release_capture();
            self.commit_picker(core, out);
            return;
        }
        if self.scroll_drag.take().is_some() {
            self.window.release_capture();
            return;
        }
        let target = self.hit(x, y);
        if target.is_some() && target == pressed {
            if let Some(t) = target {
                self.activate(core, t, out);
            }
        }
    }

    fn commit_picker(&mut self, core: &mut Core, out: &mut Outcome) {
        let Some(p) = &self.picker else {
            return;
        };
        let (key, color) = (p.key, widgets::hsv(p.h, p.s, p.v));
        themes::set_color(core, key, color);
        out.theme = true;
        self.rebuild(core);
    }

    fn start_editing(&mut self, editing: Editing) {
        self.editing = Some(editing);
        self.caret = true;
        self.window.set_timer(CARET, 530);
    }

    /// Clicks, Enter and Space on a control.
    fn activate(&mut self, core: &mut Core, target: Target, out: &mut Outcome) {
        let at = match target {
            Target::Nav(page) => {
                self.navigate(core, page);
                return;
            }
            Target::Scrollbar => return,
            Target::Row(at, part) => (at, part),
        };
        let (at, part) = at;
        let Some(control) = self.control(at) else {
            return;
        };
        match (control, part) {
            (Control::Toggle(on, set) | Control::Check(on, set), Part::Main) => {
                let (on, set) = (*on, set.clone());
                self.set_value(core, &set, if on { 0.0 } else { 1.0 }, out);
            }
            (
                Control::Choice {
                    options, selected, ..
                },
                Part::Main,
            ) => {
                out.menu = self.choice_menu(at, options, *selected);
            }
            (Control::Hotkey { target, .. }, Part::Main) => {
                if self.recording.as_ref() == Some(target) {
                    return;
                }
                self.recording = Some(target.clone());
                out.capture = Some(true);
            }
            (Control::Hotkey { target, .. }, Part::Clear) => {
                let target = target.clone();
                lists::set_hotkey(core, &target, HotkeyData::default());
                out.hotkeys = true;
                self.rebuild(core);
            }
            (Control::Color { key, color, .. }, Part::Main) => {
                let (key, color) = (*key, *color);
                if self.picker.as_ref().is_some_and(|p| p.row == at) {
                    self.picker = None;
                } else {
                    let (h, s, v) = widgets::to_hsv(color);
                    self.picker = Some(Picker {
                        row: at,
                        key,
                        h,
                        s,
                        v,
                    });
                    self.hex = hex_of(color);
                }
                self.editing = None;
            }
            (Control::Color { .. }, Part::Hex) => self.start_editing(Editing::Hex),
            (Control::Buttons(_) | Control::Input { .. }, Part::Item(_)) | (_, Part::Trail(_)) => {
                if let Some(b) = self.target_button(target).cloned() {
                    self.press(core, &b, out);
                }
            }
            (_, Part::Header) => {
                if let Some((action, _)) = self.row(at).and_then(|r| r.expand.clone()) {
                    self.run(core, action, out);
                }
            }
            (Control::Themes(chips), Part::Item(i)) => {
                if let Some(chip) = chips.get(i).filter(|c| c.custom.is_some()) {
                    let name = chip.name.clone();
                    self.run(core, Action::ThemeSelect(name), out);
                }
            }
            (Control::Themes(chips), Part::Delete(i)) => {
                // Two steps, like the original's ConfirmButton.
                if self.armed == Some(i) {
                    let name = chips.get(i).map(|c| c.name.clone());
                    self.armed = None;
                    if let Some(name) = name {
                        self.run(core, Action::ThemeDelete(name), out);
                    }
                } else {
                    self.armed = Some(i);
                }
            }
            (Control::Input { field, .. }, Part::Name) => {
                let field = *field;
                self.start_editing(Editing::Field(field));
            }
            _ => {}
        }
    }

    /// A button click: two-step buttons arm first, like the original's
    /// ConfirmButton.
    fn press(&mut self, core: &mut Core, b: &ButtonData, out: &mut Outcome) {
        if b.confirm.is_some() && !self.is_confirming(b) {
            self.confirming = Some(b.action.clone());
            return;
        }
        self.confirming = None;
        self.run(core, b.action.clone(), out);
    }

    /// Enter in a text box runs its primary (or first) button.
    fn submit(&mut self, core: &mut Core, field: Field, out: &mut Outcome) {
        let button = self
            .sections
            .iter()
            .flat_map(|s| &s.rows)
            .find_map(|r| match &r.control {
                Control::Input {
                    field: f, buttons, ..
                } if *f == field => buttons
                    .iter()
                    .find(|b| b.primary)
                    .or(buttons.first())
                    .cloned(),
                _ => None,
            });
        if let Some(b) = button {
            self.run(core, b.action, out);
        }
    }

    fn run(&mut self, core: &mut Core, action: Action, out: &mut Outcome) {
        match &action {
            Action::ThemeSelect(name) => themes::select(core, name),
            Action::ThemeDelete(name) => themes::delete(core, name),
            Action::Randomize => themes::randomize(core),
            Action::ResetColors => themes::reset(core),
            Action::ThemeSave => {
                let name = std::mem::take(&mut self.texts.theme);
                themes::save(core, &name);
                self.editing = None;
            }
            Action::OpenUrl(url) => {
                let _ = std::process::Command::new("explorer").arg(url).spawn();
                return;
            }
            Action::RestoreApp { .. }
            | Action::RestoreAllApps
            | Action::RestoreDevice(_)
            | Action::RestoreAllDevices => pages::restore(core, &action),
            list => {
                // File dialogs are modal: a pressed state must not stick.
                self.pressed = None;
                self.editing = None;
                let owner = self.window.raw();
                let effect = lists::run(core, &mut self.state, &mut self.texts, owner, list);
                out.hotkeys |= effect.presets || effect.imported;
                out.apply_preset = effect.apply;
                // An import changes appearance and behaviour settings too.
                out.theme |= effect.imported;
                self.rebuild(core);
                if let Some(expanded) = self.state.expanded.clone() {
                    self.reveal_item(&expanded);
                }
                return;
            }
        }
        out.theme = true;
        self.rebuild(core);
    }

    /// Scrolls a newly opened list item's details into view.
    fn reveal_item(&mut self, item: &Expand) {
        self.layout = self.compute_layout();
        let header = self.layout.rows.iter().position(|r| {
            self.row(r.at)
                .and_then(|row| row.expand.as_ref())
                .is_some_and(|(action, open)| {
                    *open
                        && matches!(action,
                            Action::PresetToggle(e) | Action::RuleToggle(e) | Action::FolderToggle(e)
                            if e == item)
                })
        });
        let Some(i) = header else {
            return;
        };
        // The header and its detail rows, up to the next unindented row.
        let top = self.layout.rows[i].line.y;
        let mut bottom = self.layout.rows[i].line.bottom();
        for r in &self.layout.rows[i + 1..] {
            if !self.row(r.at).is_some_and(|row| row.indent) {
                break;
            }
            bottom = r.line.bottom();
        }
        let view = self.layout.view;
        if bottom > view.bottom() - 16.0 {
            let by = bottom - (view.bottom() - 16.0);
            let max_by = (top - (view.y + 16.0)).max(0.0);
            self.scroll = (self.scroll + by.min(max_by)).min(self.max_scroll());
            self.layout = self.compute_layout();
        }
    }

    fn choice_menu(&self, at: RowRef, options: &[String], selected: usize) -> Option<MenuRequest> {
        let r = self.part_rect(Target::Row(at, Part::Main))?;
        let (x, y) = self.window.client_to_screen(r.x, r.bottom() + 2.0);
        let items = options
            .iter()
            .enumerate()
            .map(|(i, o)| Item::check(o.as_str(), i as u32 + 1, i == selected))
            .collect();
        let commands = (0..options.len())
            .map(|index| {
                MenuCommand::SettingsChoice(Command {
                    page: self.page,
                    row: at,
                    index,
                })
            })
            .collect();
        Some(MenuRequest {
            owner: self.window,
            items,
            commands,
            x,
            y,
            above: false,
        })
    }

    fn key(&mut self, core: &mut Core, vk: u16, m: Modifiers, out: &mut Outcome) {
        if let Some(target) = self.recording.clone() {
            if MODIFIER_KEYS.contains(&vk) {
                return;
            }
            // Escape, Backspace and Delete clear the shortcut.
            let hotkey = if matches!(vk, VK_ESCAPE | VK_BACK | VK_DELETE) {
                HotkeyData::default()
            } else {
                pages::hotkey_from(vk, m.ctrl, m.alt, m.shift, m.win)
            };
            lists::set_hotkey(core, &target, hotkey);
            self.recording = None;
            out.capture = Some(false);
            out.hotkeys = true;
            self.rebuild(core);
            return;
        }
        // Characters arrive as `Char`; only Tab leaves a text box.
        if self.editing.is_some() && vk != VK_TAB {
            return;
        }
        self.keyboard = true;
        let page = self.view_height();
        match vk {
            VK_TAB => {
                self.editing = None;
                let list = self.focusables();
                let index = self.focus.and_then(|f| list.iter().position(|t| *t == f));
                let next = match (index, m.shift) {
                    (None, false) => 0,
                    (None, true) => list.len() - 1,
                    (Some(i), false) => (i + 1) % list.len(),
                    (Some(i), true) => (i + list.len() - 1) % list.len(),
                };
                self.focus = list.get(next).copied();
                if let Some(t) = self.focus {
                    self.reveal(t);
                    match t {
                        Target::Row(at, Part::Name) => {
                            if let Some(Control::Input { field, .. }) = self.control(at) {
                                let field = *field;
                                self.start_editing(Editing::Field(field));
                            }
                        }
                        Target::Row(_, Part::Hex) => self.start_editing(Editing::Hex),
                        _ => {}
                    }
                }
            }
            VK_RETURN | VK_SPACE => {
                if let Some(t) = self.focus {
                    self.activate(core, t, out);
                }
            }
            VK_ESCAPE => {
                self.picker = None;
                self.armed = None;
                self.confirming = None;
            }
            VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT => {
                let forward = matches!(vk, VK_DOWN | VK_RIGHT);
                match self.focus {
                    Some(Target::Nav(current)) if matches!(vk, VK_UP | VK_DOWN) => {
                        let all: Vec<PageId> = PageId::all().collect();
                        let i = all.iter().position(|p| *p == current).unwrap_or(0);
                        let next = if forward {
                            (i + 1).min(all.len() - 1)
                        } else {
                            i.saturating_sub(1)
                        };
                        self.navigate(core, all[next]);
                        self.focus = Some(Target::Nav(all[next]));
                    }
                    Some(Target::Row(at, Part::Main)) => self.nudge(core, at, forward, out),
                    _ if matches!(vk, VK_UP | VK_DOWN) => {
                        self.scroll = (self.scroll + if forward { 48.0 } else { -48.0 })
                            .clamp(0.0, self.max_scroll());
                    }
                    _ => {}
                }
            }
            VK_PRIOR => self.scroll = (self.scroll - page).max(0.0),
            VK_NEXT => self.scroll = (self.scroll + page).min(self.max_scroll()),
            VK_HOME => self.scroll = 0.0,
            VK_END => self.scroll = self.max_scroll(),
            _ => {}
        }
        self.layout = self.compute_layout();
    }

    fn view_height(&self) -> f32 {
        (self.layout.view.h - 48.0).max(48.0)
    }

    /// Arrow keys on a slider, toggle or drop-down.
    fn nudge(&mut self, core: &mut Core, at: RowRef, forward: bool, out: &mut Outcome) {
        let Some(control) = self.control(at) else {
            return;
        };
        let (set, value) = match control {
            Control::Slider {
                value,
                min,
                max,
                step,
                set,
                ..
            } => {
                let next = value + if forward { *step } else { -*step };
                (set.clone(), next.clamp(*min, *max))
            }
            Control::Toggle(on, set) | Control::Check(on, set) if *on != forward => {
                (set.clone(), if forward { 1.0 } else { 0.0 })
            }
            Control::Choice {
                options,
                selected,
                set,
            } => {
                let next = if forward {
                    (*selected + 1).min(options.len() - 1)
                } else {
                    selected.saturating_sub(1)
                };
                (set.clone(), next as f64)
            }
            _ => return,
        };
        self.set_value(core, &set, value, out);
    }

    fn char(&mut self, core: &mut Core, c: u16, out: &mut Outcome) {
        let Some(editing) = self.editing else {
            return;
        };
        self.caret = true;
        let text = match editing {
            Editing::Field(field) => self.texts.get_mut(field),
            Editing::Hex => &mut self.hex,
        };
        match c {
            0x08 => {
                text.pop();
            }
            // Ctrl+Backspace.
            0x7f => text.clear(),
            0x0d => {
                self.editing = None;
                match editing {
                    Editing::Field(field) => self.submit(core, field, out),
                    Editing::Hex => self.commit_hex(core, out),
                }
            }
            0x1b => {
                self.editing = None;
                if editing == Editing::Hex {
                    if let Some(p) = &self.picker {
                        self.hex = hex_of(widgets::hsv(p.h, p.s, p.v));
                    }
                }
            }
            0xd800..=0xdbff => self.surrogate = Some(c),
            c if c >= 0x20 => {
                let units: Vec<u16> = match self.surrogate.take() {
                    Some(high) if (0xdc00..=0xdfff).contains(&c) => vec![high, c],
                    _ => vec![c],
                };
                let limit = if editing == Editing::Hex { 7 } else { 64 };
                if text.chars().count() < limit {
                    text.push_str(&String::from_utf16_lossy(&units));
                }
            }
            _ => {}
        }
    }

    fn commit_hex(&mut self, core: &mut Core, out: &mut Outcome) {
        let text = self.hex.trim().trim_start_matches('#');
        let Some(color) = (text.len() == 6)
            .then(|| Rgba::hex(&format!("#{text}")))
            .flatten()
        else {
            return;
        };
        if let Some(p) = &mut self.picker {
            (p.h, p.s, p.v) = widgets::to_hsv(color);
            let key = p.key;
            themes::set_color(core, key, color);
            out.theme = true;
            self.rebuild(core);
        }
    }

    // ---------- painting ----------

    /// Draws a whole page without showing the window: in the system theme
    /// or `light`, and with the colour picker open on the first colour row
    /// if `picker`.
    pub fn render(
        &mut self,
        core: &mut Core,
        page: PageId,
        light: Option<bool>,
        picker: bool,
    ) -> Option<(u32, u32, Vec<u8>)> {
        self.mica = false;
        self.palette = palette(false);
        if let Some(light) = light {
            let accent = Rgba::hex(&system::accent()).unwrap_or(Rgba::rgb(0, 120, 215));
            let dark1 = Rgba::hex(&system::accent_dark1()).unwrap_or(Rgba::rgb(0, 90, 158));
            self.palette = Palette::new(!light, false, accent, dark1);
        }
        self.scale = 1.0;
        self.navigate(core, page);
        self.picker = None;
        if picker {
            let found = self.sections.iter().enumerate().find_map(|(si, section)| {
                section
                    .rows
                    .iter()
                    .enumerate()
                    .find_map(|(ri, row)| match row.control {
                        Control::Color { key, color, .. } => Some((si, ri, key, color)),
                        _ => None,
                    })
            });
            if let Some((section, row, key, color)) = found {
                let (h, s, v) = widgets::to_hsv(color);
                self.picker = Some(Picker {
                    row: RowRef { section, row },
                    key,
                    h,
                    s,
                    v,
                });
                self.hex = hex_of(color);
            }
        }
        self.size = SIZE;
        self.layout = self.compute_layout();
        self.size.1 = self.layout.content.max(SIZE.1);
        self.draw_now(None);
        self.surface.as_ref()?.rgba()
    }

    /// `render` with the page's first list item open and, on the
    /// QuickTrumpet page, the capture form's option lists out.
    pub fn render_open(&mut self, core: &mut Core, page: PageId) -> Option<(u32, u32, Vec<u8>)> {
        self.navigate(core, page);
        let first = self.sections.iter().flat_map(|s| &s.rows).find_map(|r| {
            match r.expand.as_ref().map(|(a, _)| a) {
                Some(Action::PresetToggle(e) | Action::RuleToggle(e) | Action::FolderToggle(e)) => {
                    Some(e.clone())
                }
                _ => None,
            }
        });
        self.state.expanded = first;
        let capture = std::mem::take(&mut self.state.capture);
        if page == PageId::QuickTrumpet {
            self.state.capture.defaults = true;
            self.state.capture.open = Some(pages::Choose::Defaults);
        }
        let image = self.render(core, page, None, false);
        self.state.capture = capture;
        self.state.expanded = None;
        image
    }

    pub fn paint(&mut self) {
        let (pw, ph) = self.window.client_size();
        if pw <= 0 || ph <= 0 || self.window.is_minimized() {
            return;
        }
        self.scale = self.window.scale();
        self.size = (pw as f32 / self.scale, ph as f32 / self.scale);
        self.draw_now(Some(self.window.raw()));
    }

    fn draw_now(&mut self, hwnd: Option<isize>) {
        self.layout = self.compute_layout();
        let max = self.max_scroll();
        if self.scroll > max {
            self.scroll = max;
            self.layout = self.compute_layout();
        }
        let width = (self.size.0 * self.scale).round() as i32;
        let height = (self.size.1 * self.scale).round() as i32;
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        surface.draw(width, height, self.scale, |canvas| self.draw(canvas));
        if let Some(hwnd) = hwnd {
            surface.blit(hwnd);
        }
        self.surface = Some(surface);
    }

    fn state(&self, target: Target) -> State {
        State {
            hover: self.hover == Some(target),
            pressed: self.pressed == Some(target) && self.hover == Some(target),
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        let p = self.palette;
        canvas.rect(Rect::new(0.0, 0.0, self.size.0, self.size.1), p.background);
        self.draw_sidebar(canvas);
        let view = self.layout.view;
        canvas.push_layer(1.0, Some((view, 0.0)));
        canvas.text(
            self.page.title(),
            self.layout.title,
            &TextStyle::new(DISPLAY, 28.0).weight(600),
            p.text,
        );
        canvas.text(
            self.page.subtitle(),
            self.layout.subtitle,
            &label_style(14.0).wrap().top(),
            p.secondary,
        );
        for (i, (title, desc, card)) in self.layout.sections.iter().enumerate() {
            let section = &self.sections[i];
            canvas.text(
                &section.title,
                *title,
                &label_style(16.0).weight(600),
                p.text,
            );
            if let (Some(r), Some(d)) = (desc, &section.desc) {
                canvas.text(d, *r, &label_style(12.0).wrap().top(), p.secondary);
            }
            canvas.round_rect(*card, 6.0, p.card);
            canvas.stroke_round_rect(*card, 6.0, 1.0, p.card_stroke);
        }
        for row in &self.layout.rows {
            if row.line.bottom() < view.y - PICKER_H || row.line.y > view.bottom() {
                continue;
            }
            self.draw_row(canvas, row);
        }
        canvas.pop();
        if let Some(r) = self.scrollbar() {
            let hot = matches!(self.hover, Some(Target::Scrollbar)) || self.scroll_drag.is_some();
            canvas.round_rect(r, 3.0, p.tertiary.alpha(if hot { 0.9 } else { 0.5 }));
        }
    }

    fn draw_sidebar(&self, canvas: &mut Canvas) {
        let p = self.palette;
        if let Some(icon) = &self.app_icon {
            canvas.image(icon, Rect::new(20.0, 22.0, 24.0, 24.0), 1.0);
        }
        canvas.text(
            "BetterTrumpet",
            Rect::new(54.0, 16.0, SIDEBAR - 70.0, 36.0),
            &label_style(15.0).weight(600),
            p.text,
        );
        for (title, r) in &self.layout.groups {
            canvas.text(title, *r, &label_style(12.0).weight(600), p.secondary);
        }
        for (page, r) in &self.layout.nav {
            let target = Target::Nav(*page);
            let selected = *page == self.page;
            let state = self.state(target);
            let fill = if selected || state.hover {
                if state.pressed {
                    p.subtle_pressed
                } else {
                    p.subtle_hover
                }
            } else {
                Rgba::TRANSPARENT
            };
            canvas.round_rect(*r, 4.0, fill);
            if selected {
                canvas.round_rect(
                    Rect::new(r.x, r.y + r.h / 2.0 - 8.0, 3.0, 16.0),
                    1.5,
                    p.accent,
                );
            }
            canvas.text(
                page.glyph(),
                Rect::new(r.x + 12.0, r.y, 20.0, r.h),
                &TextStyle::new(self.icon_font, 16.0).align(Align::Center),
                p.text,
            );
            canvas.text(
                page.title(),
                Rect::new(r.x + 44.0, r.y, r.w - 52.0, r.h),
                &label_style(14.0).ellipsis(),
                p.text,
            );
            if self.keyboard && self.focus == Some(target) {
                widgets::focus_ring(canvas, &p, *r, 4.0);
            }
        }
    }

    fn draw_row(&self, canvas: &mut Canvas, row: &RowLayout) {
        let p = self.palette;
        let Some(data) = self.row(row.at) else {
            return;
        };
        let inset = if data.indent { INDENT } else { 0.0 };
        if row.at.row > 0 {
            canvas.rect(
                Rect::new(
                    row.line.x + 1.0 + inset,
                    row.line.y,
                    row.line.w - 2.0 - inset,
                    1.0,
                ),
                p.divider,
            );
        }
        if data.expand.is_some() {
            let state = self.state(Target::Row(row.at, Part::Header));
            if state.hover {
                let fill = if state.pressed {
                    p.subtle_pressed
                } else {
                    p.subtle_hover
                };
                canvas.round_rect(row.line.inset(1.0, 1.0), 4.0, fill);
            }
        }
        if !data.enabled {
            canvas.push_layer(0.4, None);
        }
        // A plain note: secondary text, unless it is an item's title.
        let plain = matches!(data.control, Control::None) && data.expand.is_none();
        let heading = matches!(data.control, Control::Heading);
        canvas.text(
            &data.label,
            row.label,
            &label_style(if heading { 12.0 } else { 14.0 })
                .weight(if plain { 400 } else { 600 })
                .wrap()
                .top(),
            if plain || heading {
                p.secondary
            } else {
                p.text
            },
        );
        if let (Some(r), Some(badge)) = (row.badge, &data.badge) {
            canvas.round_rect(r, r.h / 2.0, p.accent.alpha(0.18));
            canvas.text(
                badge,
                r,
                &label_style(12.0).weight(600).align(Align::Center),
                p.accent,
            );
        }
        for (r, chip) in row.chips.iter().zip(&data.chips) {
            canvas.round_rect(*r, r.h / 2.0, p.control);
            canvas.stroke_round_rect(r.inset(0.5, 0.5), r.h / 2.0 - 0.5, 1.0, p.control_stroke);
            canvas.text(
                chip,
                r.inset(10.0, 0.0),
                &label_style(12.0).align(Align::Center).ellipsis(),
                p.secondary,
            );
        }
        if let (Some(r), Some((_, open))) = (row.chevron, &data.expand) {
            canvas.text(
                if *open { "\u{E70E}" } else { "\u{E70D}" },
                r,
                &TextStyle::new(self.icon_font, 12.0).align(Align::Center),
                p.secondary,
            );
        }
        if let (Some(r), Some(desc)) = (row.desc, &data.desc) {
            canvas.text(desc, r, &label_style(12.0).wrap().top(), p.secondary);
        }
        let part = |part: Part| row.parts.iter().find(|(q, _)| *q == part).map(|(_, r)| *r);
        let target = |part: Part| Target::Row(row.at, part);
        match &data.control {
            Control::Toggle(on, _) => {
                if let Some(r) = part(Part::Main) {
                    widgets::toggle(canvas, &p, r, *on, self.state(target(Part::Main)));
                }
            }
            Control::Check(on, _) => {
                if let Some(r) = part(Part::Main) {
                    let state = self.state(target(Part::Main));
                    widgets::checkbox(canvas, &p, r, *on, self.icon_font, state);
                }
            }
            Control::Heading => {}
            Control::Slider {
                value,
                min,
                max,
                unit,
                ..
            } => {
                let value = match self.slider {
                    Some((at, live)) if at == row.at => live,
                    _ => *value,
                };
                if let Some(r) = part(Part::Main) {
                    let fraction = ((value - min) / (max - min)) as f32;
                    widgets::slider(canvas, &p, r, fraction, self.state(target(Part::Main)));
                    canvas.text(
                        &format_value(value, *unit),
                        Rect::new(r.right() + 8.0, r.y, 56.0, r.h),
                        &label_style(14.0).align(Align::Trailing),
                        p.secondary,
                    );
                }
            }
            Control::Choice {
                options, selected, ..
            } => {
                if let Some(r) = part(Part::Main) {
                    let text = options.get(*selected).map_or("", String::as_str);
                    widgets::dropdown(
                        canvas,
                        &p,
                        r,
                        text,
                        self.icon_font,
                        self.state(target(Part::Main)),
                    );
                }
            }
            Control::Hotkey {
                target,
                text,
                fallback,
            } => self.draw_hotkey(canvas, row, target, text, fallback.as_deref()),
            Control::Color { color, set, .. } => {
                let shown = match &self.picker {
                    Some(pk) if pk.row == row.at => widgets::hsv(pk.h, pk.s, pk.v),
                    _ => *color,
                };
                if let Some(r) = part(Part::Main) {
                    let label = if *set || self.picker.as_ref().is_some_and(|pk| pk.row == row.at) {
                        hex_of(shown)
                    } else {
                        trf("ColorDefaultFormat", &[("Color", &hex_of(shown))])
                    };
                    canvas.text(
                        &label,
                        Rect::new(r.x - 160.0, r.y, 152.0, r.h),
                        &label_style(13.0).align(Align::Trailing),
                        p.secondary,
                    );
                    widgets::swatch(canvas, &p, r, shown, self.state(target(Part::Main)));
                }
            }
            Control::Buttons(buttons) => {
                for (i, b) in buttons.iter().enumerate() {
                    if let Some(r) = part(Part::Item(i)) {
                        self.draw_button(canvas, r, b, self.state(target(Part::Item(i))));
                    }
                }
            }
            Control::Chip(text) => {
                let w = self.control_width(data);
                let r = Rect::new(
                    row.line.right() - 16.0 - w,
                    row.line.y + row.line.h / 2.0 - 14.0,
                    w,
                    28.0,
                );
                canvas.round_rect(r, 14.0, p.control);
                canvas.text(text, r, &label_style(14.0).align(Align::Center), p.text);
            }
            Control::Themes(chips) => self.draw_chips(canvas, row, chips),
            Control::Input {
                field,
                placeholder,
                buttons,
            } => {
                if let Some(r) = part(Part::Name) {
                    widgets::text_box(
                        canvas,
                        &p,
                        r,
                        self.texts.get(*field),
                        placeholder,
                        self.editing == Some(Editing::Field(*field)),
                        self.caret,
                        self.state(target(Part::Name)),
                    );
                }
                for (i, b) in buttons.iter().enumerate() {
                    if let Some(r) = part(Part::Item(i)) {
                        self.draw_button(canvas, r, b, self.state(target(Part::Item(i))));
                    }
                }
            }
            Control::None => {}
        }
        for (i, b) in data.trail.iter().enumerate() {
            if let Some(r) = part(Part::Trail(i)) {
                self.draw_button(canvas, r, b, self.state(target(Part::Trail(i))));
            }
        }
        if self.picker.as_ref().is_some_and(|pk| pk.row == row.at) {
            self.draw_picker(canvas, row);
        }
        if !data.enabled {
            canvas.pop();
        }
        if self.keyboard {
            if let Some(Target::Row(at, focused)) = self.focus {
                if at == row.at {
                    if let Some(r) = part(focused) {
                        let radius = match (&data.control, focused) {
                            (Control::Toggle(..), Part::Main) => 10.0,
                            (_, Part::Header) => 6.0,
                            _ => 4.0,
                        };
                        widgets::focus_ring(canvas, &p, r, radius);
                    }
                }
            }
        }
    }

    /// A button as it looks now: an armed two-step button shows its
    /// question on red.
    fn draw_button(&self, canvas: &mut Canvas, r: Rect, b: &ButtonData, state: State) {
        let p = self.palette;
        if self.is_confirming(b) {
            canvas.round_rect(r, 4.0, Rgba::rgb(196, 43, 28));
            canvas.text(
                b.confirm.unwrap_or_default(),
                r,
                &label_style(14.0).weight(600).align(Align::Center),
                Rgba::WHITE,
            );
            return;
        }
        if b.label.is_empty() {
            // Icon buttons are subtle until hovered.
            if state.hover {
                let fill = if state.pressed {
                    p.subtle_pressed
                } else {
                    p.subtle_hover
                };
                canvas.round_rect(r, 4.0, fill);
            }
            canvas.text(
                b.glyph.unwrap_or_default(),
                r,
                &TextStyle::new(self.icon_font, 14.0).align(Align::Center),
                p.text,
            );
            return;
        }
        widgets::button(
            canvas,
            &p,
            r,
            &b.label,
            b.glyph.map(|g| (self.icon_font, g)),
            b.primary,
            state,
        );
    }

    fn draw_hotkey(
        &self,
        canvas: &mut Canvas,
        row: &RowLayout,
        target: &HotkeyTarget,
        text: &str,
        fallback: Option<&str>,
    ) {
        let p = self.palette;
        let part = |part: Part| row.parts.iter().find(|(q, _)| *q == part).map(|(_, r)| *r);
        let Some(r) = part(Part::Main) else {
            return;
        };
        let state = self.state(Target::Row(row.at, Part::Main));
        if self.recording.as_ref() == Some(target) {
            widgets::button(
                canvas,
                &p,
                r,
                tr("WebSettingsPressShortcut"),
                None,
                true,
                state,
            );
        } else if text.is_empty() {
            let label = match fallback {
                Some(f) => format!("{}{f}", tr("SettingsDefaultPrefix")),
                None => tr("WebSettingsRecordShortcut").into(),
            };
            widgets::button(canvas, &p, r, &label, None, false, state);
        } else {
            let fill = if state.hover {
                p.subtle_hover
            } else {
                Rgba::TRANSPARENT
            };
            canvas.round_rect(r, 4.0, fill);
            widgets::keycaps(canvas, &p, r.inset(4.0, 0.0), text);
        }
        if let Some(c) = part(Part::Clear) {
            let state = self.state(Target::Row(row.at, Part::Clear));
            if state.hover {
                canvas.round_rect(c, 4.0, p.subtle_hover);
            }
            canvas.text(
                "\u{E711}",
                c,
                &TextStyle::new(self.icon_font, 12.0).align(Align::Center),
                p.secondary,
            );
        }
    }

    fn draw_chips(&self, canvas: &mut Canvas, row: &RowLayout, chips: &[pages::ThemeChip]) {
        let p = self.palette;
        for (i, chip) in chips.iter().enumerate() {
            let Some((_, r)) = row.parts.iter().find(|(q, _)| *q == Part::Item(i)) else {
                continue;
            };
            let r = *r;
            let state = self.state(Target::Row(row.at, Part::Item(i)));
            let fill = if state.pressed {
                p.control_pressed
            } else if state.hover {
                p.control_hover
            } else {
                p.control
            };
            canvas.round_rect(r, 6.0, fill);
            if chip.active {
                canvas.stroke_round_rect(r, 6.0, 2.0, p.accent);
            } else {
                canvas.stroke_round_rect(r.inset(0.5, 0.5), 5.5, 1.0, p.control_stroke);
            }
            for (j, color) in chip.swatches.iter().enumerate() {
                let s = Rect::new(
                    r.x + 12.0 + j as f32 * 13.0,
                    r.y + r.h / 2.0 - 8.0,
                    16.0,
                    16.0,
                );
                canvas.circle(s.x + 8.0, s.y + 8.0, 8.5, fill.alpha(1.0).lerp(p.card, 0.0));
                canvas.circle(s.x + 8.0, s.y + 8.0, 7.0, *color);
            }
            let text_x = r.x + 12.0 + 4.0 * 13.0 + 10.0;
            let has_delete = chip.custom == Some(true);
            canvas.text(
                &chip.name,
                Rect::new(
                    text_x,
                    r.y,
                    r.right() - text_x - if has_delete { 30.0 } else { 8.0 },
                    r.h,
                ),
                &label_style(13.0).weight(600).ellipsis(),
                p.text,
            );
            if has_delete {
                if let Some((_, d)) = row.parts.iter().find(|(q, _)| *q == Part::Delete(i)) {
                    let armed = self.armed == Some(i);
                    let hot = self.state(Target::Row(row.at, Part::Delete(i))).hover;
                    if armed {
                        canvas.round_rect(*d, 4.0, Rgba::rgb(196, 43, 28));
                        canvas.text(
                            tr("WebSettingsConfirmDelete"),
                            *d,
                            &label_style(12.0).weight(600).align(Align::Center),
                            Rgba::WHITE,
                        );
                    } else {
                        if hot {
                            canvas.round_rect(*d, 4.0, p.subtle_hover);
                        }
                        canvas.text(
                            "\u{E74D}",
                            *d,
                            &TextStyle::new(self.icon_font, 12.0).align(Align::Center),
                            p.secondary,
                        );
                    }
                }
            }
        }
    }

    fn draw_picker(&self, canvas: &mut Canvas, row: &RowLayout) {
        let p = self.palette;
        let Some(pk) = &self.picker else {
            return;
        };
        let part = |part: Part| row.parts.iter().find(|(q, _)| *q == part).map(|(_, r)| *r);
        if let Some(r) = part(Part::Sv) {
            widgets::sv_square(canvas, r, pk.h, pk.s, pk.v);
        }
        if let Some(r) = part(Part::Hue) {
            widgets::hue_bar(canvas, r, pk.h);
        }
        if let Some(r) = part(Part::Hex) {
            canvas.text(
                tr("WebSettingsHexColor"),
                Rect::new(r.x, r.bottom() + 6.0, r.w, 20.0),
                &label_style(12.0),
                p.secondary,
            );
            widgets::text_box(
                canvas,
                &p,
                r,
                &self.hex,
                "#RRGGBB",
                self.editing == Some(Editing::Hex),
                self.caret,
                self.state(Target::Row(row.at, Part::Hex)),
            );
            let preview = Rect::new(r.x, r.bottom() + 32.0, 64.0, 40.0);
            widgets::swatch(
                canvas,
                &p,
                preview,
                widgets::hsv(pk.h, pk.s, pk.v),
                State::default(),
            );
        }
    }
}

/// The slider value under `x`, snapped to `step`.
fn slider_value(rect: Rect, x: f32, min: f64, max: f64, step: f64) -> f64 {
    let fraction = f64::from(((x - rect.x - 10.0) / (rect.w - 20.0)).clamp(0.0, 1.0));
    let raw = min + (max - min) * fraction;
    let snapped = min + ((raw - min) / step).round() * step;
    snapped.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_values_snap_to_their_step() {
        let r = Rect::new(0.0, 0.0, 220.0, 32.0);
        assert_eq!(slider_value(r, 10.0, 0.5, 5.0, 0.25), 0.5);
        assert_eq!(slider_value(r, 210.0, 0.5, 5.0, 0.25), 5.0);
        assert_eq!(slider_value(r, 110.0, 0.0, 5000.0, 100.0), 2500.0);
    }

    #[test]
    fn values_format_with_their_units() {
        assert_eq!(format_value(42.0, Unit::Percent), "42%");
        assert_eq!(format_value(2.0, Unit::Seconds), "2 s");
        assert_eq!(format_value(2.25, Unit::Seconds), "2.25 s");
        assert_eq!(format_value(2.5, Unit::Seconds), "2.5 s");
        assert_eq!(format_value(3.0, Unit::OutOf10), "3/10");
    }
}
