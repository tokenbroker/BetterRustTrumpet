//! The original's own toast (ToastNotification and its manager), used
//! instead of shell balloons for QuickTrumpet confirmations and device
//! switch notices: a dark rounded box with an icon and the message
//! ("title\nbody"), at the bottom-right of the primary work area, 20 DIP
//! from its edges. Toasts stack upwards 10 DIP apart in the order they
//! were shown; when one closes, the ones above slide down into its place.
//!
//! Each toast is its own layered window that never takes focus and is
//! destroyed after it fades out, so nothing is kept while none is shown.
//! The look is fixed (it does not follow the theme), as in the original.
use super::anim::{self, EASE_IN, EASE_OUT};
use bt_app::win32::{
    gfx::{Gfx, Rect, Rgba, Surface, TextStyle},
    wnd::{self, Event, Window},
};
use std::{rc::Rc, time::Instant};

/// ToastNotification.DefaultDisplayDurationMs.
pub const DEFAULT_MS: u32 = 3000;
/// The Volume glyph ShellNotifyIcon.ShowNotification passes.
pub const VOLUME_GLYPH: &str = "\u{E767}";

const FRAME: usize = 1;
const CLOSE: usize = 2;
const FONT: &str = "Segoe UI";
const SPACING: f32 = 10.0;
const MARGIN: f32 = 20.0;
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 14.0;
const TEXT_MAX: f32 = 280.0;
const ENTER_MS: f32 = 500.0;
const FADE_IN_MS: f32 = 400.0;
const EXIT_MS: f32 = 300.0;
const MOVE_MS: f32 = 400.0;

/// WPF ExponentialEase in EaseOut mode.
fn exp_out(exponent: f32, t: f32) -> f32 {
    let ease_in = |t: f32| ((exponent * t).exp() - 1.0) / (exponent.exp() - 1.0);
    1.0 - ease_in(1.0 - t.clamp(0.0, 1.0))
}

fn text_style() -> TextStyle {
    TextStyle::new(FONT, 13.0).weight(500).wrap().top()
}

struct Toast {
    window: Window,
    surface: Option<Surface>,
    message: String,
    icon: &'static str,
    /// Box size in DIPs.
    size: (f32, f32),
    scale: f32,
    x: i32,
    /// Current top and an animated move (from, to, start), pixels.
    top: f32,
    moving: Option<(f32, f32, Instant)>,
    shown: Instant,
    closing: Option<Instant>,
}

impl Toast {
    fn height_px(&self) -> f32 {
        (self.size.1 * self.scale).round()
    }

    fn animating(&self, now: Instant) -> bool {
        anim::running(Some(self.shown), ENTER_MS, now)
            || self.closing.is_some()
            || self
                .moving
                .is_some_and(|(_, _, start)| anim::running(Some(start), MOVE_MS, now))
    }

    fn paint(&mut self, gfx: &Gfx, icon_font: &'static str, now: Instant) {
        let enter = anim::progress(self.shown, 0.0, ENTER_MS, now);
        let mut opacity = EASE_OUT.at(anim::progress(self.shown, 0.0, FADE_IN_MS, now));
        let mut dy = 40.0 * (1.0 - exp_out(6.0, enter));
        let scale = 0.92 + 0.08 * exp_out(6.0, enter);
        if let Some(start) = self.closing {
            let t = EASE_IN.at(anim::progress(start, 0.0, EXIT_MS, now));
            opacity *= 1.0 - t;
            dy += 20.0 * t;
        }
        if let Some((from, to, start)) = self.moving {
            let t = exp_out(5.0, anim::progress(start, 0.0, MOVE_MS, now));
            self.top = anim::lerp(from, to, t);
            if t >= 1.0 {
                self.moving = None;
            }
        }
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        let (w, h) = self.size;
        let pixels = (
            (w * self.scale).round() as i32,
            (h * self.scale).round() as i32,
        );
        let message = self.message.clone();
        let icon = self.icon;
        surface.draw(pixels.0, pixels.1, self.scale, |canvas| {
            // Render transforms on the box (origin bottom centre); the
            // window clips them, as WPF's SizeToContent window does.
            canvas.push_transform(0.0, dy, scale, w / 2.0, h);
            draw(canvas, gfx, icon_font, (w, h), icon, &message);
            canvas.pop_transform();
        });
        surface.present(self.window.raw(), self.x, self.top.round() as i32, opacity);
        self.surface = Some(surface);
    }
}

/// The box size in DIPs for `message`.
fn measure(gfx: &Gfx, icon_font: &'static str, icon: &str, message: &str) -> (f32, f32) {
    let (icon_w, icon_h) = icon_box(gfx, icon_font, icon);
    let (text_w, text_h) = text_size(gfx, message);
    (
        1.0 + PAD_X + icon_w + 12.0 + text_w + PAD_X + 1.0,
        1.0 + PAD_Y + icon_h.max(text_h) + PAD_Y + 1.0,
    )
}

fn icon_box(gfx: &Gfx, icon_font: &'static str, icon: &str) -> (f32, f32) {
    let (w, h) = gfx.measure(icon, &TextStyle::new(icon_font, 16.0));
    (w.ceil() + 16.0, h.ceil() + 16.0)
}

fn text_size(gfx: &Gfx, message: &str) -> (f32, f32) {
    let style = text_style();
    let widest = message
        .lines()
        .map(|line| gfx.measure(line, &style).0)
        .fold(0.0, f32::max);
    let w = widest.ceil().min(TEXT_MAX);
    (w, gfx.measure_wrapped(message, &style, w + 0.5).ceil())
}

fn draw(
    canvas: &mut bt_app::win32::gfx::Canvas,
    gfx: &Gfx,
    icon_font: &'static str,
    (w, h): (f32, f32),
    icon: &str,
    message: &str,
) {
    let rect = Rect::new(0.0, 0.0, w, h);
    canvas.round_rect(rect, 12.0, Rgba::rgba(0x11, 0x11, 0x11, 0.9));
    canvas.stroke_round_rect(
        rect.inset(0.5, 0.5),
        11.5,
        1.0,
        Rgba::rgba(255, 255, 255, 0.2),
    );
    let foreground = Rgba::rgba(255, 255, 255, 0.9);
    let (icon_w, icon_h) = icon_box(gfx, icon_font, icon);
    let inner = rect.inset(1.0 + PAD_X, 1.0 + PAD_Y);
    let icon_rect = Rect::new(inner.x, inner.y + (inner.h - icon_h) / 2.0, icon_w, icon_h);
    canvas.round_rect(icon_rect, 6.0, Rgba::rgba(255, 255, 255, 0.1));
    canvas.text(
        icon,
        icon_rect,
        &TextStyle::new(icon_font, 16.0).align(bt_app::win32::gfx::Align::Center),
        foreground,
    );
    let (text_w, text_h) = text_size(gfx, message);
    let text = Rect::new(
        icon_rect.right() + 12.0,
        inner.y + (inner.h - text_h) / 2.0,
        text_w + 0.5,
        text_h,
    );
    canvas.text(message, text, &text_style(), foreground);
}

pub struct Toasts {
    gfx: Rc<Gfx>,
    icon_font: &'static str,
    list: Vec<Toast>,
}

impl Toasts {
    pub fn new(gfx: Rc<Gfx>, icon_font: &'static str) -> Self {
        Self {
            gfx,
            icon_font,
            list: vec![],
        }
    }

    pub fn owns(&self, window: Window) -> bool {
        self.list.iter().any(|t| t.window == window)
    }

    /// ShellNotifyIcon.ShowNotification: "title\nmessage" with the volume
    /// glyph, for `ms` milliseconds.
    pub fn notify(&mut self, title: &str, message: &str, ms: u32) {
        let text = if title.is_empty() {
            message.to_owned()
        } else {
            format!("{title}\n{message}")
        };
        self.show(&text, VOLUME_GLYPH, ms);
    }

    /// ToastNotificationManager.Show.
    pub fn show(&mut self, message: &str, icon: &'static str, ms: u32) {
        let window = match wnd::create(
            "BetterTrumpet notification",
            wnd::Options {
                activate: false,
                click_through: false,
            },
        ) {
            Ok(window) => window,
            Err(e) => {
                eprintln!("BetterTrumpet: the notification failed to open: {e}");
                return;
            }
        };
        let Ok(surface) = Surface::new(self.gfx.clone()) else {
            window.destroy();
            return;
        };
        // SystemParameters.WorkArea: the primary monitor's work area.
        let monitor = wnd::monitor_at(0, 0);
        let scale = monitor.scale;
        let size = measure(&self.gfx, self.icon_font, icon, message);
        let (wx, _, ww, _) = monitor.work;
        let x = wx + ww - ((size.0 + MARGIN) * scale).round() as i32;
        let mut toast = Toast {
            window,
            surface: Some(surface),
            message: message.to_owned(),
            icon,
            size,
            scale,
            x,
            top: 0.0,
            moving: None,
            shown: Instant::now(),
            closing: None,
        };
        toast.top = self.targets_with(&toast).last().copied().unwrap_or(0.0);
        let now = Instant::now();
        toast.paint(&self.gfx, self.icon_font, now);
        toast.window.show(false);
        toast.window.set_timer(FRAME, 15);
        toast.window.set_timer(CLOSE, ms.max(1));
        self.list.push(toast);
        self.reposition();
    }

    /// The tops (pixels) of the current toasts plus `extra` stacked above.
    fn targets_with(&self, extra: &Toast) -> Vec<f32> {
        let monitor = wnd::monitor_at(0, 0);
        let (_, wy, _, wh) = monitor.work;
        let bottom = (wy + wh) as f32;
        let mut offset = MARGIN * monitor.scale;
        self.list
            .iter()
            .chain(Some(extra))
            .map(|t| {
                let top = bottom - offset - t.height_px();
                offset += t.height_px() + SPACING * monitor.scale;
                top
            })
            .collect()
    }

    /// RepositionAllToasts: moves each toast to its slot, animated when it
    /// is more than 5 DIP away.
    fn reposition(&mut self) {
        let monitor = wnd::monitor_at(0, 0);
        let (_, wy, _, wh) = monitor.work;
        let bottom = (wy + wh) as f32;
        let mut offset = MARGIN * monitor.scale;
        let now = Instant::now();
        for toast in &mut self.list {
            let target = bottom - offset - toast.height_px();
            offset += toast.height_px() + SPACING * monitor.scale;
            let goal = toast.moving.map_or(toast.top, |(_, to, _)| to);
            if (goal - target).abs() > 5.0 * monitor.scale {
                toast.moving = Some((toast.top, target, now));
                toast.window.set_timer(FRAME, 15);
            } else if toast.moving.is_none() && toast.top != target {
                toast.top = target;
                toast.paint(&self.gfx, self.icon_font, now);
            }
        }
    }

    pub fn event(&mut self, window: Window, event: Event) {
        let Some(index) = self.list.iter().position(|t| t.window == window) else {
            return;
        };
        let now = Instant::now();
        let gfx = self.gfx.clone();
        let toast = &mut self.list[index];
        match event {
            Event::Timer(CLOSE) => {
                toast.window.kill_timer(CLOSE);
                if toast.closing.is_none() {
                    toast.closing = Some(now);
                    toast.window.set_timer(FRAME, 15);
                }
            }
            Event::Timer(FRAME) => {
                toast.paint(&gfx, self.icon_font, now);
                if toast
                    .closing
                    .is_some_and(|start| !anim::running(Some(start), EXIT_MS, now))
                {
                    let mut toast = self.list.remove(index);
                    toast.surface = None;
                    toast.window.destroy();
                    self.reposition();
                } else if !toast.animating(now) {
                    toast.window.kill_timer(FRAME);
                }
            }
            _ => {}
        }
    }

    /// Draws `message` as a toast in its final state without a window.
    pub fn render(&self, message: &str) -> Option<(u32, u32, Vec<u8>)> {
        let mut surface = Surface::new(self.gfx.clone()).ok()?;
        let scale = wnd::monitor_at(0, 0).scale;
        let (w, h) = measure(&self.gfx, self.icon_font, VOLUME_GLYPH, message);
        surface.draw(
            (w * scale).round() as i32,
            (h * scale).round() as i32,
            scale,
            |canvas| {
                draw(
                    canvas,
                    &self.gfx,
                    self.icon_font,
                    (w, h),
                    VOLUME_GLYPH,
                    message,
                )
            },
        );
        surface.rgba()
    }
}

impl Drop for Toasts {
    fn drop(&mut self) {
        for mut toast in self.list.drain(..) {
            toast.surface = None;
            toast.window.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::exp_out;

    #[test]
    fn exponential_ease_out_runs_from_zero_to_one_fast() {
        assert!(exp_out(6.0, 0.0).abs() < 1e-6);
        assert!((exp_out(6.0, 1.0) - 1.0).abs() < 1e-6);
        assert!(exp_out(6.0, 0.2) > 0.65);
    }
}
