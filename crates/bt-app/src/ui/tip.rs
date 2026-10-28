//! The tray hover tooltip shown above the media popup while it is up (the
//! original blanks the native one then): 12 DIP semibold white on
//! #E81C1C1E, a 1 DIP #46FFFFFF border, radius 8. It ignores the pointer.
use super::anim::{self, Curve};
use bt_app::win32::{
    gfx::{Gfx, Rect, Rgba, Surface, TextStyle},
    wnd::{self, Window},
};
use std::{rc::Rc, time::Instant};

const FRAME: usize = 1;
const FONT: &str = "Segoe UI Variable Text";
/// Room around the box for its shadow.
const MARGIN: f32 = 8.0;
const HEIGHT: f32 = 40.0;

pub struct Tip {
    pub window: Window,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    pub visible: bool,
    text: String,
    shown: Instant,
    size: (f32, f32),
    position: (i32, i32),
    scale: f32,
}

impl Tip {
    pub fn new(gfx: Rc<Gfx>) -> windows::core::Result<Self> {
        let window = wnd::create(
            "BetterTrumpet tooltip",
            wnd::Options {
                activate: false,
                click_through: true,
            },
        )?;
        Ok(Self {
            window,
            surface: Some(Surface::new(gfx.clone())?),
            gfx,
            visible: false,
            text: String::new(),
            shown: Instant::now(),
            size: (0.0, HEIGHT),
            position: (0, 0),
            scale: 1.0,
        })
    }

    fn style() -> TextStyle {
        TextStyle::new(FONT, 12.0).weight(600)
    }

    /// Shows `text` centred on `center_x` with its bottom edge at `bottom`
    /// (screen pixels), kept 4 DIP inside the work area.
    pub fn show(&mut self, text: &str, center_x: i32, bottom: i32) {
        let monitor = wnd::monitor_at(center_x, bottom);
        self.scale = monitor.scale;
        let (text_w, _) = self.gfx.measure(text, &Self::style());
        self.size = (text_w.ceil() + 20.0 + 2.0 * MARGIN, HEIGHT);
        let width = (self.size.0 * self.scale).round() as i32;
        let height = (self.size.1 * self.scale).round() as i32;
        let inset = (4.0 * self.scale) as i32;
        let (wx, wy, ww, _) = monitor.work;
        let left = wx + inset;
        let right = wx + ww - width - inset;
        let x = (center_x - width / 2).clamp(left, right.max(left));
        let y = (bottom - height).max(wy + inset);
        self.position = (x, y);
        self.text = text.into();
        if !self.visible {
            self.shown = Instant::now();
            self.visible = true;
            self.paint();
            self.window.show(false);
            self.window.set_timer(FRAME, 15);
        } else {
            self.paint();
        }
    }

    pub fn hide(&mut self) {
        if self.visible {
            self.visible = false;
            self.window.kill_timer(FRAME);
            self.window.hide();
            if let Some(surface) = &mut self.surface {
                surface.release();
            }
        }
    }

    /// Draws `text` without showing the window.
    pub fn render(&mut self, text: &str) -> Option<(u32, u32, Vec<u8>)> {
        let (text_w, _) = self.gfx.measure(text, &Self::style());
        self.size = (text_w.ceil() + 20.0 + 2.0 * MARGIN, HEIGHT);
        self.scale = wnd::monitor_at(0, 0).scale;
        self.text = text.into();
        self.shown = Instant::now() - std::time::Duration::from_secs(5);
        self.paint();
        self.surface.as_ref()?.rgba()
    }

    /// The 200 ms fade-in.
    pub fn timer(&mut self, id: usize) {
        if id != FRAME || !self.visible {
            return;
        }
        self.paint();
        if !anim::running(Some(self.shown), 200.0, Instant::now()) {
            self.window.kill_timer(FRAME);
        }
    }

    fn paint(&mut self) {
        let fade =
            Curve(0.0, 0.0, 0.58, 1.0).at(anim::progress(self.shown, 0.0, 200.0, Instant::now()));
        let (w, h) = self.size;
        let pixels = (
            (w * self.scale).round() as i32,
            (h * self.scale).round() as i32,
        );
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        let text = self.text.clone();
        surface.draw(pixels.0, pixels.1, self.scale, |canvas| {
            // The box sits 6 DIP above the window's bottom edge.
            let (_, text_h) = canvas.gfx().measure(&text, &Self::style());
            let box_h = text_h.ceil() + 12.0;
            let rect = Rect::new(MARGIN, h - 6.0 - box_h, w - 2.0 * MARGIN, box_h);
            for i in (1..=4).rev() {
                let spread = i as f32 * 1.5;
                canvas.round_rect(
                    rect.offset(0.0, 2.0).inset(-spread, -spread),
                    8.0 + spread,
                    Rgba::rgba(0, 0, 0, 0.35 / 4.0 * (1.0 - i as f32 / 5.0)),
                );
            }
            canvas.round_rect(rect, 8.0, Rgba::rgba(28, 28, 30, 0.91));
            canvas.stroke_round_rect(rect, 8.0, 1.0, Rgba::rgba(255, 255, 255, 0.27));
            canvas.text(
                &text,
                rect,
                &Self::style().align(bt_app::win32::gfx::Align::Center),
                Rgba::WHITE,
            );
        });
        surface.present(self.window.raw(), self.position.0, self.position.1, fade);
        self.surface = Some(surface);
    }
}
