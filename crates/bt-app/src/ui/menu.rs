//! The app's menus, drawn like the original's WPF ContextMenu ("Windows 11
//! style" in App.xaml) instead of the system's: 13 DIP text, 30 DIP items
//! with a 4 DIP rounded hover highlight that fades in (80 ms) and out
//! (150 ms), a 22 DIP column for the check mark, icon or submenu arrow,
//! 7 DIP separators, at least 200 DIP wide, radius 7.
//!
//! Two looks, as the original: the tray menu and every submenu are acrylic
//! (AcrylicColor_Menu tint, DWM rounded corners and the
//! AcrylicMenuBackground veil, no border or shadow); a window's own context
//! menu is solid {Theme}ChromeMediumLow with a 1 DIP border and a drop
//! shadow.
//!
//! `popup` blocks in a nested message loop like TrackPopupMenu and returns
//! the chosen command id. Menu windows never take activation except the
//! tray menu's first level (the original sets its popup foreground so it
//! gets the keyboard and closes when another app is clicked); the loop
//! reads keys and clicks before the owner sees them. The menu closes on a
//! choice, Escape (one level at a time) or Alt, a click outside it, or
//! when another window comes to the foreground.
use super::{
    anim::{self, EASE_OUT},
    theme::Theme,
};
use bt_app::win32::{
    gfx::{Align, Canvas, Gfx, Rect, Rgba, Surface, TextStyle},
    menu::Item,
    wnd::{self, Button, ModalInput, Monitor, Window},
};
use std::{rc::Rc, time::Instant};

const FONT: &str = "Segoe UI";
/// The original names this font for its check mark and arrow glyphs.
const GLYPHS: &str = "Segoe MDL2 Assets";
const GLYPH_CHECK: &str = "\u{E73E}";
const GLYPH_ARROW: &str = "\u{E0E2}";
const MIN_WIDTH: f32 = 200.0;
const BORDER: f32 = 1.0;
const PADDING: f32 = 4.0;
const ITEM_H: f32 = 30.0;
const ITEM_MARGIN_X: f32 = 2.0;
const ITEM_MARGIN_Y: f32 = 0.5;
const SEPARATOR_H: f32 = 7.0;
const ICON_COLUMN: f32 = 22.0;
const RADIUS: f32 = 7.0;
/// The solid look's room for its shadow (right and bottom, as WPF's
/// `Padding="0,0,8,8"`).
const SHADOW: f32 = 8.0;
/// The tray menu's distance from the taskbar (TrayContextMenuGap).
const TRAY_GAP: f32 = 8.0;
/// SystemParameters.MenuShowDelay.
const SUBMENU_DELAY: u32 = 400;

// Timers on the first level's window.
const FRAME: usize = 1;
const SUBMENU: usize = 2;
const CHECK: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Look {
    Acrylic,
    Solid,
}

/// The menu colours for the system theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    text: Rgba,
    /// BaseMedium: check marks and arrows.
    secondary: Rgba,
    /// BaseMediumLow: disabled text.
    disabled: Rgba,
    /// ListLow: the hover highlight.
    highlight: Rgba,
    separator: Rgba,
    /// ChromeMediumLow and the border of the solid look.
    background: Rgba,
    border: Rgba,
    /// AcrylicMenuBackground over the acrylic.
    veil: Rgba,
    /// AcrylicColor_Menu as the accent policy's ABGR tint, `None` with
    /// transparency effects off.
    tint: Option<u32>,
    tint_color: Rgba,
}

impl Palette {
    pub fn new(theme: &Theme) -> Self {
        let light = theme.light;
        let ink = if light { Rgba::BLACK } else { Rgba::WHITE };
        // AcrylicColor_Menu / AcrylicMenuBackground: accent mode
        // SystemAccentDark1, light #F3F3F3, dark #2C2C2C; the tint at 0.7,
        // the veil at 0.36 (0.86 without transparency).
        let base = if theme.accent_mode {
            theme.accent_dark1
        } else if light {
            Rgba::rgb(0xf3, 0xf3, 0xf3)
        } else {
            Rgba::rgb(0x2c, 0x2c, 0x2c)
        };
        let tint = theme.transparency.then(|| {
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
            (byte(0.7) << 24) | (byte(base.b) << 16) | (byte(base.g) << 8) | byte(base.r)
        });
        Self {
            text: ink,
            secondary: ink.alpha(0.6),
            disabled: ink.alpha(0.4),
            highlight: ink.alpha(0.098),
            // BaseMediumLow at the separator's 0.4 opacity.
            separator: ink.alpha(0.4 * 0.4),
            background: if light {
                Rgba::rgb(0xf2, 0xf2, 0xf2)
            } else {
                Rgba::rgb(0x2b, 0x2b, 0x2b)
            },
            // LightChromeHigh; ControlDarkAppButtonTextDisabled at 0.9.
            border: if light {
                Rgba::rgb(0xcc, 0xcc, 0xcc)
            } else {
                Rgba::rgba(0x5d, 0x5d, 0x5d, 0.9)
            },
            veil: base.alpha(if theme.transparency { 0.36 } else { 0.86 }),
            tint,
            tint_color: base.alpha(0.7),
        }
    }
}

/// An item's label without its access-key marker, and the key: the letter
/// after `&` (`&&` is a literal ampersand), else the first letter or digit.
fn label(text: &str) -> (String, Option<char>) {
    let mut out = String::with_capacity(text.len());
    let mut key = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.next() {
                Some('&') => out.push('&'),
                Some(next) => {
                    if key.is_none() {
                        key = Some(next);
                    }
                    out.push(next);
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    let key = key.or_else(|| out.chars().find(|c| c.is_alphanumeric()));
    (out, key.map(|k| k.to_ascii_uppercase()))
}

fn text_of(item: &Item) -> Option<&str> {
    match item {
        Item::Command(text, ..) | Item::Icon(text, ..) | Item::Submenu(text, _) => Some(text),
        Item::Separator => None,
    }
}

/// Whether the pointer or keyboard can select the item.
fn selectable(item: &Item) -> bool {
    match item {
        Item::Command(_, id, _, enabled) => *enabled && *id != 0,
        Item::Icon(_, id, ..) => *id != 0,
        Item::Submenu(_, children) => !children.is_empty(),
        Item::Separator => false,
    }
}

/// Item rectangles (separators included) and the menu box size, in DIPs
/// from the box's top-left corner.
fn layout(gfx: &Gfx, items: &[Item], max_width: f32) -> (Vec<Rect>, f32, f32) {
    let style = TextStyle::new(FONT, 13.0);
    let text_w = items
        .iter()
        .filter_map(text_of)
        .map(|t| gfx.measure(&label(t).0, &style).0.ceil())
        .fold(0.0, f32::max);
    let inset = BORDER + PADDING;
    // Icon column, the header's 2 + 8 DIP margins, the item margins.
    let content = ICON_COLUMN + 2.0 + text_w + 8.0 + 2.0 * ITEM_MARGIN_X;
    let width = (content + 2.0 * inset)
        .max(MIN_WIDTH)
        .min(max_width.max(MIN_WIDTH));
    let mut y = inset;
    let rects = items
        .iter()
        .map(|item| {
            if matches!(item, Item::Separator) {
                let rect = Rect::new(inset, y, width - 2.0 * inset, SEPARATOR_H);
                y += SEPARATOR_H;
                rect
            } else {
                let rect = Rect::new(
                    inset + ITEM_MARGIN_X,
                    y + ITEM_MARGIN_Y,
                    width - 2.0 * (inset + ITEM_MARGIN_X),
                    ITEM_H,
                );
                y += ITEM_H + 2.0 * ITEM_MARGIN_Y;
                rect
            }
        })
        .collect();
    (rects, width, y + inset)
}

/// Where a taskbar is on its monitor, from the work area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
    Top,
    Left,
    Right,
    Bottom,
}

fn taskbar_edge(monitor: &Monitor) -> Edge {
    let (bx, by, bw, _) = monitor.bounds;
    let (wx, wy, ww, _) = monitor.work;
    if wy > by {
        Edge::Top
    } else if wx > bx {
        Edge::Left
    } else if wx + ww < bx + bw {
        Edge::Right
    } else {
        Edge::Bottom
    }
}

/// The tray menu's position (ShellNotifyIcon.PositionTrayContextMenu):
/// against the taskbar's edge of the work area, `gap` pixels from it, at
/// the click point along it, kept `gap` inside the work area.
fn place_tray(size: (i32, i32), point: (i32, i32), monitor: &Monitor, gap: i32) -> (i32, i32) {
    let (w, h) = size;
    let (wx, wy, ww, wh) = monitor.work;
    let (mut left, mut top) = point;
    match taskbar_edge(monitor) {
        Edge::Top => top = wy + gap,
        Edge::Left => left = wx + gap,
        Edge::Right => left = wx + ww - w - gap,
        Edge::Bottom => top = wy + wh - h - gap,
    }
    let min_left = wx + gap;
    let max_left = (wx + ww - w - gap).max(min_left);
    let min_top = wy + gap;
    let max_top = (wy + wh - h - gap).max(min_top);
    (left.clamp(min_left, max_left), top.clamp(min_top, max_top))
}

/// A context menu at a point: its top-left corner there (bottom-left when
/// `above`), flipped to the other side of the point when it would leave
/// the work area, then kept inside it.
fn place_at(size: (i32, i32), point: (i32, i32), above: bool, monitor: &Monitor) -> (i32, i32) {
    let (w, h) = size;
    let (x, y) = point;
    let (wx, wy, ww, wh) = monitor.work;
    let (right, bottom) = (wx + ww, wy + wh);
    let left = if x + w > right { x - w } else { x };
    let top = if above {
        if y - h < wy { y } else { y - h }
    } else if y + h > bottom {
        y - h
    } else {
        y
    };
    (
        left.clamp(wx, (right - w).max(wx)),
        top.clamp(wy, (bottom - h).max(wy)),
    )
}

/// A submenu beside its item (screen rectangle left, top, right, bottom):
/// to the right with a 2 DIP offset, or to the left where it does not fit;
/// moved up to stay inside the work area.
fn place_submenu(
    size: (i32, i32),
    item: (i32, i32, i32, i32),
    gap: i32,
    m: &Monitor,
) -> (i32, i32) {
    let (w, h) = size;
    let (l, t, r, _) = item;
    let (wx, wy, ww, wh) = m.work;
    let left = if r + gap + w <= wx + ww || l - gap - w < wx {
        r + gap
    } else {
        l - gap - w
    };
    (
        left.clamp(wx, (wx + ww - w).max(wx)),
        t.clamp(wy, (wy + wh - h).max(wy)),
    )
}

/// One open menu level: the first, or a submenu.
struct Level<'a> {
    window: Option<Window>,
    /// Taken out while drawing.
    surface: Option<Surface>,
    items: &'a [Item],
    rects: Vec<Rect>,
    look: Look,
    /// The menu box inside the window (the solid look leaves room for its
    /// shadow).
    frame: Rect,
    position: (i32, i32),
    scale: f32,
    hover: Option<usize>,
    /// Per item: highlight at the last change, its target and when.
    glow: Vec<(f32, f32, Instant)>,
    /// The item of the level above that opened this one.
    parent: Option<usize>,
}

impl<'a> Level<'a> {
    fn new(
        gfx: &Rc<Gfx>,
        items: &'a [Item],
        look: Look,
        scale: f32,
        max_width: f32,
    ) -> Option<Self> {
        let (rects, w, h) = layout(gfx, items, max_width);
        let now = Instant::now();
        Some(Self {
            window: None,
            surface: Some(Surface::new(gfx.clone()).ok()?),
            items,
            glow: vec![(0.0, 0.0, now); rects.len()],
            rects,
            look,
            frame: Rect::new(0.0, 0.0, w, h),
            position: (0, 0),
            scale,
            hover: None,
            parent: None,
        })
    }

    /// The window size in DIPs.
    fn size(&self) -> (f32, f32) {
        match self.look {
            Look::Acrylic => (self.frame.w, self.frame.h),
            Look::Solid => (self.frame.w + SHADOW, self.frame.h + SHADOW),
        }
    }

    fn pixels(&self) -> (i32, i32) {
        let (w, h) = self.size();
        (
            (w * self.scale).round() as i32,
            (h * self.scale).round() as i32,
        )
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        let (w, h) = self.pixels();
        let (px, py) = self.position;
        x >= px && y >= py && x < px + w && y < py + h
    }

    /// The selectable item at a screen point.
    fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        let lx = (x - self.position.0) as f32 / self.scale;
        let ly = (y - self.position.1) as f32 / self.scale;
        self.rects
            .iter()
            .position(|r| r.contains(lx, ly))
            .filter(|&i| selectable(&self.items[i]))
    }

    /// An item's rectangle on screen (left, top, right, bottom), with the
    /// MenuItem's 2 DIP margin.
    fn item_screen(&self, index: usize) -> (i32, i32, i32, i32) {
        let r = self.rects[index];
        let px = |v: f32| (v * self.scale).round() as i32;
        (
            self.position.0 + px(r.x - ITEM_MARGIN_X),
            self.position.1 + px(r.y - ITEM_MARGIN_Y),
            self.position.0 + px(r.right() + ITEM_MARGIN_X),
            self.position.1 + px(r.bottom() + ITEM_MARGIN_Y),
        )
    }

    fn glow_at(&self, index: usize, now: Instant) -> f32 {
        let (from, to, start) = self.glow[index];
        let ms = if to > from { 80.0 } else { 150.0 };
        anim::lerp(from, to, EASE_OUT.at(anim::progress(start, 0.0, ms, now)))
    }

    fn fading(&self, now: Instant) -> bool {
        self.glow
            .iter()
            .any(|&(from, to, start)| from != to && anim::running(Some(start), 150.0, now))
    }

    fn set_hover(&mut self, hover: Option<usize>, now: Instant) {
        if hover == self.hover {
            return;
        }
        for (index, target) in [(self.hover, 0.0), (hover, 1.0)] {
            if let Some(i) = index {
                let current = self.glow_at(i, now);
                self.glow[i] = (current, target, now);
            }
        }
        self.hover = hover;
    }

    /// The next selectable item from `from` in `step` direction, wrapping.
    fn step(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        let n = self.items.len();
        let mut i = from;
        for _ in 0..n {
            i = Some(match (i, forward) {
                (None, true) => 0,
                (None, false) => n - 1,
                (Some(i), true) => (i + 1) % n,
                (Some(i), false) => (i + n - 1) % n,
            });
            if i.is_some_and(|i| selectable(&self.items[i])) {
                return i;
            }
        }
        None
    }

    fn draw(
        &self,
        canvas: &mut Canvas,
        palette: &Palette,
        open_child: Option<usize>,
        now: Instant,
    ) {
        let frame = self.frame;
        match self.look {
            Look::Acrylic => canvas.round_rect(frame, RADIUS, palette.veil),
            Look::Solid => {
                // DropShadowEffect: blur 16, depth 4 down-right, 0.3.
                for i in 0..6 {
                    let spread = i as f32 * 1.4;
                    canvas.round_rect(
                        frame.offset(2.8, 2.8).inset(-spread, -spread),
                        RADIUS + spread,
                        Rgba::rgba(0, 0, 0, 0.3 / 6.0 * (1.0 - i as f32 / 7.0)),
                    );
                }
                canvas.round_rect(frame, RADIUS, palette.border);
                canvas.round_rect(
                    frame.inset(BORDER, BORDER),
                    RADIUS - BORDER,
                    palette.background,
                );
            }
        }
        let style = TextStyle::new(FONT, 13.0).ellipsis();
        for (i, (item, rect)) in self.items.iter().zip(&self.rects).enumerate() {
            if matches!(item, Item::Separator) {
                let line = Rect::new(rect.x + 8.0, rect.y + 3.0, rect.w - 16.0, 1.0);
                canvas.rect(line, palette.separator);
                continue;
            }
            let glow = if open_child == Some(i) {
                1.0
            } else {
                self.glow_at(i, now)
            };
            if glow > 0.0 {
                let color = palette.highlight;
                canvas.round_rect(*rect, 4.0, color.alpha(glow));
            }
            let enabled = selectable(item);
            let ink = if enabled {
                palette.text
            } else {
                palette.disabled
            };
            let column = Rect::new(rect.x, rect.y, ICON_COLUMN, rect.h);
            let glyph = |canvas: &mut Canvas, text: &str, size: f32| {
                let style = TextStyle::new(GLYPHS, size).align(Align::Center);
                canvas.text(text, column, &style, palette.secondary);
            };
            match item {
                Item::Command(_, _, true, _) => glyph(canvas, GLYPH_CHECK, 12.0),
                Item::Icon(_, _, path, scale) => {
                    // A 16 DIP Viewbox nudged (-1.5, 1), at 0.76 opacity
                    // (1 when highlighted).
                    let (cx, cy) = column.center();
                    let box_ = Rect::new(cx - 8.0 - 1.5, cy - 8.0 + 1.0, 16.0, 16.0);
                    canvas.push_transform(0.0, 0.0, *scale, cx - 1.5, cy + 1.0);
                    let opacity = anim::lerp(0.76, 1.0, glow);
                    canvas.path(path, box_, ink.alpha(opacity));
                    canvas.pop_transform();
                }
                Item::Submenu(..) => glyph(canvas, GLYPH_ARROW, 10.0),
                _ => {}
            }
            let text = label(text_of(item).unwrap_or_default()).0;
            let text_rect = Rect::new(
                rect.x + ICON_COLUMN + 2.0,
                rect.y,
                (rect.w - ICON_COLUMN - 10.0).max(0.0),
                rect.h,
            );
            canvas.text(&text, text_rect, &style, ink);
        }
    }

    fn paint(&mut self, palette: &Palette, open_child: Option<usize>, now: Instant) {
        let (w, h) = self.pixels();
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        surface.draw(w, h, self.scale, |canvas| {
            self.draw(canvas, palette, open_child, now)
        });
        if let Some(window) = self.window {
            surface.present(window.raw(), self.position.0, self.position.1, 1.0);
        }
        self.surface = Some(surface);
    }
}

/// An open menu and its submenus.
struct Menu<'a> {
    gfx: Rc<Gfx>,
    palette: Palette,
    levels: Vec<Level<'a>>,
    chosen: Option<u32>,
    done: bool,
    /// The window in the foreground once the menu opened.
    foreground: Window,
    /// A submenu to open (level, item) or close (level, None) when
    /// SUBMENU fires.
    pending: Option<(usize, Option<usize>)>,
    pointer: (i32, i32),
}

impl<'a> Menu<'a> {
    fn root(&self) -> Option<Window> {
        self.levels.first().and_then(|l| l.window)
    }

    /// Which level a screen point is over, deepest first.
    fn level_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.levels.len())
            .rev()
            .find(|&i| self.levels[i].contains(x, y))
    }

    fn open_child(&self, level: usize) -> Option<usize> {
        self.levels.get(level + 1).and_then(|l| l.parent)
    }

    fn paint(&mut self) {
        let now = Instant::now();
        let mut fading = false;
        for i in 0..self.levels.len() {
            let child = self.open_child(i);
            let palette = self.palette;
            self.levels[i].paint(&palette, child, now);
            fading |= self.levels[i].fading(now);
        }
        if let Some(root) = self.root() {
            if fading {
                root.set_timer(FRAME, 15);
            } else {
                root.kill_timer(FRAME);
            }
        }
    }

    fn close_from(&mut self, level: usize) {
        while self.levels.len() > level.max(1) {
            if let Some(mut closed) = self.levels.pop() {
                if let Some(window) = closed.window.take() {
                    window.destroy();
                }
            }
        }
    }

    fn finish(&mut self, chosen: Option<u32>) {
        self.chosen = chosen;
        self.done = true;
    }

    /// Opens the submenu of item `index` on `level` (closing any other
    /// below it); with `select`, its first item is selected (keyboard).
    fn open_submenu(&mut self, level: usize, index: usize, select: bool) {
        if self.open_child(level) == Some(index) {
            if select {
                let child = &mut self.levels[level + 1];
                if child.hover.is_none() {
                    let first = child.step(None, true);
                    child.set_hover(first, Instant::now());
                }
            }
            return;
        }
        self.close_from(level + 1);
        let Item::Submenu(_, children) = &self.levels[level].items[index] else {
            return;
        };
        let parent = &self.levels[level];
        let scale = parent.scale;
        let item = parent.item_screen(index);
        let monitor = wnd::monitor_at(item.2, item.1);
        let max_width = monitor.work.2 as f32 / scale;
        let Some(mut child) = Level::new(&self.gfx, children, Look::Acrylic, scale, max_width)
        else {
            return;
        };
        let gap = (2.0 * scale).round() as i32;
        child.position = place_submenu(child.pixels(), item, gap, &monitor);
        child.parent = Some(index);
        if select {
            let first = child.step(None, true);
            child.set_hover(first, Instant::now());
        }
        let Ok(window) = create_window(false) else {
            return;
        };
        style_window(window, Look::Acrylic, &self.palette);
        child.window = Some(window);
        self.levels.push(child);
        self.paint();
        window.show(false);
    }

    /// Which item each level lights (to repaint only on a change).
    fn hovers(&self) -> Vec<Option<usize>> {
        self.levels.iter().map(|l| l.hover).collect()
    }

    /// The pointer moved to a screen point: it lights the item under it
    /// (an item whose submenu is open stays lit) and, after the menu show
    /// delay, opens that item's submenu or closes another one.
    fn pointer_at(&mut self, x: i32, y: i32) {
        self.pointer = (x, y);
        let now = Instant::now();
        let before = self.hovers();
        match self.level_at(x, y) {
            None => {
                let deepest = self.levels.len() - 1;
                let keep = self.open_child(deepest);
                self.levels[deepest].set_hover(keep, now);
                self.pending = None;
            }
            Some(level) => {
                for upper in 0..level {
                    let keep = self.open_child(upper);
                    self.levels[upper].set_hover(keep, now);
                }
                if self.pending.is_some_and(|(l, _)| l < level) {
                    self.pending = None;
                }
                let item = self.levels[level].item_at(x, y);
                let open = self.open_child(level);
                self.levels[level].set_hover(item.or(open), now);
                if let Some(i) = item {
                    let wanted =
                        matches!(self.levels[level].items[i], Item::Submenu(..)).then_some(i);
                    if wanted == open {
                        self.pending = None;
                    } else if self.pending != Some((level, wanted)) {
                        self.pending = Some((level, wanted));
                        if let Some(root) = self.root() {
                            root.set_timer(SUBMENU, SUBMENU_DELAY);
                        }
                    }
                }
            }
        }
        if self.hovers() != before {
            self.paint();
        }
    }

    fn submenu_timer(&mut self) {
        if let Some(root) = self.root() {
            root.kill_timer(SUBMENU);
        }
        match self.pending.take() {
            Some((level, Some(index))) if level < self.levels.len() => {
                self.open_submenu(level, index, false)
            }
            Some((level, None)) if level < self.levels.len() => {
                self.close_from(level + 1);
                self.paint();
            }
            _ => {}
        }
    }

    /// Chooses an item: runs a command, or opens a submenu.
    fn activate(&mut self, level: usize, index: usize, keyboard: bool) {
        match &self.levels[level].items[index] {
            Item::Command(_, id, _, true) | Item::Icon(_, id, ..) if *id != 0 => {
                let id = *id;
                self.finish(Some(id));
            }
            Item::Submenu(..) => {
                self.pending = None;
                self.open_submenu(level, index, keyboard);
            }
            _ => {}
        }
    }

    fn key(&mut self, vk: u16, alt: bool) {
        const VK_RETURN: u16 = 0x0d;
        const VK_MENU: u16 = 0x12;
        const VK_ESCAPE: u16 = 0x1b;
        const VK_SPACE: u16 = 0x20;
        const VK_END: u16 = 0x23;
        const VK_HOME: u16 = 0x24;
        const VK_LEFT: u16 = 0x25;
        const VK_UP: u16 = 0x26;
        const VK_RIGHT: u16 = 0x27;
        const VK_DOWN: u16 = 0x28;
        const VK_F10: u16 = 0x79;
        let now = Instant::now();
        let level = self.levels.len() - 1;
        let hover = self.levels[level].hover;
        self.pending = None;
        match vk {
            VK_ESCAPE | VK_LEFT if level > 0 => {
                self.close_from(level);
            }
            VK_ESCAPE | VK_MENU | VK_F10 => {
                self.finish(None);
                return;
            }
            _ if alt => {
                self.finish(None);
                return;
            }
            VK_UP | VK_DOWN => {
                let next = self.levels[level].step(hover, vk == VK_DOWN);
                self.levels[level].set_hover(next, now);
            }
            VK_HOME | VK_END => {
                let next = self.levels[level].step(None, vk == VK_HOME);
                self.levels[level].set_hover(next, now);
            }
            VK_RIGHT => {
                if let Some(i) = hover {
                    if matches!(self.levels[level].items[i], Item::Submenu(..)) {
                        self.open_submenu(level, i, true);
                    }
                }
            }
            VK_RETURN | VK_SPACE => {
                if let Some(i) = hover {
                    self.activate(level, i, true);
                }
            }
            0x30..=0x39 | 0x41..=0x5a => {
                // Access keys: one match runs; several cycle.
                let key = char::from(vk as u8);
                let items = self.levels[level].items;
                let matches: Vec<usize> = (0..items.len())
                    .filter(|&i| selectable(&items[i]))
                    .filter(|&i| text_of(&items[i]).and_then(|t| label(t).1) == Some(key))
                    .collect();
                match matches.as_slice() {
                    [] => {}
                    [only] => {
                        self.levels[level].set_hover(Some(*only), now);
                        self.activate(level, *only, true);
                    }
                    many => {
                        let next = many
                            .iter()
                            .copied()
                            .find(|&i| hover.is_some_and(|h| i > h))
                            .unwrap_or(many[0]);
                        self.levels[level].set_hover(Some(next), now);
                    }
                }
            }
            _ => {}
        }
        if !self.done {
            self.paint();
        }
    }

    fn is_menu_window(&self, window: Window) -> bool {
        self.levels.iter().any(|l| l.window == Some(window))
    }

    /// Input from the nested loop; true swallows it.
    fn input(&mut self, input: ModalInput) -> bool {
        match input {
            ModalInput::MouseMove(_, x, y) => {
                self.pointer_at(x, y);
                true
            }
            ModalInput::MouseDown(_, _, x, y) => {
                if self.level_at(x, y).is_none() {
                    self.finish(None);
                }
                true
            }
            ModalInput::MouseUp(_, button, x, y) => {
                if button != Button::Middle {
                    if let Some(level) = self.level_at(x, y) {
                        if let Some(index) = self.levels[level].item_at(x, y) {
                            self.activate(level, index, false);
                        }
                    }
                }
                true
            }
            ModalInput::Wheel(_) | ModalInput::KeyUp(_) => true,
            ModalInput::Key(vk, m) => {
                self.key(vk, m.alt);
                true
            }
            ModalInput::Timer(window, id) if Some(window) == self.root() => {
                match id {
                    FRAME => self.paint(),
                    SUBMENU => self.submenu_timer(),
                    CHECK => self.check(),
                    _ => {}
                }
                true
            }
            ModalInput::Timer(window, _) => self.is_menu_window(window),
        }
    }

    /// Closes when another window comes forward or the pointer is pressed
    /// outside the menu (in any app); follows the pointer outside this
    /// thread's windows.
    fn check(&mut self) {
        if wnd::foreground() != self.foreground {
            self.finish(None);
            return;
        }
        let (x, y) = wnd::cursor_pos();
        let inside = self.level_at(x, y).is_some();
        if wnd::button_pressed() && !inside {
            self.finish(None);
            return;
        }
        if (x, y) != self.pointer && !inside {
            self.pointer_at(x, y);
        }
    }
}

fn create_window(activate: bool) -> windows::core::Result<Window> {
    wnd::create(
        "BetterTrumpet menu",
        wnd::Options {
            activate,
            click_through: false,
        },
    )
}

fn style_window(window: Window, look: Look, palette: &Palette) {
    if look == Look::Acrylic {
        window.set_acrylic(palette.tint);
        window.round_corners();
    }
}

/// Shows `items` as a menu owned by `owner` at screen point (`x`, `y`)
/// and returns the chosen command id. `tray` places it against the
/// taskbar (the tray menu); otherwise it opens at the point, upwards with
/// `above`. Fails (for a native fallback) when its windows cannot be made.
#[allow(clippy::too_many_arguments)]
pub fn popup(
    gfx: &Rc<Gfx>,
    theme: &Theme,
    owner: Window,
    items: &[Item],
    x: i32,
    y: i32,
    above: bool,
    tray: bool,
) -> Result<Option<u32>, ()> {
    if items.is_empty() {
        return Ok(None);
    }
    let palette = Palette::new(theme);
    let monitor = wnd::monitor_at(x, y);
    let scale = monitor.scale;
    let look = if tray { Look::Acrylic } else { Look::Solid };
    let max_width = monitor.work.2 as f32 / scale - 2.0 * TRAY_GAP;
    let mut root = Level::new(gfx, items, look, scale, max_width).ok_or(())?;
    root.position = if tray {
        let gap = (TRAY_GAP * scale).round().max(1.0) as i32;
        place_tray(root.pixels(), (x, y), &monitor, gap)
    } else {
        place_at(root.pixels(), (x, y), above, &monitor)
    };
    // The tray menu's first level takes the foreground (the original's
    // SetForegroundWindow on its popup); other menus leave it with their
    // window, which keeps the keyboard and stays active.
    let window = create_window(tray).map_err(|_| ())?;
    style_window(window, look, &palette);
    root.window = Some(window);
    let mut menu = Menu {
        gfx: gfx.clone(),
        palette,
        levels: vec![root],
        chosen: None,
        done: false,
        foreground: owner,
        pending: None,
        pointer: wnd::cursor_pos(),
    };
    menu.paint();
    if tray {
        window.show(true);
        window.focus();
    } else {
        window.show(false);
        if !owner.is_foreground() {
            owner.focus();
        }
    }
    menu.foreground = wnd::foreground();
    // Clears presses recorded before the menu opened.
    let _ = wnd::button_pressed();
    window.set_timer(CHECK, 50);
    let menu = std::cell::RefCell::new(menu);
    wnd::run_modal(
        |input| menu.borrow_mut().input(input),
        || menu.borrow().done,
    );
    let mut menu = menu.into_inner();
    menu.close_from(1);
    if let Some(window) = menu.levels.first_mut().and_then(|l| l.window.take()) {
        window.destroy();
    }
    Ok(menu.chosen)
}

/// Draws a menu without showing it (`--render`): the tray look over its
/// acrylic tint colour, or a window's context menu; `hover` lights an
/// item.
pub fn render(
    gfx: &Rc<Gfx>,
    theme: &Theme,
    items: &[Item],
    tray: bool,
    hover: Option<usize>,
) -> Option<(u32, u32, Vec<u8>)> {
    let palette = Palette::new(theme);
    let look = if tray { Look::Acrylic } else { Look::Solid };
    let scale = wnd::monitor_at(0, 0).scale;
    let mut level = Level::new(gfx, items, look, scale, 1000.0)?;
    let past = Instant::now() - std::time::Duration::from_secs(1);
    level.set_hover(hover, past);
    let (w, h) = level.pixels();
    let mut surface = Surface::new(gfx.clone()).ok()?;
    surface.draw(w, h, scale, |canvas| {
        if look == Look::Acrylic {
            canvas.round_rect(level.frame, RADIUS + 1.0, palette.tint_color);
        }
        level.draw(canvas, &palette, None, Instant::now());
    });
    surface.rgba()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(work: (i32, i32, i32, i32)) -> Monitor {
        Monitor {
            bounds: (0, 0, 1920, 1080),
            work,
            scale: 1.0,
        }
    }

    #[test]
    fn labels_and_access_keys() {
        assert_eq!(label("E&xit"), ("Exit".into(), Some('X')));
        assert_eq!(label("Fish && chips"), ("Fish & chips".into(), Some('F')));
        assert_eq!(label("  42 apps"), ("  42 apps".into(), Some('4')));
        assert_eq!(label("open settings"), ("open settings".into(), Some('O')));
    }

    #[test]
    fn tray_menu_sits_against_each_taskbar_edge() {
        let size = (200, 300);
        // Bottom taskbar: 8 px above the work area's bottom, at the click.
        let bottom = monitor((0, 0, 1920, 1032));
        assert_eq!(
            place_tray(size, (1700, 1050), &bottom, 8),
            (1700, 1032 - 300 - 8)
        );
        // Near the right edge it stays inside.
        assert_eq!(
            place_tray(size, (1900, 1050), &bottom, 8),
            (1920 - 200 - 8, 724)
        );
        let top = monitor((0, 48, 1920, 1032));
        assert_eq!(place_tray(size, (1700, 20), &top, 8).1, 56);
        let left = monitor((48, 0, 1872, 1080));
        assert_eq!(place_tray(size, (20, 500), &left, 8), (56, 500));
        let right = monitor((0, 0, 1872, 1080));
        assert_eq!(
            place_tray(size, (1900, 500), &right, 8),
            (1872 - 200 - 8, 500)
        );
    }

    #[test]
    fn context_menu_flips_at_the_work_area() {
        let m = monitor((0, 0, 1920, 1040));
        assert_eq!(place_at((200, 300), (100, 100), false, &m), (100, 100));
        assert_eq!(place_at((200, 300), (100, 900), false, &m), (100, 600));
        assert_eq!(place_at((200, 300), (100, 500), true, &m), (100, 200));
        assert_eq!(place_at((200, 300), (100, 100), true, &m), (100, 100));
        assert_eq!(place_at((200, 300), (1800, 100), false, &m), (1600, 100));
    }

    #[test]
    fn submenus_open_right_then_left() {
        let m = monitor((0, 0, 1920, 1040));
        assert_eq!(
            place_submenu((200, 100), (100, 50, 300, 80), 2, &m),
            (302, 50)
        );
        assert_eq!(
            place_submenu((200, 100), (1600, 50, 1800, 80), 2, &m),
            (1398, 50)
        );
        assert_eq!(
            place_submenu((200, 100), (100, 1000, 300, 1030), 2, &m),
            (302, 940)
        );
    }

    #[test]
    fn separators_disabled_items_and_empty_submenus_are_skipped() {
        let items = [
            Item::disabled("No devices"),
            Item::Separator,
            Item::command("A", 1),
            Item::Submenu("Empty".into(), vec![]),
            Item::command("B", 2),
        ];
        assert!(!selectable(&items[0]));
        assert!(!selectable(&items[3]));
        assert!(selectable(&items[2]));
        let selectables: Vec<usize> = (0..items.len())
            .filter(|&i| selectable(&items[i]))
            .collect();
        assert_eq!(selectables, vec![2, 4]);
    }
}
