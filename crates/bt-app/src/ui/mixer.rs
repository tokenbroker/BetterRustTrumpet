//! The volume mixer window, after the original's FullWindow /
//! FullWindowViewModel: every visible output side by side as a 360 DIP
//! column (DeviceView) with its apps, the same sliders, meters, mute
//! buttons, popups and menus as the flyout, in a top-level window with its
//! own title bar and close button over acrylic (`WindowStyle="None"`,
//! `AllowsTransparency`). Up to three outputs it hugs its content and
//! cannot be resized; with more it scrolls, resizes and remembers its size
//! (WindowSizePolicy). Its placement is kept in FullMixerWindowPlacement.
//! Closing destroys the window; meters run only while it is shown.
use super::{
    MenuCommand, MenuRequest,
    anim::{self, Curve, FADE_OUT_QUAD},
    blur,
    rows::{
        self, APP_ROW_H, APPS_PAD, BLUR_RADIUS, Command, DEVICE_ROW_H, GLYPH_CLOSE, PopupKind,
        RowPaint, RowPart, SliderKey, TITLE_H, Ticker, Tool, ToolStep, UI_FONT, glyph, mute_rect,
        slider_rect, sorted_apps,
    },
    slider::Slider,
    theme::Theme,
    tip::Tip,
};
use crate::core::{Core, DeviceView, View};
use bt_app::i18n::tr;
use bt_app::win32::{
    gfx::{Align, Canvas, Gfx, Image, Rect, Rgba, Surface, TextStyle},
    theme as system,
    wnd::{self, Button, Chrome, Event, Modifiers, Placement, Window},
};
use bt_core::{
    legacy_xml::{self, WindowPlacement},
    settings::{Key, Value},
};
use std::{
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

/// WindowAndItemSize: one output's column.
const COLUMN: f32 = 360.0;
/// The window border (DialogWindowStyle's BorderThickness).
const BORDER: f32 = 1.0;
/// The title row: the title's 8 DIP padding around a 12 DIP line, or the
/// 31 DIP caption buttons below their 1 DIP margin.
const BAR_H: f32 = 32.0;
const CLOSE_W: f32 = 46.0;
/// WindowSizePolicy.
const SMALL_DEVICE_COUNT: usize = 3;
const MIN_HEIGHT: f32 = 200.0;
const MAX_SIZE: f32 = 4096.0;
/// The strip under the columns in many-devices mode.
const STRIP_H: f32 = 12.0;
/// The scroll bars' hit width.
const BAR_HIT: f32 = 16.0;
/// The FullMixerWindowPlacement setting.
const PLACEMENT: &str = "FullMixerWindowPlacement";
const FRAME: usize = 1;
const TIP_SHOW: usize = 2;
const TIP_HIDE: usize = 3;
const TIP_DELAY: u32 = 400;
const TIP_BETWEEN: u128 = 100;
const TIP_DURATION: u32 = 5000;
/// WindowAnimationLibrary: entrance and exit fades (QuadraticEase).
const ENTER_MS: f32 = 165.0;
const EXIT_MS: f32 = 105.0;
const QUAD_IN: Curve = Curve(0.11, 0.0, 0.5, 0.0);
/// The inactive (opaque) background's fade, and the popup's.
const FADE_MS: f32 = 150.0;

#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Close,
    Mute(SliderKey),
    Slider(SliderKey),
    Row(SliderKey),
    /// A column's title (part of the device item).
    DeviceTitle(String),
    Tool(usize),
    Dismiss,
    PopupBody,
    VScroll,
    HScroll,
}

#[derive(Clone, Debug, PartialEq)]
enum TipAnchor {
    Right(Rect),
    Mouse,
}

#[derive(Clone, Debug)]
struct TipTarget {
    hit: Hit,
    text: String,
    delay: u32,
    anchor: TipAnchor,
    done: bool,
}

struct Popup {
    kind: PopupKind,
    title: String,
    tools: Vec<Tool>,
    /// The column's left edge and the row's top at opening.
    anchor: (f32, f32),
    offset: f32,
    opened: Instant,
}

#[derive(Clone, Debug)]
struct Row {
    rect: Rect,
    key: SliderKey,
}

struct Column {
    rect: Rect,
    name: String,
}

#[derive(Default)]
struct Layout {
    columns: Vec<Column>,
    rows: Vec<Row>,
}

/// The mixer's own colours (`Theme:Options.Source="App"`: the app theme,
/// AppsUseLightTheme, rather than the taskbar's).
#[derive(Clone, Copy, Debug)]
struct Colors {
    /// Background: the app lists, popups and the light-dismiss layer.
    background: Rgba,
    /// AcrylicBackground over the acrylic (all but transparent), or its
    /// opaque fallback without transparency effects.
    base: Rgba,
    /// AcrylicBackgroundFallback, shown while the window is inactive.
    inactive: Rgba,
    /// ActiveWindowBorder.
    border: Rgba,
    /// AcrylicColor_Settings as an ABGR tint, `None` without transparency.
    acrylic: Option<u32>,
    /// The close button's pressed glyph colour.
    close_pressed_text: Rgba,
}

fn abgr(color: Rgba, alpha: f32) -> u32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(alpha) << 24) | (byte(color.b) << 16) | (byte(color.g) << 8) | byte(color.r)
}

/// The flyout theme recoloured for an app window: Text, Background,
/// PopupBackground and the hover colours follow the app theme; custom
/// text and window colours still apply (ApplyExtendedThemeColors).
fn palette(base: &Theme, light: bool) -> (Theme, Colors) {
    let mut theme = base.clone();
    let custom_text = base.custom.and_then(|c| c.text);
    let custom_window = base.custom.and_then(|c| c.window);
    theme.light = light;
    theme.accent_mode = false;
    theme.text = custom_text.unwrap_or(if light { Rgba::BLACK } else { Rgba::WHITE });
    (theme.hover, theme.pressed) = if light {
        (Rgba::rgba(0, 0, 0, 0.1), Rgba::rgba(0, 0, 0, 0.16))
    } else {
        (
            Rgba::rgba(255, 255, 255, 0.125),
            Rgba::rgba(255, 255, 255, 0.184),
        )
    };
    let background = custom_window.unwrap_or(if light {
        Rgba::WHITE
    } else {
        Rgba::rgb(21, 21, 21)
    });
    theme.popup_bg = background;
    theme.dismiss = background;
    // FlyoutThemeTrackRightBackground.
    theme.track_rest = if base.accent_mode {
        Rgba::rgba(255, 255, 255, 0.5)
    } else if light {
        Rgba::rgba(0, 0, 0, 0.224)
    } else {
        Rgba::rgba(255, 255, 255, 0.224)
    };
    // {Light,Dark}AcrylicWindowBackdropFallback.
    let fallback = custom_window.unwrap_or(if light {
        Rgba::rgb(249, 249, 249)
    } else {
        Rgba::rgb(31, 31, 31)
    });
    let border = if system::accent_on_title_bars() {
        base.accent
    } else if light {
        Rgba::rgb(122, 122, 122)
    } else {
        Rgba::rgb(133, 133, 133)
    };
    let colors = Colors {
        background,
        base: if base.transparency {
            Rgba::rgba(0, 0, 0, 1.0 / 255.0)
        } else {
            fallback
        },
        inactive: fallback,
        border,
        acrylic: base.transparency.then(|| {
            if light {
                abgr(Rgba::WHITE, 0.6)
            } else {
                abgr(Rgba::BLACK, 0.7)
            }
        }),
        close_pressed_text: if light { theme.text } else { Rgba::BLACK },
    };
    (theme, colors)
}

fn to_settings(p: &Placement) -> WindowPlacement {
    WindowPlacement {
        length: 44,
        flags: p.flags,
        show_cmd: p.show_cmd,
        pt_min_position: legacy_xml::Point {
            x: p.min_position.0,
            y: p.min_position.1,
        },
        pt_max_position: legacy_xml::Point {
            x: p.max_position.0,
            y: p.max_position.1,
        },
        rc_normal_position: legacy_xml::Rect {
            left: p.normal.0,
            top: p.normal.1,
            right: p.normal.2,
            bottom: p.normal.3,
        },
    }
}

fn from_settings(p: &WindowPlacement) -> Placement {
    let r = &p.rc_normal_position;
    Placement {
        flags: p.flags,
        show_cmd: p.show_cmd,
        min_position: (p.pt_min_position.x, p.pt_min_position.y),
        max_position: (p.pt_max_position.x, p.pt_max_position.y),
        normal: (r.left, r.top, r.right, r.bottom),
    }
}

/// WindowSizePolicy.TryNormalize.
fn normalize(width: f64, height: f64) -> Option<(f32, f32)> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some((
        width.clamp(f64::from(COLUMN), f64::from(MAX_SIZE)) as f32,
        height.clamp(f64::from(MIN_HEIGHT), f64::from(MAX_SIZE)) as f32,
    ))
}

pub struct Mixer {
    pub window: Window,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    /// The mixer's tooltips (app names, toggle mute, lock).
    pub tip: Tip,
    theme: Theme,
    colors: Colors,
    view: View,
    sliders: HashMap<SliderKey, Slider>,
    icons: HashMap<(String, u32), Option<Rc<Image>>>,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    capture: Option<(SliderKey, Rect)>,
    /// A scroll bar being dragged: horizontal, pointer start, scroll start.
    scroll_drag: Option<(bool, f32, f32)>,
    focus: Option<usize>,
    keyboard: bool,
    scroll: (f32, f32),
    popup: Option<Popup>,
    dismiss_out: Option<Instant>,
    backdrop: Option<(Rc<Image>, (i32, i32))>,
    hiding_app: Option<(String, String, Instant)>,
    /// Window size in DIPs (borders included) and pixels per DIP.
    size: (f32, f32),
    scale: f32,
    active: bool,
    active_changed: Option<Instant>,
    opened: Instant,
    closing: Option<Instant>,
    /// The exit fade finished: the app drops the window.
    pub closed: bool,
    /// Many-devices mode as of the last fit, and whether the saved size
    /// was applied in it.
    many: Option<bool>,
    restored_user_size: bool,
    ticker: Ticker,
    last_peaks: Instant,
    pointer: Option<(f32, f32)>,
    tip_target: Option<TipTarget>,
    tip_closed: Option<Instant>,
}

impl Mixer {
    pub fn new(gfx: Rc<Gfx>, base: &Theme) -> windows::core::Result<Self> {
        let window = wnd::create_chrome_app(tr("FullWindowTitleText"))?;
        window.round_corners();
        // FullWindow: FlowDirection="Auto".
        window.set_mirrored(bt_app::i18n::is_rtl());
        window.set_chrome(Chrome {
            caption: BORDER + BAR_H,
            buttons: CLOSE_W + BORDER,
            resizable: false,
        });
        let (theme, colors) = palette(base, system::apps_light());
        let now = Instant::now();
        Ok(Self {
            window,
            surface: Some(super::flow_surface(&gfx)?),
            tip: Tip::new(gfx.clone())?,
            gfx,
            theme,
            colors,
            view: View::default(),
            sliders: HashMap::new(),
            icons: HashMap::new(),
            hover: None,
            pressed: None,
            capture: None,
            scroll_drag: None,
            focus: None,
            keyboard: false,
            scroll: (0.0, 0.0),
            popup: None,
            dismiss_out: None,
            backdrop: None,
            hiding_app: None,
            size: (COLUMN + 2.0 * BORDER, MIN_HEIGHT),
            scale: window.scale(),
            active: true,
            active_changed: None,
            opened: now,
            closing: None,
            closed: false,
            many: None,
            restored_user_size: false,
            ticker: Ticker::new(),
            last_peaks: now,
            pointer: None,
            tip_target: None,
            tip_closed: None,
        })
    }

    /// Opens the window where it was last closed (FullMixerWindowPlacement)
    /// or centred on the monitor under the pointer, and fades it in.
    pub fn open(&mut self, core: &mut Core) {
        let saved = core.store.placement(PLACEMENT);
        let (cx, cy) = wnd::cursor_pos();
        let monitor = wnd::monitor_at(cx, cy);
        let (wx, wy, ww, wh) = monitor.work;
        match &saved {
            Some(saved) => self.window.set_placement(&from_settings(saved)),
            // On the pointer's monitor, centred once sized.
            None => self.window.set_bounds(wx, wy, 1, 1),
        }
        self.scale = self.window.scale();
        self.view = core.view();
        self.sync();
        self.fit(&core.store);
        if saved.is_none() {
            let (_, _, w, h) = self.window.bounds();
            self.window
                .set_bounds(wx + (ww - w) / 2, wy + (wh - h) / 2, w, h);
        }
        self.opened = Instant::now();
        self.window.set_acrylic(self.colors.acrylic);
        self.paint();
        self.window.present_app();
        self.window.set_timer(FRAME, 15);
    }

    /// Brings the open window forward (restoring it if minimised).
    pub fn raise(&mut self) {
        if self.closing.is_none() {
            self.window.present_app();
        }
    }

    pub fn closing(&self) -> bool {
        self.closing.is_some()
    }

    /// FullWindowViewModel.OnClosing: saves the placement (and the size in
    /// many-devices mode), closes the popup and plays the exit fade.
    pub fn close(&mut self, core: &mut Core) {
        if self.closing.is_some() {
            return;
        }
        if let Some(placement) = self.window.placement() {
            core.store
                .set_placement(PLACEMENT, &to_settings(&placement));
        }
        if self.many == Some(true) {
            core.store
                .set(Key::MixerWindowWidth, Value::Float(f64::from(self.size.0)));
            core.store
                .set(Key::MixerWindowHeight, Value::Float(f64::from(self.size.1)));
        }
        self.popup = None;
        self.dismiss_out = None;
        self.close_tip(false);
        if let Some((key, _)) = self.capture.take() {
            self.slider(&key).release(Instant::now());
            self.window.release_capture();
        }
        if self.window.is_minimized() || !self.theme.transparency {
            // No fade without transparency effects (or when not shown).
            self.closed = true;
            return;
        }
        self.closing = Some(Instant::now());
        self.window.set_timer(FRAME, 15);
    }

    /// Destroys the window and everything it holds.
    pub fn destroy(mut self) {
        self.window.kill_timer(FRAME);
        self.tip.hide();
        self.tip.window.destroy();
        self.surface = None;
        self.window.destroy();
    }

    /// New system or appearance colours.
    pub fn retheme(&mut self, base: &Theme) {
        let (theme, colors) = palette(base, system::apps_light());
        self.theme = theme;
        self.colors = colors;
        self.window.set_acrylic(self.colors.acrylic);
        self.backdrop = None;
        self.paint();
    }

    // ---------- devices and layout ----------

    /// VisibleDevices: by display name, ignoring case.
    fn shown(&self) -> Vec<&DeviceView> {
        let mut devices: Vec<&DeviceView> =
            self.view.devices.iter().filter(|d| !d.hidden).collect();
        devices.sort_by_cached_key(|d| d.device.name.to_lowercase());
        devices
    }

    fn many_devices(&self) -> bool {
        self.shown().len() > SMALL_DEVICE_COUNT
    }

    fn column_height(device: &DeviceView) -> f32 {
        TITLE_H + DEVICE_ROW_H + APPS_PAD * 2.0 + sorted_apps(device).len() as f32 * APP_ROW_H
    }

    fn no_devices_height(&self) -> f32 {
        self.gfx.measure_wrapped(
            tr("NoDevicesPanelContent"),
            &TextStyle::new(UI_FONT, 13.0).align(Align::Center),
            COLUMN - 48.0,
        ) + 48.0
    }

    /// The columns' natural size (the strip included).
    fn content(&self) -> (f32, f32) {
        let shown = self.shown();
        if shown.is_empty() {
            return (COLUMN, self.no_devices_height());
        }
        let height = shown
            .iter()
            .map(|d| Self::column_height(d))
            .fold(0.0, f32::max);
        let strip = if shown.len() > SMALL_DEVICE_COUNT {
            STRIP_H
        } else {
            0.0
        };
        (shown.len() as f32 * COLUMN, height + strip)
    }

    /// The scrolling area below the title row.
    fn viewport(&self) -> Rect {
        Rect::new(
            BORDER,
            BORDER + BAR_H,
            (self.size.0 - 2.0 * BORDER).max(0.0),
            (self.size.1 - 2.0 * BORDER - BAR_H).max(0.0),
        )
    }

    fn max_scroll(&self) -> (f32, f32) {
        let (w, h) = self.content();
        let view = self.viewport();
        ((w - view.w).max(0.0), (h - view.h).max(0.0))
    }

    fn close_rect(&self) -> Rect {
        Rect::new(
            self.size.0 - BORDER - CLOSE_W,
            BORDER + 1.0,
            CLOSE_W,
            BAR_H - 1.0,
        )
    }

    fn layout(&self) -> Layout {
        let mut out = Layout::default();
        let view = self.viewport();
        let (_, content_h) = self.content();
        let height = content_h.max(view.h);
        let top = view.y - self.scroll.1;
        for (i, device) in self.shown().into_iter().enumerate() {
            let x = view.x + i as f32 * COLUMN - self.scroll.0;
            let id = &device.device.id;
            out.columns.push(Column {
                rect: Rect::new(x, top, COLUMN, height),
                name: device.device.name.clone(),
            });
            out.rows.push(Row {
                rect: Rect::new(x, top + TITLE_H, COLUMN, DEVICE_ROW_H),
                key: SliderKey::device(id, false),
            });
            let mut y = top + TITLE_H + DEVICE_ROW_H + APPS_PAD;
            for app in sorted_apps(device) {
                out.rows.push(Row {
                    rect: Rect::new(x, y, COLUMN, APP_ROW_H),
                    key: SliderKey::app(id, &app.app.key, None, false),
                });
                y += APP_ROW_H;
            }
        }
        out
    }

    /// Sizes the window as the original does (FullWindow.OnDevicesChanged):
    /// up to three outputs it fits its content and cannot be resized;
    /// with more it is resizable between 360 DIP and the last column, and
    /// takes the user's saved size once.
    fn fit(&mut self, store: &crate::store::Store) {
        let count = self.shown().len();
        let many = count > SMALL_DEVICE_COUNT;
        let (content_w, content_h) = self.content();
        let natural = (content_w + 2.0 * BORDER, content_h + BAR_H + 2.0 * BORDER);
        let (x, y, _, _) = self.window.bounds();
        let monitor = wnd::monitor_at(
            x + (self.size.0 * self.scale / 2.0) as i32,
            y + (BAR_H * self.scale) as i32,
        );
        let work_h = monitor.work.3 as f32 / self.scale;
        let work_w = monitor.work.2 as f32 / self.scale;
        let max_w = if count == 0 {
            MAX_SIZE
        } else {
            2.0 * BORDER + count as f32 * COLUMN
        };
        let mut size = self.size;
        if many {
            if self.many != Some(true) {
                // Entering many-devices mode: the first layout's width
                // (three columns), then the user's size once.
                size = (
                    natural.0.min(SMALL_DEVICE_COUNT as f32 * COLUMN),
                    natural.1.min(work_h),
                );
                if !self.restored_user_size {
                    if let Some(saved) = normalize(
                        store.float(Key::MixerWindowWidth),
                        store.float(Key::MixerWindowHeight),
                    ) {
                        size = saved;
                        self.restored_user_size = true;
                    }
                }
            }
            size.0 = size.0.clamp(COLUMN, max_w);
            size.1 = size.1.max(MIN_HEIGHT);
            self.window
                .set_limits((COLUMN, MIN_HEIGHT), Some((max_w, MAX_SIZE)));
        } else {
            self.restored_user_size = false;
            // SizeToContent, kept on the monitor.
            size = (natural.0.min(work_w), natural.1.min(work_h));
            self.window.set_limits(size, Some(size));
        }
        self.window.set_chrome(Chrome {
            caption: BORDER + BAR_H,
            buttons: CLOSE_W + BORDER,
            resizable: many,
        });
        self.many = Some(many);
        let pixels = (
            (size.0 * self.scale).round() as i32,
            (size.1 * self.scale).round() as i32,
        );
        let (_, _, pw, ph) = self.window.bounds();
        if pixels != (pw, ph) {
            self.size = size;
            self.window.set_bounds(x, y, pixels.0, pixels.1);
        }
        self.size = size;
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let (mx, my) = self.max_scroll();
        self.scroll = (self.scroll.0.clamp(0.0, mx), self.scroll.1.clamp(0.0, my));
    }

    // ---------- popup layout ----------

    fn popup_rows(&self) -> Vec<SliderKey> {
        self.popup
            .as_ref()
            .map_or(vec![], |p| rows::popup_keys(&self.view, &p.kind))
    }

    /// The popup over its row, kept inside the window (the original's
    /// AppPopup keeps it inside the monitor's work area instead).
    fn popup_layout(&self) -> Option<(Rect, Vec<Row>)> {
        let popup = self.popup.as_ref()?;
        let keys = self.popup_rows();
        let row_h = if matches!(popup.kind, PopupKind::Device { .. }) {
            DEVICE_ROW_H
        } else {
            APP_ROW_H
        };
        let height = 1.0 + TITLE_H + APPS_PAD * 2.0 + row_h * keys.len() as f32 + 1.0;
        let top = (popup.anchor.1 - popup.offset)
            .min(self.size.1 - BORDER - height)
            .max(BORDER);
        let left = popup
            .anchor
            .0
            .min(self.size.0 - BORDER - COLUMN)
            .max(BORDER);
        let frame = Rect::new(left, top, COLUMN, height);
        let mut y = top + 1.0 + TITLE_H + APPS_PAD;
        let rows = keys
            .into_iter()
            .map(|key| {
                let row = Row {
                    rect: Rect::new(left + 1.0, y, COLUMN - 2.0, row_h),
                    key,
                };
                y += row_h;
                row
            })
            .collect();
        Some((frame, rows))
    }

    fn tool_rects(&self, frame: Rect) -> Vec<Rect> {
        self.popup
            .as_ref()
            .map_or(vec![], |p| rows::tool_rects(frame, p.tools.len()))
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
        if self.close_rect().contains(x, y) {
            return Some(Hit::Close);
        }
        let view = self.viewport();
        if !view.contains(x, y) {
            return None;
        }
        let (mx, my) = self.max_scroll();
        if my > 0.0 && x >= view.right() - BAR_HIT {
            return Some(Hit::VScroll);
        }
        if mx > 0.0 && y >= view.bottom() - BAR_HIT {
            return Some(Hit::HScroll);
        }
        let layout = self.layout();
        for row in &layout.rows {
            if let Some(hit) = Self::row_hit(row, x, y) {
                return Some(hit);
            }
        }
        for (column, row) in layout
            .columns
            .iter()
            .zip(layout.rows.iter().filter(|r| r.key.app.is_none()))
        {
            let title = Rect::new(column.rect.x, column.rect.y, COLUMN, TITLE_H);
            if title.contains(x, y) {
                return Some(Hit::DeviceTitle(row.key.device.clone()));
            }
        }
        None
    }

    fn row_hit(row: &Row, x: f32, y: f32) -> Option<Hit> {
        Some(match rows::row_part(row.rect, x, y)? {
            RowPart::Mute => Hit::Mute(row.key.clone()),
            RowPart::Slider => Hit::Slider(row.key.clone()),
            RowPart::Row => Hit::Row(row.key.clone()),
        })
    }

    // ---------- state from the view ----------

    fn locked(&self, key: &SliderKey) -> bool {
        rows::locked(&self.view, key)
    }

    fn slider(&mut self, key: &SliderKey) -> &mut Slider {
        let value = rows::volume_of(&self.view, key).map_or(0, |(v, _)| rows::percent(v));
        self.sliders
            .entry(key.clone())
            .or_insert_with(|| Slider::new(value))
    }

    /// Re-reads devices and apps (after Windows or another window changed
    /// them); sliders in use keep their value.
    pub fn refresh(&mut self, core: &mut Core) {
        if self.closing.is_some() {
            return;
        }
        self.view = core.view();
        self.sync();
        self.fit(&core.store);
        self.paint();
    }

    fn sync(&mut self) {
        let mut keys: Vec<SliderKey> = self.layout().rows.into_iter().map(|r| r.key).collect();
        if let Some((_, rows)) = self.popup_layout() {
            keys.extend(rows.into_iter().map(|r| r.key));
        }
        rows::sync_sliders(&self.view, &mut self.sliders, keys);
        if self
            .popup
            .as_ref()
            .is_some_and(|p| rows::popup_gone(&self.view, &p.kind))
        {
            self.popup = None;
            self.dismiss_out = None;
        }
        let rows = self.focus_rows().len();
        if self.focus.is_some_and(|f| f >= rows) {
            self.focus = rows.checked_sub(1);
        }
    }

    // ---------- frames ----------

    fn animating(&self, now: Instant) -> bool {
        anim::running(Some(self.opened), ENTER_MS, now)
            || self.closing.is_some()
            || anim::running(self.active_changed, FADE_MS, now)
            || self
                .popup
                .as_ref()
                .is_some_and(|p| anim::running(Some(p.opened), FADE_MS, now))
            || anim::running(self.dismiss_out, FADE_MS, now)
            || self.hiding_app.is_some()
            || self.sliders.values().any(Slider::animating)
    }

    /// Animations, and the peak meters at PeakMeterFps while shown.
    pub fn timer(&mut self, id: usize, core: &mut Core) {
        match id {
            TIP_SHOW => {
                self.window.kill_timer(TIP_SHOW);
                if self.closing.is_none() {
                    self.show_tip();
                }
                return;
            }
            TIP_HIDE => {
                self.close_tip(true);
                return;
            }
            FRAME => {}
            _ => return,
        }
        let now = Instant::now();
        if let Some(start) = self.closing {
            if anim::progress(start, 0.0, EXIT_MS, now) >= 1.0 {
                self.window.kill_timer(FRAME);
                self.closed = true;
                return;
            }
            self.paint();
            self.window.set_timer(FRAME, 15);
            return;
        }
        if self.window.is_minimized() {
            // Nothing is seen: no meters until it is restored.
            self.window.kill_timer(FRAME);
            return;
        }
        if let Some((device, key, start)) = self.hiding_app.clone() {
            if anim::progress(start, 0.0, 140.0, now) >= 1.0 {
                self.hiding_app = None;
                if let Some(app) = self.view.app(&device, &key).cloned() {
                    core.hide_app(&device, &app);
                }
                self.refresh(core);
            }
        }
        let keys: Vec<SliderKey> = self.sliders.keys().cloned().collect();
        for key in keys {
            if let Some(value) = self.slider(&key).tick(now) {
                self.apply_value(core, &key, value);
            }
        }
        let frame_ms = 1000.0 / self.theme.peak_fps as f32;
        let mut showing = false;
        if now.duration_since(self.last_peaks).as_secs_f32() * 1000.0 >= frame_ms - 2.0 {
            self.last_peaks = now;
            let popup = self.popup.as_ref().map(|p| &p.kind);
            rows::read_peaks(core, &self.view, &mut self.sliders, popup);
            for slider in self.sliders.values_mut() {
                showing |= slider.animate_peaks();
            }
        }
        let animating = self.animating(now);
        if animating || showing {
            self.paint();
        }
        let interval = if animating {
            15
        } else {
            frame_ms.round() as u32
        };
        self.window.set_timer(FRAME, interval);
    }

    // ---------- actions ----------

    fn apply_value(&mut self, core: &mut Core, key: &SliderKey, value: i32) {
        self.ticker.play(&self.theme, value);
        rows::apply_value(&mut self.view, &mut self.sliders, core, key, value);
    }

    fn toggle_mute(&mut self, core: &mut Core, key: &SliderKey) {
        rows::toggle_mute(&mut self.view, core, key);
        self.refresh(core);
    }

    fn solo(&mut self, core: &mut Core, device: &str, key: &str) {
        rows::solo(&self.view, core, device, key);
        self.refresh(core);
    }

    fn hide_app(&mut self, device: &str, key: &str) {
        self.hiding_app = Some((device.into(), key.into(), Instant::now()));
        self.window.set_timer(FRAME, 15);
    }

    /// A command from one of the mixer's menus.
    pub fn command(&mut self, core: &mut Core, command: Command) {
        if !rows::run_command(&self.view, core, command) {
            return;
        }
        self.close_popup();
        self.refresh(core);
    }

    // ---------- popups ----------

    /// FullWindowViewModel.OpenPopup for an app.
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
        self.popup = Some(Popup {
            kind: PopupKind::App {
                device: device.into(),
                key: key.into(),
            },
            title: app.app.display_name.clone(),
            tools: rows::app_tools(&app),
            anchor: (row.rect.x, row.rect.y),
            // Shifted up so its first row lies over the original row.
            offset: TITLE_H + APPS_PAD + 1.0,
            opened: Instant::now(),
        });
        self.after_popup_change();
    }

    /// FocusedDeviceViewModel, opened only when it has something to offer.
    fn open_device_popup(&mut self, device: &str) {
        let count = self.shown().len();
        if rows::device_menu(&self.view, device, count, &mut vec![]).is_empty() {
            return;
        }
        let Some(view) = self.view.device(device) else {
            return;
        };
        let title = view.device.name.clone();
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
            title,
            tools: rows::device_tools(),
            anchor: (row.rect.x, row.rect.y),
            offset: TITLE_H + APPS_PAD,
            opened: Instant::now(),
        });
        self.after_popup_change();
    }

    fn after_popup_change(&mut self) {
        self.dismiss_out = None;
        self.focus = Some(0);
        self.close_tip(false);
        self.sync();
        self.window.set_timer(FRAME, 15);
    }

    fn close_popup(&mut self) {
        if self.popup.take().is_some() {
            self.dismiss_out = Some(Instant::now());
            self.focus = None;
            self.sync();
            self.window.set_timer(FRAME, 15);
        }
    }

    fn tool(&mut self, core: &mut Core, index: usize) -> Option<MenuRequest> {
        let popup = self.popup.as_ref()?;
        let action = popup.tools.get(index)?.action;
        let (frame, _) = self.popup_layout()?;
        let button = *self.tool_rects(frame).get(index)?;
        // Below the button's leading edge (its right edge when mirrored,
        // which is where x counts from).
        let (x, y) = self.window.client_to_screen(button.x, button.bottom());
        let count = self.shown().len();
        match rows::tool_step(&self.view, &popup.kind, action, count)? {
            ToolStep::Close => {
                self.close_popup();
                None
            }
            ToolStep::Hide(device, key) => {
                self.close_popup();
                self.hide_app(&device, &key);
                None
            }
            ToolStep::Rule(device, app, kind) => {
                self.close_popup();
                core.app_rule(&device, &app, kind);
                self.refresh(core);
                None
            }
            ToolStep::Menu(items, commands) => Some(MenuRequest {
                owner: self.window,
                items,
                commands: commands.into_iter().map(MenuCommand::MixerWindow).collect(),
                x,
                y,
                above: false,
            }),
        }
    }

    // ---------- keyboard ----------

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

    /// Scrolls the focused row into view.
    fn reveal(&mut self) {
        if self.popup.is_some() {
            return;
        }
        let Some(row) = self.focused() else {
            return;
        };
        let view = self.viewport();
        if row.rect.x < view.x {
            self.scroll.0 -= view.x - row.rect.x;
        } else if row.rect.right() > view.right() {
            self.scroll.0 += row.rect.right() - view.right();
        }
        // The column's title stays in view with its device row.
        let top = if row.key.app.is_none() {
            row.rect.y - TITLE_H
        } else {
            row.rect.y
        };
        if top < view.y {
            self.scroll.1 -= view.y - top;
        } else if row.rect.bottom() > view.bottom() {
            self.scroll.1 += row.rect.bottom() - view.bottom();
        }
        self.clamp_scroll();
    }

    fn key(&mut self, core: &mut Core, vk: u16, m: Modifiers) -> Option<MenuRequest> {
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
        // Ctrl+Z / Ctrl+Y undo and redo.
        if m.ctrl && !m.alt && !m.shift && (vk == u16::from(b'Z') || vk == u16::from(b'Y')) {
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
                    self.close(core);
                }
            }
            VK_TAB | VK_UP | VK_DOWN if rows > 0 => {
                self.keyboard = true;
                let back = vk == VK_UP || (vk == VK_TAB && m.shift);
                let current = self.focus.unwrap_or(0);
                self.focus = Some(if self.focus.is_none() {
                    if back { rows - 1 } else { 0 }
                } else if back {
                    (current + rows - 1) % rows
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
                    self.apply_value(core, &row.key, value);
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

    fn slider_rect_of(&self, key: &SliderKey) -> Option<Rect> {
        let rows = if key.popup {
            self.popup_layout()?.1
        } else {
            self.layout().rows
        };
        rows.into_iter()
            .find(|r| &r.key == key)
            .map(|r| slider_rect(r.rect))
    }

    /// Handles a window event; may ask for a menu.
    pub fn event(&mut self, core: &mut Core, event: Event) -> Option<MenuRequest> {
        if self.closing.is_some() || self.closed {
            return None;
        }
        let now = Instant::now();
        let mut menu = None;
        match event {
            Event::Close => {
                self.close(core);
                return None;
            }
            Event::Resized | Event::Moved => {
                // FullWindowViewModel: moving or resizing closes the popup.
                self.close_popup();
                if self.window.is_minimized() {
                    self.close_tip(false);
                    return None;
                }
                let (_, _, pw, ph) = self.window.bounds();
                self.scale = self.window.scale();
                self.size = (pw as f32 / self.scale, ph as f32 / self.scale);
                self.clamp_scroll();
                // Restored: the meters run again.
                self.window.set_timer(FRAME, 15);
            }
            Event::DpiChanged => {
                self.scale = self.window.scale();
                self.icons.clear();
                self.backdrop = None;
                self.fit(&core.store);
            }
            Event::Activate(active) => {
                if active != self.active {
                    self.active = active;
                    self.active_changed = Some(now);
                    self.window.set_timer(FRAME, 15);
                }
                if !active {
                    self.close_tip(false);
                }
            }
            Event::MouseMove(x, y) => {
                self.pointer = Some((x, y));
                if let Some((key, rect)) = self.capture.clone() {
                    if !self.locked(&key) {
                        if let Some(value) = self.slider(&key).drag(x - rect.x, rect.w) {
                            self.apply_value(core, &key, value);
                        }
                    }
                } else if let Some((horizontal, start, from)) = self.scroll_drag {
                    let (content_w, content_h) = self.content();
                    let view = self.viewport();
                    let (mx, my) = self.max_scroll();
                    if horizontal {
                        let track = view.w - view.w * view.w / content_w;
                        self.scroll.0 = (from + (x - start) * mx / track.max(1.0)).clamp(0.0, mx);
                    } else {
                        let track = view.h - view.h * view.h / content_h;
                        self.scroll.1 = (from + (y - start) * my / track.max(1.0)).clamp(0.0, my);
                    }
                } else {
                    let hit = self.hit(x, y);
                    self.update_tip(hit.as_ref(), x, y);
                    if hit == self.hover {
                        return None;
                    }
                    self.hover = hit;
                }
            }
            Event::MouseLeave => {
                self.pointer = None;
                self.hover = None;
                self.close_tip(false);
            }
            Event::MouseDown(..) | Event::Wheel(..) | Event::Key(..)
                if self.tip_target.as_ref().is_some_and(|t| !t.done) =>
            {
                self.close_tip(true);
                return self.event(core, event);
            }
            Event::MouseDown(Button::Left, x, y, _) => {
                self.keyboard = false;
                let hit = self.hit(x, y);
                self.pressed = hit.clone();
                match hit {
                    Some(Hit::Slider(key)) if !self.locked(&key) => {
                        if let Some(rect) = self.slider_rect_of(&key) {
                            if let Some(index) = self.focus_rows().iter().position(|r| r.key == key)
                            {
                                self.focus = Some(index);
                            }
                            self.slider(&key);
                            let value = self.sliders.get_mut(&key).and_then(|s| {
                                s.press(x - rect.x, y - rect.y, rect.w, &self.theme, now)
                            });
                            if let Some(value) = value {
                                self.apply_value(core, &key, value);
                            }
                            self.capture = Some((key, rect));
                            self.window.capture();
                            self.window.set_timer(FRAME, 15);
                        }
                    }
                    Some(Hit::VScroll) => {
                        self.scroll_drag = Some((false, y, self.scroll.1));
                        self.window.capture();
                    }
                    Some(Hit::HScroll) => {
                        self.scroll_drag = Some((true, x, self.scroll.0));
                        self.window.capture();
                    }
                    // LightDismissBorder: a press outside closes the popup.
                    Some(Hit::Dismiss) => {
                        self.pressed = None;
                        self.close_popup();
                    }
                    _ => {}
                }
            }
            Event::MouseUp(Button::Left, x, y, m) => {
                if let Some((key, _)) = self.capture.take() {
                    self.slider(&key).release(now);
                    self.window.release_capture();
                    self.pressed = None;
                    self.paint();
                    return None;
                }
                if self.scroll_drag.take().is_some() {
                    self.window.release_capture();
                    return None;
                }
                let hit = self.hit(x, y);
                let pressed = self.pressed.take();
                if let Some(hit) = hit.filter(|h| Some(h) == pressed.as_ref()) {
                    menu = self.click(core, hit, m);
                }
            }
            Event::MouseDown(Button::Middle, x, y, _) => {
                // Middle-click on an app's icon hides it on this output.
                if let Some(Hit::Mute(key)) = self.hit(x, y) {
                    if let (Some(app), false) = (&key.app, key.popup) {
                        let (device, app) = (key.device.clone(), app.clone());
                        self.hide_app(&device, &app);
                    }
                }
            }
            Event::MouseDown(Button::Right, x, y, _)
                if matches!(self.hit(x, y), Some(Hit::Dismiss)) =>
            {
                self.close_popup();
            }
            Event::MouseUp(Button::Right, x, y, _) => match self.hit(x, y) {
                Some(Hit::Mute(key) | Hit::Slider(key) | Hit::Row(key)) if !key.popup => {
                    match &key.app {
                        Some(app) => self.open_app_popup(&key.device, &app.clone()),
                        None => self.open_device_popup(&key.device),
                    }
                }
                Some(Hit::DeviceTitle(device)) => self.open_device_popup(&device),
                _ => {}
            },
            Event::Wheel(delta, x, y) => {
                let notch = if delta > 0 { 1 } else { -1 };
                match self.hit(x, y) {
                    Some(Hit::Slider(key)) if !self.locked(&key) => {
                        if let Some(value) = self.slider(&key).step(2 * notch, now) {
                            self.apply_value(core, &key, value);
                        }
                    }
                    Some(Hit::Slider(_)) => {}
                    _ if self.popup.is_none() => {
                        let (mx, my) = self.max_scroll();
                        let step = f32::from(delta) / 120.0 * 48.0;
                        // The wheel scrolls down, or across when nothing
                        // is below.
                        if my > 0.0 {
                            self.scroll.1 = (self.scroll.1 - step).clamp(0.0, my);
                        } else if mx > 0.0 {
                            self.scroll.0 = (self.scroll.0 - step).clamp(0.0, mx);
                        }
                        if let Some((px, py)) = self.pointer {
                            self.hover = self.hit(px, py);
                        }
                    }
                    _ => {}
                }
            }
            Event::Key(vk, m) => menu = self.key(core, vk, m),
            Event::CaptureLost => {
                if let Some((key, _)) = self.capture.take() {
                    self.slider(&key).release(now);
                }
                self.scroll_drag = None;
            }
            _ => return None,
        }
        if !self.closed {
            self.paint();
        }
        menu
    }

    fn click(&mut self, core: &mut Core, hit: Hit, m: Modifiers) -> Option<MenuRequest> {
        match hit {
            Hit::Close => self.close(core),
            Hit::Mute(key) if m.ctrl && key.app.is_some() && !key.popup => {
                self.solo(core, &key.device, key.app.as_deref().unwrap_or_default());
            }
            Hit::Mute(key) => self.toggle_mute(core, &key),
            Hit::Row(key) if m.ctrl && key.app.is_some() && key.pid.is_none() && !key.popup => {
                self.solo(core, &key.device, key.app.as_deref().unwrap_or_default());
            }
            Hit::Tool(i) => return self.tool(core, i),
            _ => {}
        }
        None
    }

    // ---------- tooltips ----------

    /// As in the flyout: an app's name on its icon (ShowAppTooltips, at
    /// once, beside it), "Lock" on the lock badge and "Toggle mute" on an
    /// output's mute button (after the hover delay, below the pointer).
    fn tip_for(&self, hit: &Hit, x: f32, y: f32) -> Option<TipTarget> {
        let target = |text: &str, delay: u32, anchor: TipAnchor| TipTarget {
            hit: hit.clone(),
            text: text.into(),
            delay,
            anchor,
            done: false,
        };
        let Hit::Mute(key) = hit else {
            return None;
        };
        match &key.app {
            Some(app) => {
                let view = self.view.app(&key.device, app)?;
                let cell = mute_rect(self.row_rect(key)?);
                if view.locked() && rows::lock_badge(cell).contains(x, y) {
                    return Some(target(tr("AppRulesModeLockText"), 0, TipAnchor::Mouse));
                }
                let name = view.app.display_name.trim();
                (self.theme.show_tooltips && !name.is_empty())
                    .then(|| target(name, 0, TipAnchor::Right(cell)))
            }
            None => Some(target(tr("ToggleMuteToolTip"), TIP_DELAY, TipAnchor::Mouse)),
        }
    }

    fn row_rect(&self, key: &SliderKey) -> Option<Rect> {
        let rows = if key.popup {
            self.popup_layout()?.1
        } else {
            self.layout().rows
        };
        rows.into_iter().find(|r| &r.key == key).map(|r| r.rect)
    }

    fn update_tip(&mut self, hit: Option<&Hit>, x: f32, y: f32) {
        if self.capture.is_some() || self.scroll_drag.is_some() {
            return;
        }
        let wanted = hit.and_then(|h| self.tip_for(h, x, y));
        let same = match (&wanted, &self.tip_target) {
            (Some(w), Some(t)) => w.hit == t.hit && w.text == t.text,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.close_tip(false);
        self.tip_target = wanted;
        let Some(target) = &self.tip_target else {
            return;
        };
        let quick = self
            .tip_closed
            .is_some_and(|at| at.elapsed().as_millis() < TIP_BETWEEN);
        if target.delay == 0 || quick {
            self.show_tip();
        } else {
            self.window.set_timer(TIP_SHOW, target.delay);
        }
    }

    fn show_tip(&mut self) {
        let Some(target) = self.tip_target.as_ref().filter(|t| !t.done) else {
            return;
        };
        let scale = self.window.scale();
        let (x, y, flip) = match target.anchor {
            TipAnchor::Right(rect) => {
                let (x, y) = self.window.client_to_screen(rect.right(), rect.y);
                let (_, bottom) = self.window.client_to_screen(rect.right(), rect.bottom());
                (x, y, bottom)
            }
            TipAnchor::Mouse => {
                let (cx, cy) = wnd::cursor_pos();
                (cx, cy + (20.0 * scale).round() as i32, cy)
            }
        };
        let text = target.text.clone();
        let end = self.window.is_mirrored() && matches!(target.anchor, TipAnchor::Right(_));
        self.tip.show_at(&text, x, y, end, flip, self.theme.light);
        self.window.set_timer(TIP_HIDE, TIP_DURATION);
    }

    fn close_tip(&mut self, done: bool) {
        self.window.kill_timer(TIP_SHOW);
        self.window.kill_timer(TIP_HIDE);
        if self.tip.visible {
            self.tip.hide();
            self.tip_closed = Some(Instant::now());
        }
        if done {
            if let Some(target) = &mut self.tip_target {
                target.done = true;
            }
        } else {
            self.tip_target = None;
        }
    }

    // ---------- debug renders ----------

    /// Draws the mixer as it would open, without showing it, in the app
    /// theme (`light`) and with the first app's popup open (`popup`).
    pub fn render(
        &mut self,
        core: &mut Core,
        base: &Theme,
        light: bool,
        popup: bool,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let (theme, mut colors) = palette(base, light);
        // Nothing is behind an offscreen drawing: the opaque fallback
        // stands in for the acrylic.
        colors.base = colors.inactive;
        self.theme = theme;
        self.colors = colors;
        self.scale = 1.0;
        self.view = core.view();
        self.sync();
        let (w, h) = self.content();
        self.size = (
            w.min(SMALL_DEVICE_COUNT as f32 * COLUMN) + 2.0 * BORDER,
            h + BAR_H + 2.0 * BORDER,
        );
        let past = Instant::now() - Duration::from_secs(5);
        self.opened = past;
        self.popup = None;
        self.backdrop = None;
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
        self.draw_now(false);
        self.popup = None;
        self.backdrop = None;
        self.surface.as_ref()?.rgba()
    }

    // ---------- painting ----------

    pub fn paint(&mut self) {
        if self.closed || self.window.is_minimized() {
            return;
        }
        let (_, _, pw, ph) = self.window.bounds();
        self.scale = self.window.scale();
        self.size = (pw as f32 / self.scale, ph as f32 / self.scale);
        self.draw_now(true);
    }

    fn draw_now(&mut self, present: bool) {
        let now = Instant::now();
        let width = (self.size.0 * self.scale).round() as i32;
        let height = (self.size.1 * self.scale).round() as i32;
        let paths: Vec<String> = self
            .shown()
            .iter()
            .flat_map(|d| d.apps.iter())
            .filter_map(|a| a.app.executable.clone())
            .collect();
        for path in paths {
            rows::load_icon(&mut self.icons, &path, self.scale);
        }
        let opacity = match self.closing {
            Some(start) => 1.0 - QUAD_IN.at(anim::progress(start, 0.0, EXIT_MS, now)),
            None => FADE_OUT_QUAD.at(anim::progress(self.opened, 0.0, ENTER_MS, now)),
        };
        if self.popup.is_some() {
            if self
                .backdrop
                .as_ref()
                .is_none_or(|(_, size)| *size != (width, height))
            {
                self.backdrop = self
                    .blur_content(width, height, now)
                    .map(|image| (image, (width, height)));
            }
        } else if !anim::running(self.dismiss_out, FADE_MS, now) {
            self.backdrop = None;
        }
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        surface.draw(width, height, self.scale, |canvas| self.draw(canvas, now));
        if present {
            let (x, y, _, _) = self.window.bounds();
            surface.present(self.window.raw(), x, y, opacity);
        }
        self.surface = Some(surface);
    }

    /// The window's content drawn offscreen and blurred, for behind a
    /// popup (LightDismissBorder's VisualBrush of ContentRoot through a
    /// Gaussian BlurEffect), once per popup.
    fn blur_content(&self, width: i32, height: i32, now: Instant) -> Option<Rc<Image>> {
        let mut surface = super::flow_surface(&self.gfx).ok()?;
        surface.draw(width, height, self.scale, |canvas| {
            self.draw_background(canvas, now);
            self.draw_content(canvas, now);
        });
        let (w, h, pixels) = surface.bgra()?;
        let (mut small, sw, sh) = blur::half(&pixels, w as usize, h as usize);
        let sigma = blur::sigma_for_radius(BLUR_RADIUS) * self.scale / 2.0;
        blur::gaussian(&mut small, sw, sh, sigma);
        Some(Image::new(sw as u32, sh as u32, small))
    }

    fn dismiss_level(&self, now: Instant) -> f32 {
        match (&self.popup, self.dismiss_out) {
            (Some(p), _) => anim::progress(p.opened, 0.0, FADE_MS, now),
            (None, Some(start)) => 1.0 - anim::progress(start, 0.0, FADE_MS, now),
            _ => 0.0,
        }
    }

    /// How opaque the inactive background is: faded out over 150 ms when
    /// the window becomes active, in when it stops being.
    fn inactive_level(&self, now: Instant) -> f32 {
        let t = self
            .active_changed
            .map_or(1.0, |at| anim::progress(at, 0.0, FADE_MS, now));
        if self.active { 1.0 - t } else { t }
    }

    fn full(&self) -> Rect {
        Rect::new(0.0, 0.0, self.size.0, self.size.1)
    }

    fn draw(&self, canvas: &mut Canvas, now: Instant) {
        let full = self.full();
        self.draw_background(canvas, now);
        let dismiss = self.dismiss_level(now);
        match &self.backdrop {
            Some(_) if dismiss >= 1.0 => {}
            Some(_) if dismiss > 0.0 => {
                canvas.push_layer(1.0 - dismiss, None);
                self.draw_content(canvas, now);
                canvas.pop();
            }
            _ => self.draw_content(canvas, now),
        }
        if dismiss > 0.0 {
            // Background, with the blurred window at 0.8 over all but the
            // bottom 14 DIP.
            canvas.rect(full, self.colors.background.alpha(dismiss));
            if let Some((image, _)) = &self.backdrop {
                canvas.push_layer(
                    1.0,
                    Some((Rect::new(0.0, 0.0, full.w, (full.h - 14.0).max(0.0)), 0.0)),
                );
                canvas.image(image, full, 0.8 * dismiss);
                canvas.pop();
            }
        }
        self.draw_popup(canvas, now);
    }

    /// The acrylic layers and the border.
    fn draw_background(&self, canvas: &mut Canvas, now: Instant) {
        let full = self.full();
        canvas.rect(full, self.colors.base);
        let inactive = self.inactive_level(now);
        if self.theme.transparency && inactive > 0.0 {
            canvas.rect(full, self.colors.inactive.alpha(inactive));
        }
        let border = 1.0 - inactive;
        if border > 0.0 {
            canvas.stroke_rect(
                full.inset(BORDER / 2.0, BORDER / 2.0),
                BORDER,
                self.colors.border.alpha(border),
            );
        }
    }

    /// The title row and the columns.
    fn draw_content(&self, canvas: &mut Canvas, now: Instant) {
        let theme = &self.theme;
        canvas.text(
            tr("FullWindowTitleText"),
            Rect::new(
                BORDER + 12.0,
                BORDER,
                (self.size.0 - 2.0 * BORDER - CLOSE_W - 24.0).max(0.0),
                BAR_H,
            ),
            &TextStyle::new(UI_FONT, 12.0).ellipsis(),
            theme.text,
        );
        self.draw_close(canvas);
        let view = self.viewport();
        canvas.push_layer(1.0, Some((view, 0.0)));
        if self.shown().is_empty() {
            let style = TextStyle::new(UI_FONT, 13.0)
                .align(Align::Center)
                .wrap()
                .top();
            canvas.text(
                tr("NoDevicesPanelContent"),
                Rect::new(view.x + 24.0, view.y + 24.0, COLUMN - 48.0, view.h),
                &style,
                theme.text,
            );
        } else {
            self.draw_columns(canvas, now);
        }
        canvas.pop();
    }

    fn draw_close(&self, canvas: &mut Canvas) {
        let rect = self.close_rect();
        let hovered = self.hover == Some(Hit::Close);
        let pressed = hovered && self.pressed == Some(Hit::Close);
        let text = if pressed {
            canvas.rect(rect, Rgba::rgb(0xf1, 0x70, 0x7a));
            self.colors.close_pressed_text
        } else if hovered {
            canvas.rect(rect, Rgba::rgb(0xe8, 0x11, 0x23));
            Rgba::WHITE
        } else {
            self.theme.text
        };
        let style = TextStyle::new(self.theme.icon_font, 10.0).align(Align::Center);
        canvas.text(&glyph(GLYPH_CLOSE), rect, &style, text);
    }

    fn painter(&self) -> RowPaint<'_> {
        fn mute(hit: &Option<Hit>) -> Option<&SliderKey> {
            match hit {
                Some(Hit::Mute(key)) => Some(key),
                _ => None,
            }
        }
        RowPaint {
            view: &self.view,
            sliders: &self.sliders,
            icons: &self.icons,
            scale: self.scale,
            hover: mute(&self.hover),
            pressed: mute(&self.pressed),
        }
    }

    fn draw_columns(&self, canvas: &mut Canvas, now: Instant) {
        let theme = &self.theme;
        let layout = self.layout();
        let painter = self.painter();
        let title = TextStyle::new(UI_FONT, 15.0).ellipsis();
        let strip = if self.many_devices() { STRIP_H } else { 0.0 };
        for column in &layout.columns {
            let r = column.rect;
            canvas.text(
                &column.name,
                Rect::new(r.x + 12.0, r.y, COLUMN - 24.0, TITLE_H),
                &title,
                theme.text,
            );
            let apps_top = r.y + TITLE_H + DEVICE_ROW_H;
            canvas.rect(
                Rect::new(
                    r.x,
                    apps_top,
                    COLUMN,
                    (r.bottom() - strip - apps_top).max(0.0),
                ),
                self.colors.background,
            );
        }
        if strip > 0.0 {
            if let (Some(first), Some(last)) = (layout.columns.first(), layout.columns.last()) {
                canvas.rect(
                    Rect::new(
                        first.rect.x,
                        first.rect.bottom() - strip,
                        last.rect.right() - first.rect.x,
                        strip,
                    ),
                    self.colors.background,
                );
            }
        }
        for row in &layout.rows {
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
                painter.row(canvas, theme, row.rect, &row.key);
                canvas.pop_transform();
                canvas.pop();
            } else {
                painter.row(canvas, theme, row.rect, &row.key);
            }
        }
        if self.popup.is_none() {
            self.draw_focus(canvas, &layout.rows);
        }
        self.draw_scrollbars(canvas);
    }

    fn draw_focus(&self, canvas: &mut Canvas, rows: &[Row]) {
        if !self.keyboard {
            return;
        }
        if let Some(row) = self.focus.and_then(|f| rows.get(f)) {
            rows::draw_focus_ring(canvas, &self.theme, row.rect);
        }
    }

    fn draw_scrollbars(&self, canvas: &mut Canvas) {
        let view = self.viewport();
        let (content_w, content_h) = self.content();
        let (mx, my) = self.max_scroll();
        let color = self.theme.text.alpha(0.4);
        let dragging = |horizontal: bool| self.scroll_drag.is_some_and(|(h, ..)| h == horizontal);
        if my > 0.0 {
            let thumb = (view.h * view.h / content_h).max(24.0);
            let y = view.y + self.scroll.1 / my * (view.h - thumb);
            let wide = self.hover == Some(Hit::VScroll) || dragging(false);
            let w = if wide { 8.0 } else { 2.0 };
            let x = view.right() - 8.0 - w / 2.0;
            canvas.round_rect(Rect::new(x, y + 4.0, w, thumb - 8.0), w / 2.0, color);
        }
        if mx > 0.0 {
            let thumb = (view.w * view.w / content_w).max(24.0);
            let x = view.x + self.scroll.0 / mx * (view.w - thumb);
            let wide = self.hover == Some(Hit::HScroll) || dragging(true);
            let h = if wide { 8.0 } else { 2.0 };
            let y = view.bottom() - 6.0 - h / 2.0;
            canvas.round_rect(Rect::new(x + 4.0, y, thumb - 8.0, h), h / 2.0, color);
        }
    }

    fn draw_popup(&self, canvas: &mut Canvas, now: Instant) {
        let Some(popup) = &self.popup else {
            return;
        };
        let Some((frame, rows)) = self.popup_layout() else {
            return;
        };
        let theme = &self.theme;
        canvas.push_layer(anim::progress(popup.opened, 0.0, FADE_MS, now), None);
        let tool = |hit: &Option<Hit>| match hit {
            Some(Hit::Tool(i)) => Some(*i),
            _ => None,
        };
        let hover = tool(&self.hover);
        let pressed = tool(&self.pressed).filter(|p| hover == Some(*p));
        rows::draw_popup_frame(
            canvas,
            theme,
            frame,
            &popup.title,
            &popup.tools,
            hover,
            pressed,
        );
        let painter = self.painter();
        for row in &rows {
            painter.row(canvas, theme, row.rect, &row.key);
        }
        self.draw_focus(canvas, &rows);
        canvas.pop();
    }
}
