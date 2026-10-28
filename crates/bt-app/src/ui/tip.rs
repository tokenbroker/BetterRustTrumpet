//! Tooltips, drawn in a click-through window that never takes focus.
//!
//! - `Look::Tray`: the tray hover tooltip shown above the media popup while
//!   it is up (the original blanks the native one then): 12 DIP semibold
//!   white on #E81C1C1E, a 1 DIP #46FFFFFF border, radius 8.
//! - `Look::Wpf`: the original's ToolTip style (App.xaml) on WPF's Aero2
//!   template, used in the flyout: 12 DIP Segoe UI, padding 4, a 1 DIP
//!   {Theme}ChromeDisabledLow border on {Theme}ChromeMediumLow, opacity
//!   0.92, square corners and the 5 DIP system drop shadow.
//!
//! Under high contrast both take WindowText on Window with a WindowFrame
//! border (the ToolTip style's HighContrast branches).
use super::{
    anim::{self, Curve},
    theme::HighContrast,
};
use bt_app::win32::{
    gfx::{Align, Gfx, Rect, Rgba, Surface, TextStyle},
    wnd::{self, Window},
};
use std::{rc::Rc, time::Instant};

const FRAME: usize = 1;
const FONT: &str = "Segoe UI Variable Text";
/// Room around the box for its shadow.
const MARGIN: f32 = 8.0;
const HEIGHT: f32 = 40.0;
/// The WPF tooltip's font, padding plus border, and shadow margin.
const WPF_FONT: &str = "Segoe UI";
const WPF_INSET: f32 = 5.0;
const WPF_SHADOW: f32 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Tray,
    Wpf { light: bool },
}

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
    look: Look,
    /// High contrast's system colours, set by the UI with the theme.
    pub contrast: Option<HighContrast>,
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
            surface: Some(super::flow_surface(&gfx)?),
            gfx,
            visible: false,
            text: String::new(),
            shown: Instant::now(),
            size: (0.0, HEIGHT),
            position: (0, 0),
            scale: 1.0,
            look: Look::Tray,
            contrast: None,
        })
    }

    fn style() -> TextStyle {
        TextStyle::new(FONT, 12.0).weight(600)
    }

    fn wpf_style() -> TextStyle {
        TextStyle::new(WPF_FONT, 12.0)
    }

    /// The WPF look's window size in DIPs (box plus shadow margin).
    fn wpf_size(&self, text: &str) -> (f32, f32) {
        let (w, h) = self.gfx.measure(text, &Self::wpf_style());
        (
            w.ceil() + 2.0 * WPF_INSET + WPF_SHADOW,
            h.ceil() + 2.0 * WPF_INSET + WPF_SHADOW,
        )
    }

    /// Shows `text` in the original's ToolTip style with its top-left
    /// corner (top-right with `end`) at screen pixel (`x`, `y`). Where it
    /// would cross the bottom of the work area it moves up to end at
    /// `flip_bottom` instead (WPF flips a popup that does not fit); it
    /// stays inside horizontally.
    pub fn show_at(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        end: bool,
        flip_bottom: i32,
        light: bool,
    ) {
        let monitor = wnd::monitor_at(x, y);
        self.scale = monitor.scale;
        self.look = Look::Wpf { light };
        self.size = self.wpf_size(text);
        let width = (self.size.0 * self.scale).round() as i32;
        let height = (self.size.1 * self.scale).round() as i32;
        let (wx, wy, ww, wh) = monitor.work;
        let x = if end { x - width } else { x };
        let x = x.clamp(wx, (wx + ww - width).max(wx));
        let y = if y + height > wy + wh {
            (flip_bottom - height).max(wy)
        } else {
            y.max(wy)
        };
        self.text = text.into();
        self.position = (x, y);
        self.appear();
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
        self.look = Look::Tray;
        self.appear();
    }

    fn appear(&mut self) {
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

    /// Draws `text` in `look` without showing the window.
    pub fn render(&mut self, text: &str, look: Look) -> Option<(u32, u32, Vec<u8>)> {
        self.look = look;
        self.size = match look {
            Look::Tray => {
                let (text_w, _) = self.gfx.measure(text, &Self::style());
                (text_w.ceil() + 20.0 + 2.0 * MARGIN, HEIGHT)
            }
            Look::Wpf { .. } => self.wpf_size(text),
        };
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
        let contrast = self
            .contrast
            .map(|hc| (hc.window, hc.window_frame, hc.window_text));
        if let Look::Wpf { light } = self.look {
            surface.draw(pixels.0, pixels.1, self.scale, |canvas| {
                let rect = Rect::new(0.0, 0.0, w - WPF_SHADOW, h - WPF_SHADOW);
                // SystemDropShadowChrome: #71000000 offset 5 DIP, soft edges.
                for i in 0..4 {
                    let spread = i as f32;
                    canvas.round_rect(
                        rect.offset(WPF_SHADOW - 1.0, WPF_SHADOW - 1.0)
                            .inset(spread - 1.0, spread - 1.0),
                        3.0 - spread * 0.5,
                        Rgba::rgba(0, 0, 0, 0.44 / 4.0),
                    );
                }
                let (background, border, foreground) = if let Some(colors) = contrast {
                    colors
                } else if light {
                    (
                        Rgba::rgb(242, 242, 242),
                        Rgba::rgb(122, 122, 122),
                        Rgba::BLACK,
                    )
                } else {
                    (Rgba::rgb(43, 43, 43), Rgba::rgb(133, 133, 133), Rgba::WHITE)
                };
                canvas.rect(rect, border);
                canvas.rect(rect.inset(1.0, 1.0), background);
                canvas.text(
                    &text,
                    rect.inset(WPF_INSET, WPF_INSET),
                    &Self::wpf_style(),
                    foreground,
                );
            });
            surface.present(
                self.window.raw(),
                self.position.0,
                self.position.1,
                0.92 * fade,
            );
            self.surface = Some(surface);
            return;
        }
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
            let (background, border, foreground) = contrast.unwrap_or((
                Rgba::rgba(28, 28, 30, 0.91),
                Rgba::rgba(255, 255, 255, 0.27),
                Rgba::WHITE,
            ));
            canvas.round_rect(rect, 8.0, background);
            canvas.stroke_round_rect(rect, 8.0, 1.0, border);
            canvas.text(&text, rect, &Self::style().align(Align::Center), foreground);
        });
        surface.present(self.window.raw(), self.position.0, self.position.1, fade);
        self.surface = Some(surface);
    }
}
