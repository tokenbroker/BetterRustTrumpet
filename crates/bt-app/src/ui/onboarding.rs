//! The first-run setup window (OnboardingWindow and OnboardingViewModel):
//! a borderless 760x620 window centred on the screen with a taskbar
//! button, in the original's fixed dark palette, with five pages: audio
//! output, appearance, privacy and startup, a summary and how to pin the
//! tray icon (with the original's animation). Skip, Back and
//! Continue/Done sit at the bottom with five page dots.
//!
//! What each page writes, through the Store, as the original:
//! - Continue on the audio page makes the chosen output the default.
//! - Continue on the appearance page writes the theme: System turns custom
//!   colours off and unsets the colours, Custom applies the
//!   "BetterTrumpet Focus" palette.
//! - Continue on the privacy page and Skip write IsTelemetryEnabled,
//!   RunAtStartup and UpdateNotifyChannel (All). This build has no
//!   telemetry or update service, so the consent key is only stored.
//!
//! Done and Skip (and Esc) close the window; Alt+F4 closes it without
//! writing anything else. The window is destroyed with its pixels.
use super::{
    anim::{self, EASE_OUT},
    gif,
};
use crate::core::Core;
use bt_app::i18n::{tr, trf};
use bt_app::win32::{
    gfx::{Align, Canvas, Gfx, Image, Rect, Rgba, Surface, TextStyle},
    startup,
    wnd::{self, Button, Event, Modifiers, Window},
};
use bt_core::{
    colors::{Color, NumberCulture},
    settings::{Key, Value},
};
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

/// The pin animation (Assets/TrayPin.gif).
pub const TRAY_PIN: &[u8] = include_bytes!("../../assets/TrayPin.gif");

pub const PAGES: usize = 5;
const SIZE: (f32, f32) = (760.0, 620.0);
const FONT: &str = "Segoe UI";
const SIDE: f32 = 31.0;
const PAGE_MS: f32 = 260.0;
const DOT_MS: f32 = 180.0;
const AMBIENT_MS: f32 = 5000.0;

// Timers.
const FRAME: usize = 1;
const AMBIENT: usize = 2;
const GIF: usize = 3;

// Virtual keys.
const VK_TAB: u16 = 0x09;
const VK_RETURN: u16 = 0x0d;
const VK_ESCAPE: u16 = 0x1b;
const VK_SPACE: u16 = 0x20;

// OnboardingComponents.xaml.
const BG: Rgba = Rgba::rgb(0x14, 0x14, 0x18);
const SURFACE: Rgba = Rgba::rgb(0x1b, 0x1b, 0x21);
const ELEVATED: Rgba = Rgba::rgb(0x20, 0x20, 0x28);
const BORDER: Rgba = Rgba::rgb(0x2b, 0x2b, 0x33);
const BORDER_SOFT: Rgba = Rgba::rgb(0x24, 0x24, 0x2b);
const ACCENT: Rgba = Rgba::rgb(0x3b, 0x9e, 0xff);
const ACCENT_HOVER: Rgba = Rgba::rgb(0x57, 0xac, 0xff);
const TEXT: Rgba = Rgba::rgb(0xf3, 0xf5, 0xf8);
const MUTED: Rgba = Rgba::rgb(0xa8, 0xaa, 0xb3);
const SUBTLE: Rgba = Rgba::rgb(0x7c, 0x7f, 0x88);
const TRACK: Rgba = Rgba::rgb(0x2a, 0x2d, 0x36);
const DANGER: Rgba = Rgba::rgb(0xfc, 0xa5, 0xa5);
const DOT_OFF: Rgba = Rgba::rgb(52, 54, 64);
const GHOST_HOVER: Rgba = Rgba::rgba(255, 255, 255, 15.0 / 255.0);

/// The "BetterTrumpet Focus" colours ApplyTheme writes for Custom, in
/// CHANNEL_KEYS order.
const FOCUS: [(&str, (u8, u8, u8)); 7] = [
    ("SliderThumbColor", (88, 166, 255)),
    ("SliderTrackFillColor", (59, 158, 255)),
    ("SliderTrackBackgroundColor", (48, 55, 68)),
    ("PeakMeterColor", (78, 213, 168)),
    ("WindowBackgroundColor", (18, 18, 22)),
    ("TextColor", (238, 241, 246)),
    ("AccentGlowColor", (59, 158, 255)),
];
/// `Colors.Transparent.ToString()`.
const TRANSPARENT: &str = "#00FFFFFF";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Skip,
    Back,
    Next,
    Device(usize),
    Theme(usize),
    Telemetry,
    DisableAnyway,
    KeepEnabled,
    Startup,
}

/// What the rest of the app has to pick up after an event.
#[derive(Default)]
pub struct Outcome {
    pub close: bool,
    /// Settings were written (theme, consent, startup).
    pub settings: bool,
    /// The default output changed.
    pub device: bool,
}

struct Device {
    id: String,
    name: String,
}

/// SineEase EaseInOut.
fn sine_in_out(t: f32) -> f32 {
    (1.0 - (t * std::f32::consts::PI).cos()) / 2.0
}

fn style(size: f32, weight: u16) -> TextStyle {
    TextStyle::new(FONT, size).weight(weight).top()
}

fn color_text((r, g, b): (u8, u8, u8)) -> String {
    let argb = 0xff00_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
    Color::from_argb(argb).to_text(&NumberCulture::INVARIANT)
}

pub struct Onboarding {
    pub window: Window,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    scale: f32,
    position: (i32, i32),
    page: usize,
    forward: bool,
    page_start: Instant,
    progress: (f32, f32, Instant),
    dots: [(f32, f32); PAGES],
    dots_start: Instant,
    opened: Instant,
    devices: Vec<Device>,
    selected: Option<usize>,
    scroll: f32,
    theme: usize,
    telemetry: bool,
    confirm: bool,
    run_at_startup: bool,
    hover: Option<Target>,
    pressed: Option<Target>,
    focus: Option<Target>,
    gif: Option<gif::Animation>,
    frame: Option<Rc<Image>>,
}

impl Onboarding {
    /// OnboardingViewModel's constructor: reads the current settings and
    /// the active outputs (sorted by name, the default one chosen).
    pub fn new(gfx: Rc<Gfx>, core: &mut Core) -> windows::core::Result<Self> {
        let window = wnd::create_layered_app(tr("OnboardingWindowTitle"))?;
        let mut devices: Vec<Device> = core
            .audio
            .endpoints(false)
            .unwrap_or_default()
            .into_iter()
            .map(|e| Device {
                id: e.id,
                name: e.name,
            })
            .collect();
        devices.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
        let default = core
            .audio
            .default_endpoint(bt_audio::Role::Playback)
            .ok()
            .flatten()
            .map(|e| e.id);
        let selected = devices.iter().position(|d| Some(&d.id) == default.as_ref());
        let now = Instant::now();
        let mut onboarding = Self {
            window,
            surface: Some(Surface::new(gfx.clone())?),
            gfx,
            scale: 1.0,
            position: (0, 0),
            page: 0,
            forward: true,
            page_start: now,
            // The bar's XAML scale is 0.1667 before the first update.
            progress: (0.1667, 0.2, now),
            dots: [(0.65, 0.65); PAGES],
            dots_start: now,
            opened: now,
            devices,
            selected,
            scroll: 0.0,
            theme: usize::from(core.store.bool(Key::UseCustomSliderColors)),
            telemetry: core.store.bool(Key::IsTelemetryEnabled),
            confirm: false,
            run_at_startup: startup::is_enabled(),
            hover: None,
            pressed: None,
            focus: None,
            gif: None,
            frame: None,
        };
        onboarding.update_dots();
        onboarding.place();
        Ok(onboarding)
    }

    /// Centres the window on the monitor under the pointer
    /// (WindowStartupLocation="CenterScreen").
    fn place(&mut self) {
        let (cx, cy) = wnd::cursor_pos();
        let monitor = wnd::monitor_at(cx, cy);
        self.scale = monitor.scale;
        let (wx, wy, ww, wh) = monitor.work;
        let w = (SIZE.0 * self.scale).round() as i32;
        let h = (SIZE.1 * self.scale).round() as i32;
        self.position = (wx + (ww - w) / 2, wy + (wh - h) / 2);
    }

    /// Shows the window (or brings it back to the front).
    pub fn present(&mut self) {
        let first = !self.window.is_visible();
        self.paint();
        self.window.present_app();
        if first {
            self.window.set_timer(FRAME, 15);
            self.window.set_timer(AMBIENT, 100);
        }
    }

    /// Destroys the window and its surface.
    pub fn close(mut self) {
        self.surface = None;
        self.window.destroy();
    }

    // ---------- state (OnboardingViewModel) ----------

    fn go_to(&mut self, page: usize) {
        if page == self.page {
            return;
        }
        let now = Instant::now();
        self.forward = page > self.page;
        self.progress = (
            self.progress_now(now),
            (page + 1) as f32 / PAGES as f32,
            now,
        );
        self.page = page;
        self.page_start = now;
        self.hover = None;
        self.update_dots();
        self.window.set_timer(FRAME, 15);
        if page == PAGES - 1 {
            self.start_gif();
        } else {
            self.window.kill_timer(GIF);
        }
    }

    fn update_dots(&mut self) {
        let now = Instant::now();
        let t = EASE_OUT.at(anim::progress(self.dots_start, 0.0, DOT_MS, now));
        for (i, dot) in self.dots.iter_mut().enumerate() {
            let current = anim::lerp(dot.0, dot.1, t);
            *dot = (current, if i == self.page { 1.0 } else { 0.65 });
        }
        self.dots_start = now;
    }

    fn progress_now(&self, now: Instant) -> f32 {
        let (from, to, start) = self.progress;
        anim::lerp(
            from,
            to,
            EASE_OUT.at(anim::progress(start, 0.0, PAGE_MS, now)),
        )
    }

    fn start_gif(&mut self) {
        if self.gif.is_none() {
            self.gif = gif::Animation::new(TRAY_PIN);
        }
        self.next_gif_frame();
    }

    fn next_gif_frame(&mut self) {
        if let Some(animation) = &mut self.gif {
            let (image, delay) = animation.step();
            self.frame = Some(image);
            self.window.set_timer(GIF, delay);
        }
    }

    fn next(&mut self, core: &mut Core, out: &mut Outcome) {
        match self.page {
            0 => self.apply_device(core, out),
            1 => self.apply_theme(core, out),
            2 => self.apply_privacy(core, out),
            _ => {}
        }
        if self.page < PAGES - 1 {
            self.go_to(self.page + 1);
        } else {
            out.close = true;
        }
    }

    fn skip(&mut self, core: &mut Core, out: &mut Outcome) {
        self.apply_privacy(core, out);
        out.close = true;
    }

    /// The telemetry check box: turning it off asks first.
    fn toggle_telemetry(&mut self) {
        if self.telemetry {
            self.confirm = true;
        } else {
            self.telemetry = true;
            self.confirm = false;
        }
    }

    fn apply_device(&mut self, core: &mut Core, out: &mut Outcome) {
        let Some(device) = self.selected.and_then(|i| self.devices.get(i)) else {
            return;
        };
        let current = core
            .audio
            .default_endpoint(bt_audio::Role::Playback)
            .ok()
            .flatten();
        if current.is_none_or(|c| c.id != device.id)
            && core.audio.set_default_device(&device.id).is_ok()
        {
            out.device = true;
        }
    }

    fn apply_theme(&mut self, core: &mut Core, out: &mut Outcome) {
        let store = &mut core.store;
        let custom = self.theme == 1;
        store.set(Key::UseCustomSliderColors, Value::Bool(custom));
        store.set(Key::UseDynamicAlbumArtTheme, Value::Bool(false));
        let name = if custom { "BetterTrumpet Focus" } else { "" };
        store.set(Key::ActiveThemeName, Value::Text(Some(name.into())));
        for (key, rgb) in FOCUS {
            if custom {
                store.set_text(key, &color_text(rgb));
            } else {
                store.set_text(key, TRANSPARENT);
            }
        }
        out.settings = true;
    }

    fn apply_privacy(&mut self, core: &mut Core, out: &mut Outcome) {
        core.store
            .set(Key::IsTelemetryEnabled, Value::Bool(self.telemetry));
        if let Ok(exe) = std::env::current_exe() {
            let _ = startup::set_enabled(self.run_at_startup, &exe);
        }
        core.store.set(Key::UpdateNotifyChannel, Value::Int(0));
        out.settings = true;
    }

    fn activate(&mut self, core: &mut Core, target: Target, out: &mut Outcome) {
        match target {
            Target::Skip => self.skip(core, out),
            Target::Back => {
                if self.page > 0 {
                    self.go_to(self.page - 1);
                }
            }
            Target::Next => self.next(core, out),
            Target::Device(i) => self.selected = Some(i),
            Target::Theme(i) => self.theme = i,
            Target::Telemetry => self.toggle_telemetry(),
            Target::DisableAnyway => {
                self.confirm = false;
                self.telemetry = false;
            }
            Target::KeepEnabled => {
                self.confirm = false;
                self.telemetry = true;
            }
            Target::Startup => self.run_at_startup = !self.run_at_startup,
        }
    }

    // ---------- events ----------

    pub fn event(&mut self, core: &mut Core, event: Event) -> Outcome {
        let mut out = Outcome::default();
        match event {
            Event::Close => {
                out.close = true;
                return out;
            }
            Event::DpiChanged => self.scale = self.window.scale(),
            Event::Timer(FRAME) => {
                let now = Instant::now();
                if !anim::running(Some(self.page_start), PAGE_MS, now)
                    && !anim::running(Some(self.dots_start), DOT_MS, now)
                {
                    self.window.kill_timer(FRAME);
                }
            }
            Event::Timer(AMBIENT) => {}
            Event::Timer(GIF) => {
                if self.page == PAGES - 1 {
                    self.next_gif_frame();
                } else {
                    self.window.kill_timer(GIF);
                }
            }
            Event::MouseMove(x, y) => {
                let hover = self.hit(x, y);
                if hover == self.hover {
                    return out;
                }
                self.hover = hover;
            }
            Event::MouseLeave => self.hover = None,
            Event::MouseDown(Button::Left, x, y, _) => {
                self.hover = self.hit(x, y);
                self.pressed = self.hover;
                // Device cards react to the press (MouseLeftButtonDown).
                if let Some(Target::Device(i)) = self.pressed {
                    self.selected = Some(i);
                    self.pressed = None;
                } else if self.pressed.is_some() {
                    self.window.capture();
                }
            }
            Event::MouseUp(Button::Left, x, y, _) => {
                let target = self.pressed.take();
                self.window.release_capture();
                self.hover = self.hit(x, y);
                if let Some(target) = target.filter(|t| Some(*t) == self.hover) {
                    self.activate(core, target, &mut out);
                }
            }
            Event::CaptureLost => self.pressed = None,
            Event::Wheel(delta, _, _) if self.page == 0 => {
                let max = self.max_scroll();
                self.scroll = (self.scroll - f32::from(delta) / 120.0 * 48.0).clamp(0.0, max);
            }
            Event::Key(vk, m) => self.key(core, vk, m, &mut out),
            _ => return out,
        }
        if !out.close {
            self.paint();
        }
        out
    }

    fn key(&mut self, core: &mut Core, vk: u16, m: Modifiers, out: &mut Outcome) {
        match vk {
            VK_ESCAPE => self.skip(core, out),
            VK_TAB => {
                let list = self.focusables();
                if list.is_empty() {
                    return;
                }
                let at = self.focus.and_then(|f| list.iter().position(|t| *t == f));
                let next = match (at, m.shift) {
                    (None, false) => 0,
                    (None, true) => list.len() - 1,
                    (Some(i), false) => (i + 1) % list.len(),
                    (Some(i), true) => (i + list.len() - 1) % list.len(),
                };
                self.focus = Some(list[next]);
            }
            VK_RETURN | VK_SPACE => {
                if let Some(target) = self.focus.filter(|f| self.focusables().contains(f)) {
                    self.activate(core, target, out);
                }
            }
            _ => {}
        }
    }

    /// Keyboard stops in tab order (device cards are not focusable).
    fn focusables(&self) -> Vec<Target> {
        let mut list = match self.page {
            1 => vec![Target::Theme(0), Target::Theme(1)],
            2 if self.confirm => vec![
                Target::Telemetry,
                Target::DisableAnyway,
                Target::KeepEnabled,
                Target::Startup,
            ],
            2 => vec![Target::Telemetry, Target::Startup],
            _ => vec![],
        };
        list.push(Target::Skip);
        if self.page > 0 {
            list.push(Target::Back);
        }
        list.push(Target::Next);
        list
    }

    // ---------- layout ----------

    fn line(&self, size: f32, weight: u16) -> f32 {
        self.gfx.measure("Ag", &style(size, weight)).1.ceil()
    }

    fn wrapped(&self, text: &str, size: f32, weight: u16, width: f32) -> f32 {
        self.gfx
            .measure_wrapped(text, &style(size, weight), width)
            .ceil()
    }

    fn text_width(&self, text: &str, size: f32, weight: u16) -> f32 {
        self.gfx.measure(text, &style(size, weight)).0.ceil()
    }

    fn button_height(&self) -> f32 {
        20.0 + self.line(14.0, 600)
    }

    fn content(&self) -> Rect {
        let header = 25.0 + self.line(22.0, 600) + 4.0 + self.line(12.0, 600) + 18.0 + 3.0 + 18.0;
        let footer = SIZE.1 - 1.0 - 24.0 - self.button_height() - 18.0;
        Rect::new(SIDE, header, SIZE.0 - 2.0 * SIDE, footer - header)
    }

    fn footer(&self) -> Vec<(Target, Rect)> {
        let h = self.button_height();
        let y = SIZE.1 - 1.0 - 24.0 - h;
        let ghost = |text: &str| (self.text_width(text, 14.0, 600) + 36.0).max(102.0);
        let skip = Rect::new(SIDE, y, ghost(tr("OnboardingSkip")), h);
        let next_w = (self.text_width(self.next_text(), 14.0, 600) + 44.0).max(126.0);
        let next = Rect::new(SIZE.0 - SIDE - next_w, y, next_w, h);
        let mut list = vec![(Target::Skip, skip)];
        if self.page > 0 {
            let w = ghost(tr("OnboardingBack"));
            list.push((Target::Back, Rect::new(next.x - 10.0 - w, y, w, h)));
        }
        list.push((Target::Next, next));
        list
    }

    fn next_text(&self) -> &'static str {
        if self.page == PAGES - 1 {
            tr("OnboardingDone")
        } else {
            tr("OnboardingContinue")
        }
    }

    /// The device list's viewport and the top of its first card.
    fn device_view(&self) -> Rect {
        let c = self.content();
        let top = c.y
            + self.line(28.0, 600)
            + 8.0
            + self.wrapped(tr("OnboardingSubtitleAudio"), 14.0, 400, 520.0)
            + 20.0;
        Rect::new(c.x, top, c.w, (c.bottom() - top).clamp(0.0, 350.0))
    }

    fn max_scroll(&self) -> f32 {
        let view = self.device_view();
        let total = self.devices.len() as f32 * 84.0 - 10.0;
        (total - view.h).max(0.0)
    }

    /// Title and subtitle at the top of pages 1-3, then the y below them.
    fn heading(&self, width: f32, subtitle: &str, gap: f32) -> f32 {
        let c = self.content();
        c.y + self.line(28.0, 600) + 8.0 + self.wrapped(subtitle, 14.0, 400, width) + gap
    }

    fn theme_cards(&self) -> [Rect; 2] {
        let c = self.content();
        let top = self.heading(c.w, tr("OnboardingSubtitleAppearance"), 22.0);
        let w = (c.w - 24.0) / 2.0;
        [
            Rect::new(c.x, top, w, c.bottom() - top),
            Rect::new(c.x + w + 24.0, top, w, c.bottom() - top),
        ]
    }

    /// The privacy page: the card, its check box, the confirmation box and
    /// buttons, the startup check box and its description's top.
    fn privacy(&self) -> Privacy {
        let c = self.content();
        let x = c.x + (c.w - 560.0) / 2.0;
        let top = self.heading(c.w, tr("OnboardingSubtitlePrivacy"), 20.0);
        let inner = Rect::new(x + 19.0, top + 19.0, 560.0 - 38.0, 0.0);
        let text_w = inner.w - 28.0 - 16.0;
        let mut y = inner.y
            + self.line(16.0, 600)
            + 6.0
            + self.wrapped(tr("OnboardingTelemetryValueDesc"), 12.0, 400, text_w);
        let check = Rect::new(inner.right() - 28.0, inner.y, 18.0, 18.0);
        let mut confirm = None;
        if self.confirm {
            let box_top = y + 14.0;
            let box_inner_w = inner.w - 30.0;
            let mut by = box_top
                + 15.0
                + self.line(13.0, 600)
                + 6.0
                + self.wrapped(tr("OnboardingTelemetryConfirmDesc"), 12.0, 400, box_inner_w)
                + 12.0;
            let h = self.button_height();
            let keep_w = (self.text_width(tr("OnboardingTelemetryKeepEnabled"), 14.0, 600) + 44.0)
                .max(126.0);
            let danger_w = (self.text_width(tr("OnboardingTelemetryDisableAnyway"), 14.0, 600)
                + 36.0)
                .max(126.0);
            let right = inner.right() - 15.0;
            let keep = Rect::new(right - keep_w, by, keep_w, h);
            let danger = Rect::new(keep.x - 8.0 - danger_w, by, danger_w, h);
            by += h + 15.0;
            confirm = Some((
                Rect::new(inner.x, box_top, inner.w, by - box_top),
                danger,
                keep,
            ));
            y = by;
        }
        let card = Rect::new(x, top, 560.0, y + 19.0 - top);
        let row_y = card.bottom() + 10.0 + 6.0;
        let label_w = self.text_width(tr("OnboardingLaunchAtStartup"), 12.0, 400);
        let row_h = self.line(12.0, 400).max(18.0);
        let startup = Rect::new(x + 4.0, row_y, 28.0 + label_w, row_h);
        Privacy {
            card,
            inner,
            text_w,
            check,
            confirm,
            startup,
            startup_desc: row_y + row_h + 6.0,
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<Target> {
        if let Some((target, _)) = self.footer().into_iter().find(|(_, r)| r.contains(x, y)) {
            return Some(target);
        }
        let dx = self.page_offset(Instant::now());
        let (x, y) = (x - dx, y);
        match self.page {
            0 => {
                let view = self.device_view();
                if !view.contains(x, y) {
                    return None;
                }
                (0..self.devices.len())
                    .find(|i| self.device_card(*i).contains(x, y))
                    .map(Target::Device)
            }
            1 => self
                .theme_cards()
                .iter()
                .position(|r| r.contains(x, y))
                .map(Target::Theme),
            2 => {
                let p = self.privacy();
                if let Some((_, danger, keep)) = p.confirm {
                    if danger.contains(x, y) {
                        return Some(Target::DisableAnyway);
                    }
                    if keep.contains(x, y) {
                        return Some(Target::KeepEnabled);
                    }
                }
                if p.check.contains(x, y) {
                    return Some(Target::Telemetry);
                }
                p.startup.contains(x, y).then_some(Target::Startup)
            }
            _ => None,
        }
    }

    fn device_card(&self, i: usize) -> Rect {
        let view = self.device_view();
        Rect::new(view.x, view.y + i as f32 * 84.0 - self.scroll, view.w, 74.0)
    }

    /// The sliding page's x offset (TranslateTransform from ±28 DIP).
    fn page_offset(&self, now: Instant) -> f32 {
        let t = EASE_OUT.at(anim::progress(self.page_start, 0.0, PAGE_MS, now));
        let from = if self.forward { 28.0 } else { -28.0 };
        from * (1.0 - t)
    }

    // ---------- drawing ----------

    pub fn paint(&mut self) {
        let now = Instant::now();
        self.draw_now(now);
        if let Some(surface) = &self.surface {
            surface.present(self.window.raw(), self.position.0, self.position.1, 1.0);
        }
    }

    fn draw_now(&mut self, now: Instant) {
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        let w = (SIZE.0 * self.scale).round() as i32;
        let h = (SIZE.1 * self.scale).round() as i32;
        surface.draw(w, h, self.scale, |canvas| self.draw(canvas, now));
        self.surface = Some(surface);
    }

    /// Draws `page` with its animations finished, without showing the
    /// window; `confirm` opens the privacy page's confirmation.
    pub fn render(&mut self, page: usize, confirm: bool) -> Option<(u32, u32, Vec<u8>)> {
        let past = Instant::now() - Duration::from_secs(10);
        self.page = page.min(PAGES - 1);
        self.page_start = past;
        self.progress = (0.0, (self.page + 1) as f32 / PAGES as f32, past);
        self.dots_start = past;
        self.dots = std::array::from_fn(|i| {
            let o = if i == self.page { 1.0 } else { 0.65 };
            (o, o)
        });
        self.confirm = confirm;
        // The confirmation only shows while the box is still ticked.
        self.telemetry |= confirm;
        self.opened = past;
        self.scale = wnd::monitor_at(0, 0).scale;
        if self.page == PAGES - 1 && self.frame.is_none() {
            self.gif = gif::Animation::new(TRAY_PIN);
            // A frame from the middle of the loop.
            for _ in 0..60 {
                self.next_gif_frame();
            }
            self.window.kill_timer(GIF);
        }
        self.draw_now(Instant::now());
        self.surface.as_ref()?.rgba()
    }

    fn draw(&self, canvas: &mut Canvas, now: Instant) {
        let frame = Rect::new(0.0, 0.0, SIZE.0, SIZE.1);
        canvas.round_rect(frame, 12.0, BORDER);
        let inside = frame.inset(1.0, 1.0);
        canvas.round_rect(inside, 11.0, BG);
        // The breathing glow in the top-left corner.
        let phase =
            (now.saturating_duration_since(self.opened).as_secs_f32() * 1000.0 / AMBIENT_MS) % 2.0;
        let t = sine_in_out(if phase > 1.0 { 2.0 - phase } else { phase });
        let radius = anim::lerp(0.58, 0.88, t);
        canvas.radial(
            inside,
            11.0,
            inside.x + inside.w * 0.18,
            inside.y + inside.h * 0.06,
            inside.w * radius,
            inside.h * radius,
            &[
                (
                    0.0,
                    Rgba::rgba(0x3b, 0x9e, 0xff, 0x1c as f32 / 255.0 * 0.42),
                ),
                (1.0, Rgba::rgba(0x3b, 0x9e, 0xff, 0.0)),
            ],
        );
        self.draw_header(canvas, now);
        let dx = self.page_offset(now);
        canvas.push_transform(dx, 0.0, 1.0, 0.0, 0.0);
        match self.page {
            0 => self.draw_audio(canvas),
            1 => self.draw_appearance(canvas),
            2 => self.draw_privacy(canvas),
            3 => self.draw_ready(canvas),
            _ => self.draw_pin(canvas),
        }
        canvas.pop_transform();
        self.draw_footer(canvas, now);
    }

    fn draw_header(&self, canvas: &mut Canvas, now: Instant) {
        let width = SIZE.0 - 2.0 * SIDE;
        let title_h = self.line(22.0, 600);
        canvas.text(
            "Better Rust Trumpet",
            Rect::new(SIDE, 25.0, width, title_h),
            &style(22.0, 600),
            TEXT,
        );
        let step_h = self.line(12.0, 600);
        canvas.text(
            &trf(
                "OnboardingStepFormat",
                &[
                    ("0", &(self.page + 1).to_string()),
                    ("1", &PAGES.to_string()),
                ],
            ),
            Rect::new(SIDE, 25.0 + title_h + 4.0, width, step_h),
            &style(12.0, 600),
            SUBTLE,
        );
        let row = Rect::new(SIDE, 25.0, width, title_h + 4.0 + step_h);
        let current = [
            "OnboardingAudioOutput",
            "OnboardingAppearance",
            "OnboardingPrivacy",
            "OnboardingAllReady",
            "OnboardingPinTitle",
        ][self.page];
        canvas.text(
            tr(current),
            row,
            &TextStyle::new(FONT, 13.0)
                .weight(600)
                .align(Align::Trailing),
            MUTED,
        );
        let track = Rect::new(SIDE, row.bottom() + 18.0, width, 3.0);
        canvas.rect(track, TRACK);
        let fill = Rect::new(track.x, track.y, track.w * self.progress_now(now), 3.0);
        if fill.w > 0.0 {
            canvas.round_rect(fill, 2.0, ACCENT);
            canvas.rect(
                Rect::new(fill.x, fill.y, (fill.w / 2.0).min(2.0), 3.0),
                ACCENT,
            );
        }
    }

    fn page_title(&self, canvas: &mut Canvas, x: f32, width: f32, key: &'static str) -> f32 {
        let c = self.content();
        let h = self.line(28.0, 600);
        canvas.text(
            tr(key),
            Rect::new(x, c.y, width, h),
            &style(28.0, 600),
            TEXT,
        );
        c.y + h + 8.0
    }

    fn subtitle(&self, canvas: &mut Canvas, x: f32, y: f32, width: f32, key: &'static str) -> f32 {
        let h = self.wrapped(tr(key), 14.0, 400, width);
        canvas.text(
            tr(key),
            Rect::new(x, y, width, h),
            &style(14.0, 400).wrap(),
            MUTED,
        );
        y + h
    }

    fn card(&self, canvas: &mut Canvas, rect: Rect, radius: f32, lit: bool) {
        canvas.round_rect(rect, radius, if lit { ACCENT } else { BORDER });
        canvas.round_rect(
            rect.inset(1.0, 1.0),
            radius - 1.0,
            if lit { ELEVATED } else { SURFACE },
        );
    }

    fn focus_ring(&self, canvas: &mut Canvas, target: Target, rect: Rect, radius: f32) {
        if self.focus == Some(target) {
            canvas.stroke_round_rect(rect.inset(-2.0, -2.0), radius + 2.0, 1.5, TEXT);
        }
    }

    fn draw_audio(&self, canvas: &mut Canvas) {
        let c = self.content();
        let x = c.x + (c.w - 520.0) / 2.0;
        let y = self.page_title(canvas, x, 520.0, "OnboardingAudioOutput");
        self.subtitle(canvas, x, y, 520.0, "OnboardingSubtitleAudio");
        let view = self.device_view();
        if self.devices.is_empty() {
            canvas.text(
                tr("OnboardingNoDevices"),
                Rect::new(view.x, view.y, view.w, self.line(13.0, 400)),
                &style(13.0, 400),
                MUTED,
            );
            return;
        }
        canvas.push_layer(1.0, Some((view, 0.0)));
        let name_h = self.line(14.0, 600);
        let sub_h = self.line(12.0, 400);
        let check_w = self.text_width("\u{2713}", 16.0, 600);
        for (i, device) in self.devices.iter().enumerate() {
            let card = self.device_card(i);
            if card.bottom() < view.y || card.y > view.bottom() {
                continue;
            }
            self.card(canvas, card, 8.0, self.hover == Some(Target::Device(i)));
            let icon = Rect::new(card.x + 19.0, card.y + 19.0, 36.0, 36.0);
            canvas.round_rect(icon, 7.0, ELEVATED);
            canvas.text(
                "\u{266a}",
                icon,
                &TextStyle::new(FONT, 18.0).weight(600).align(Align::Center),
                ACCENT,
            );
            let text_x = icon.right() + 14.0;
            let text_w = card.right() - 19.0 - check_w - text_x;
            let top = card.y + (card.h - name_h - 3.0 - sub_h) / 2.0;
            canvas.text(
                &device.name,
                Rect::new(text_x, top, text_w, name_h),
                &style(14.0, 600).ellipsis(),
                TEXT,
            );
            canvas.text(
                tr("OnboardingDeviceConfigured"),
                Rect::new(text_x, top + name_h + 3.0, text_w, sub_h),
                &style(12.0, 400),
                MUTED,
            );
            if self.selected == Some(i) {
                canvas.text(
                    "\u{2713}",
                    Rect::new(card.right() - 19.0 - check_w, card.y, check_w, card.h),
                    &TextStyle::new(FONT, 16.0).weight(600),
                    ACCENT,
                );
            }
        }
        canvas.pop();
    }

    fn draw_appearance(&self, canvas: &mut Canvas) {
        let c = self.content();
        let y = self.page_title(canvas, c.x, c.w, "OnboardingAppearance");
        self.subtitle(canvas, c.x, y, c.w, "OnboardingSubtitleAppearance");
        let cards = self.theme_cards();
        let texts = [
            (
                "OnboardingSystemTheme",
                "OnboardingSystemThemeDesc",
                "OnboardingRecommendedBadge",
            ),
            (
                "OnboardingCustomTheme",
                "OnboardingCustomThemeDesc",
                "OnboardingCustomThemeNote",
            ),
        ];
        for (i, (card, (title, desc, badge))) in cards.into_iter().zip(texts).enumerate() {
            let target = Target::Theme(i);
            self.card(canvas, card, 8.0, self.hover == Some(target));
            let inner = card.inset(19.0, 19.0);
            let title_h = self.line(18.0, 600);
            canvas.text(
                tr(title),
                Rect::new(inner.x, inner.y, inner.w, title_h),
                &style(18.0, 600),
                TEXT,
            );
            let desc_y = inner.y + title_h + 8.0;
            let desc_h = self.wrapped(tr(desc), 13.0, 400, inner.w);
            canvas.text(
                tr(desc),
                Rect::new(inner.x, desc_y, inner.w, desc_h),
                &style(13.0, 400).wrap(),
                MUTED,
            );
            if i == 1 {
                let bar = Rect::new(inner.x, desc_y + desc_h + 18.0, inner.w, 42.0);
                canvas.round_rect(bar, 8.0, BORDER_SOFT);
                canvas.round_rect(bar.inset(1.0, 1.0), 7.0, ELEVATED);
                let lane = bar.inset(12.0, 0.0);
                let unit = (lane.w - 16.0) / 4.0;
                let y = bar.y + 18.0;
                canvas.round_rect(Rect::new(lane.x, y, unit * 2.0, 6.0), 3.0, ACCENT);
                canvas.round_rect(
                    Rect::new(lane.x + unit * 2.0 + 8.0, y, unit, 6.0),
                    3.0,
                    Rgba::rgb(0x4e, 0xd5, 0xa8),
                );
                canvas.round_rect(
                    Rect::new(lane.x + unit * 3.0 + 16.0, y, unit, 6.0),
                    3.0,
                    Rgba::rgb(0x30, 0x37, 0x44),
                );
            }
            if self.theme == i {
                let h = self.line(13.0, 600);
                canvas.text(
                    tr(badge),
                    Rect::new(inner.x, inner.bottom() - h, inner.w, h),
                    &style(13.0, 600),
                    ACCENT,
                );
            }
        }
    }

    /// OnboardingCheckBox (no hover look of its own).
    fn checkbox(&self, canvas: &mut Canvas, rect: Rect, on: bool) {
        let (fill, stroke) = if on {
            (ACCENT, ACCENT)
        } else {
            (ELEVATED, BORDER)
        };
        canvas.round_rect(rect, 4.0, stroke);
        canvas.round_rect(rect.inset(1.0, 1.0), 3.0, fill);
        if on {
            canvas.text(
                "\u{2713}",
                rect,
                &TextStyle::new(FONT, 12.0).weight(600).align(Align::Center),
                Rgba::WHITE,
            );
        }
    }

    fn draw_privacy(&self, canvas: &mut Canvas) {
        let c = self.content();
        let y = self.page_title(canvas, c.x, c.w, "OnboardingPrivacy");
        self.subtitle(canvas, c.x, y, c.w, "OnboardingSubtitlePrivacy");
        let p = self.privacy();
        self.card(canvas, p.card, 8.0, false);
        let title_h = self.line(16.0, 600);
        canvas.text(
            tr("OnboardingTelemetryValueTitle"),
            Rect::new(p.inner.x, p.inner.y, p.text_w + 16.0, title_h),
            &style(16.0, 600),
            TEXT,
        );
        let desc = tr("OnboardingTelemetryValueDesc");
        canvas.text(
            desc,
            Rect::new(
                p.inner.x,
                p.inner.y + title_h + 6.0,
                p.text_w,
                self.wrapped(desc, 12.0, 400, p.text_w),
            ),
            &style(12.0, 400).wrap(),
            MUTED,
        );
        self.checkbox(canvas, p.check, self.telemetry);
        self.focus_ring(canvas, Target::Telemetry, p.check, 4.0);
        if let Some((frame, danger, keep)) = p.confirm {
            canvas.round_rect(frame, 8.0, BORDER_SOFT);
            canvas.round_rect(frame.inset(1.0, 1.0), 7.0, ELEVATED);
            let inner = frame.inset(15.0, 15.0);
            let h = self.line(13.0, 600);
            canvas.text(
                tr("OnboardingTelemetryConfirmTitle"),
                Rect::new(inner.x, inner.y, inner.w, h),
                &style(13.0, 600),
                TEXT,
            );
            let desc = tr("OnboardingTelemetryConfirmDesc");
            canvas.text(
                desc,
                Rect::new(
                    inner.x,
                    inner.y + h + 6.0,
                    inner.w,
                    self.wrapped(desc, 12.0, 400, inner.w),
                ),
                &style(12.0, 400).wrap(),
                MUTED,
            );
            self.ghost(
                canvas,
                Target::DisableAnyway,
                danger,
                tr("OnboardingTelemetryDisableAnyway"),
                DANGER,
            );
            self.primary(
                canvas,
                Target::KeepEnabled,
                keep,
                tr("OnboardingTelemetryKeepEnabled"),
            );
        }
        let s = p.startup;
        let box_rect = Rect::new(s.x, s.y + (s.h - 18.0) / 2.0, 18.0, 18.0);
        self.checkbox(canvas, box_rect, self.run_at_startup);
        canvas.text(
            tr("OnboardingLaunchAtStartup"),
            Rect::new(s.x + 28.0, s.y, s.w - 28.0 + 1.0, s.h),
            &TextStyle::new(FONT, 12.0),
            TEXT,
        );
        self.focus_ring(canvas, Target::Startup, s, 4.0);
        canvas.text(
            tr("OnboardingLaunchAtStartupDesc"),
            Rect::new(
                p.card.x + 32.0,
                p.startup_desc,
                p.card.w - 32.0,
                self.line(12.0, 400),
            ),
            &style(12.0, 400),
            MUTED,
        );
    }

    fn draw_ready(&self, canvas: &mut Canvas) {
        let c = self.content();
        let width = 560.0;
        let x = c.x + (c.w - width) / 2.0;
        let title_h = self.line(32.0, 600);
        let desc = tr("OnboardingSetupReadyDesc");
        let desc_h = self.wrapped(desc, 15.0, 400, width);
        let column = width / 3.0;
        let labels = [
            "OnboardingDeviceConfigured",
            "OnboardingAppearanceChosen",
            "OnboardingPrivacyConfigured",
        ];
        let cards: Vec<Rect> = (0..3)
            .map(|i| {
                let left = if i == 0 { 0.0 } else { 8.0 };
                let right = if i == 2 { 0.0 } else { 8.0 };
                Rect::new(
                    x + column * i as f32 + left,
                    0.0,
                    column - left - right,
                    0.0,
                )
            })
            .collect();
        let check_h = self.line(18.0, 600);
        let card_h = labels
            .iter()
            .zip(&cards)
            .map(|(label, r)| 34.0 + check_h + 8.0 + self.wrapped(tr(label), 13.0, 600, r.w - 34.0))
            .fold(0.0, f32::max);
        let tray = tr("OnboardingTrayAccessDesc");
        let tray_h = self.wrapped(tray, 12.0, 400, width);
        let total = title_h + 10.0 + desc_h + 22.0 + card_h + 20.0 + tray_h;
        let mut y = c.y + ((c.h - total) / 2.0).max(0.0);
        canvas.text(
            tr("OnboardingAllReady"),
            Rect::new(x, y, width, title_h),
            &style(32.0, 600),
            TEXT,
        );
        y += title_h + 10.0;
        canvas.text(
            desc,
            Rect::new(x, y, width, desc_h),
            &style(15.0, 400).wrap(),
            MUTED,
        );
        y += desc_h + 22.0;
        for (label, r) in labels.iter().zip(cards) {
            let card = Rect::new(r.x, y, r.w, card_h);
            self.card(canvas, card, 8.0, false);
            let inner = card.inset(17.0, 17.0);
            canvas.text(
                "\u{2713}",
                Rect::new(inner.x, inner.y, inner.w, check_h),
                &style(18.0, 600),
                ACCENT,
            );
            canvas.text(
                tr(label),
                Rect::new(inner.x, inner.y + check_h + 8.0, inner.w, inner.h),
                &style(13.0, 600).wrap(),
                TEXT,
            );
        }
        y += card_h + 20.0;
        canvas.text(
            tray,
            Rect::new(x, y, width, tray_h),
            &style(12.0, 400).wrap(),
            SUBTLE,
        );
    }

    fn draw_pin(&self, canvas: &mut Canvas) {
        let c = self.content();
        let frame = Rect::new(c.x, c.y + (c.h - 364.0) / 2.0, 330.0, 364.0);
        canvas.round_rect(frame, 10.0, BORDER_SOFT);
        canvas.round_rect(frame.inset(1.0, 1.0), 9.0, SURFACE);
        let slot = frame.inset(17.0, 17.0);
        if let Some(image) = &self.frame {
            // Stretch="Uniform" inside the 330 DIP high image box.
            let (iw, ih) = (image.width as f32, image.height as f32);
            let fit = (slot.w / iw).min(slot.h / ih);
            let (w, h) = (iw * fit, ih * fit);
            canvas.image(
                image,
                Rect::new(
                    slot.x + (slot.w - w) / 2.0,
                    slot.y + (slot.h - h) / 2.0,
                    w,
                    h,
                ),
                1.0,
            );
        }
        let x = frame.right() + 34.0;
        let column = c.right() - x;
        let title = tr("OnboardingPinTitle");
        let title_h = self.wrapped(title, 30.0, 600, column);
        let desc = tr("OnboardingPinDesc");
        let desc_w = column.min(300.0);
        let desc_h = self.wrapped(desc, 14.0, 400, desc_w);
        let mut y = c.y + ((c.h - title_h - 10.0 - desc_h) / 2.0).max(0.0);
        canvas.text(
            title,
            Rect::new(x, y, column, title_h),
            &style(30.0, 600).wrap(),
            TEXT,
        );
        y += title_h + 10.0;
        canvas.text(
            desc,
            Rect::new(x + (column - desc_w) / 2.0, y, desc_w, desc_h),
            &style(14.0, 400).wrap(),
            MUTED,
        );
    }

    fn primary(&self, canvas: &mut Canvas, target: Target, rect: Rect, text: &str) {
        let lit = self.hover == Some(target);
        canvas.round_rect(rect, 6.0, if lit { ACCENT_HOVER } else { ACCENT });
        canvas.text(
            text,
            rect,
            &TextStyle::new(FONT, 14.0).weight(600).align(Align::Center),
            Rgba::WHITE,
        );
        self.focus_ring(canvas, target, rect, 6.0);
    }

    fn ghost(&self, canvas: &mut Canvas, target: Target, rect: Rect, text: &str, color: Rgba) {
        let lit = self.hover == Some(target);
        if lit {
            canvas.round_rect(rect, 6.0, GHOST_HOVER);
        }
        // A hovered ghost button's text turns to the main text colour;
        // the danger button keeps its red (its style sets Foreground).
        let color = if lit && color == MUTED { TEXT } else { color };
        canvas.text(
            text,
            rect,
            &TextStyle::new(FONT, 14.0).weight(600).align(Align::Center),
            color,
        );
        self.focus_ring(canvas, target, rect, 6.0);
    }

    fn draw_footer(&self, canvas: &mut Canvas, now: Instant) {
        let buttons = self.footer();
        for (target, rect) in &buttons {
            match target {
                Target::Skip => self.ghost(canvas, *target, *rect, tr("OnboardingSkip"), MUTED),
                Target::Back => self.ghost(canvas, *target, *rect, tr("OnboardingBack"), MUTED),
                _ => self.primary(canvas, *target, *rect, self.next_text()),
            }
        }
        // The dots, centred between Skip and the right-hand buttons.
        let left = buttons[0].1.right();
        let right = buttons[1].1.x;
        let center = (left + right) / 2.0;
        let cy = buttons[0].1.y + buttons[0].1.h / 2.0;
        let t = EASE_OUT.at(anim::progress(self.dots_start, 0.0, DOT_MS, now));
        for (i, (from, to)) in self.dots.iter().enumerate() {
            let active = i == self.page;
            let opacity = anim::lerp(*from, *to, t);
            let cx = center + (i as f32 - 2.0) * 17.0;
            let r = if active { 3.5 * 1.25 } else { 3.5 };
            let color = if active { ACCENT } else { DOT_OFF };
            canvas.circle(cx, cy, r, color.alpha(opacity));
        }
    }
}

struct Privacy {
    card: Rect,
    inner: Rect,
    text_w: f32,
    check: Rect,
    /// The confirmation box, Disable anyway and Keep enabled.
    confirm: Option<(Rect, Rect, Rect)>,
    startup: Rect,
    startup_desc: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_focus_palette_is_written_as_wpf_colour_text() {
        assert_eq!(color_text((88, 166, 255)), "#FF58A6FF");
        assert_eq!(color_text((18, 18, 22)), "#FF121216");
    }

    #[test]
    fn the_glow_breathes_between_its_radii() {
        assert!(sine_in_out(0.0).abs() < 1e-6);
        assert!((sine_in_out(1.0) - 1.0).abs() < 1e-6);
        assert!((sine_in_out(0.5) - 0.5).abs() < 1e-6);
    }
}
