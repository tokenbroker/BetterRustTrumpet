//! Native settings controls drawn with the Canvas: toggle switch, slider,
//! drop-down, buttons, colour swatch, key caps, text box, focus ring, and
//! the window palette (Windows 11 light and dark control colours).
use bt_app::win32::gfx::{Align, Canvas, Rect, Rgba, TextStyle};

pub const FONT: &str = "Segoe UI Variable Text";
pub const DISPLAY: &str = "Segoe UI Variable Display";

/// Colours for one theme. With Mica the window background is transparent
/// so the backdrop shows through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub background: Rgba,
    pub card: Rgba,
    pub card_stroke: Rgba,
    pub divider: Rgba,
    pub text: Rgba,
    pub secondary: Rgba,
    pub tertiary: Rgba,
    pub accent: Rgba,
    pub accent_hover: Rgba,
    pub on_accent: Rgba,
    pub control: Rgba,
    pub control_hover: Rgba,
    pub control_pressed: Rgba,
    pub control_stroke: Rgba,
    pub subtle_hover: Rgba,
    pub subtle_pressed: Rgba,
    pub strong_stroke: Rgba,
    pub rail: Rgba,
    pub field: Rgba,
    pub focus: Rgba,
}

impl Palette {
    /// `accent` is the system accent; dark mode uses a lighter tint of it
    /// (SystemAccentColorLight2), light mode a darker one (Dark1).
    pub fn new(dark: bool, mica: bool, accent: Rgba, accent_dark1: Rgba) -> Self {
        if dark {
            let accent = accent.lerp(Rgba::WHITE, 0.45);
            Self {
                dark,
                background: if mica {
                    Rgba::TRANSPARENT
                } else {
                    Rgba::rgb(32, 32, 32)
                },
                card: Rgba::rgba(255, 255, 255, 0.05),
                card_stroke: Rgba::rgba(0, 0, 0, 0.1),
                divider: Rgba::rgba(255, 255, 255, 0.08),
                text: Rgba::WHITE,
                secondary: Rgba::rgba(255, 255, 255, 0.786),
                tertiary: Rgba::rgba(255, 255, 255, 0.544),
                accent,
                accent_hover: accent.alpha(0.9),
                on_accent: Rgba::BLACK,
                control: Rgba::rgba(255, 255, 255, 0.061),
                control_hover: Rgba::rgba(255, 255, 255, 0.084),
                control_pressed: Rgba::rgba(255, 255, 255, 0.033),
                control_stroke: Rgba::rgba(255, 255, 255, 0.07),
                subtle_hover: Rgba::rgba(255, 255, 255, 0.061),
                subtle_pressed: Rgba::rgba(255, 255, 255, 0.042),
                strong_stroke: Rgba::rgba(255, 255, 255, 0.6),
                rail: Rgba::rgba(255, 255, 255, 0.54),
                field: Rgba::rgba(30, 30, 30, 0.7),
                focus: Rgba::WHITE,
            }
        } else {
            Self {
                dark,
                background: if mica {
                    Rgba::TRANSPARENT
                } else {
                    Rgba::rgb(243, 243, 243)
                },
                card: Rgba::rgba(255, 255, 255, 0.7),
                card_stroke: Rgba::rgba(0, 0, 0, 0.06),
                divider: Rgba::rgba(0, 0, 0, 0.06),
                text: Rgba::rgba(0, 0, 0, 0.896),
                secondary: Rgba::rgba(0, 0, 0, 0.606),
                tertiary: Rgba::rgba(0, 0, 0, 0.446),
                accent: accent_dark1,
                accent_hover: accent_dark1.alpha(0.9),
                on_accent: Rgba::WHITE,
                control: Rgba::rgba(255, 255, 255, 0.7),
                control_hover: Rgba::rgba(249, 249, 249, 0.5),
                control_pressed: Rgba::rgba(249, 249, 249, 0.3),
                control_stroke: Rgba::rgba(0, 0, 0, 0.1),
                subtle_hover: Rgba::rgba(0, 0, 0, 0.037),
                subtle_pressed: Rgba::rgba(0, 0, 0, 0.024),
                strong_stroke: Rgba::rgba(0, 0, 0, 0.446),
                rail: Rgba::rgba(0, 0, 0, 0.446),
                field: Rgba::WHITE,
                focus: Rgba::BLACK,
            }
        }
    }
}

/// Hover and press state of a control.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub hover: bool,
    pub pressed: bool,
}

pub fn label_style(size: f32) -> TextStyle {
    TextStyle::new(FONT, size)
}

/// The two-pixel focus ring around `rect`.
pub fn focus_ring(canvas: &mut Canvas, p: &Palette, rect: Rect, radius: f32) {
    canvas.stroke_round_rect(rect.inset(-3.0, -3.0), radius + 3.0, 2.0, p.focus);
}

/// A 40x20 toggle switch.
pub fn toggle(canvas: &mut Canvas, p: &Palette, rect: Rect, on: bool, state: State) {
    let r = rect.h / 2.0;
    let knob = if state.pressed {
        7.0
    } else if state.hover {
        6.0
    } else {
        5.0
    };
    if on {
        let fill = if state.hover {
            p.accent_hover
        } else {
            p.accent
        };
        canvas.round_rect(rect, r, fill);
        canvas.circle(rect.right() - r, rect.y + r, knob, p.on_accent);
    } else {
        let fill = if state.hover {
            p.control_hover
        } else {
            Rgba::TRANSPARENT
        };
        canvas.round_rect(rect, r, fill);
        canvas.stroke_round_rect(rect.inset(0.5, 0.5), r - 0.5, 1.0, p.strong_stroke);
        canvas.circle(rect.x + r, rect.y + r, knob, p.secondary);
    }
}

/// A 20x20 checkbox; checked ones are accent-filled with a check mark.
pub fn checkbox(
    canvas: &mut Canvas,
    p: &Palette,
    rect: Rect,
    on: bool,
    icon_font: &'static str,
    state: State,
) {
    if on {
        let fill = if state.hover {
            p.accent_hover
        } else {
            p.accent
        };
        canvas.round_rect(rect, 4.0, fill);
        canvas.text(
            "\u{E73E}",
            rect,
            &TextStyle::new(icon_font, 12.0).align(Align::Center),
            p.on_accent,
        );
    } else {
        let fill = if state.pressed {
            p.control_pressed
        } else if state.hover {
            p.control_hover
        } else {
            p.control
        };
        canvas.round_rect(rect, 4.0, fill);
        canvas.stroke_round_rect(rect.inset(0.5, 0.5), 3.5, 1.0, p.strong_stroke);
    }
}

/// A horizontal slider at `fraction` (0..1).
pub fn slider(canvas: &mut Canvas, p: &Palette, rect: Rect, fraction: f32, state: State) {
    let y = rect.y + rect.h / 2.0;
    let x = rect.x + 10.0 + (rect.w - 20.0) * fraction.clamp(0.0, 1.0);
    canvas.round_rect(Rect::new(rect.x, y - 2.0, rect.w, 4.0), 2.0, p.rail);
    canvas.round_rect(Rect::new(rect.x, y - 2.0, x - rect.x, 4.0), 2.0, p.accent);
    canvas.circle(
        x,
        y,
        10.0,
        if p.dark {
            Rgba::rgb(69, 69, 69)
        } else {
            Rgba::WHITE
        },
    );
    let inner = if state.pressed {
        5.0
    } else if state.hover {
        7.0
    } else {
        6.0
    };
    canvas.circle(x, y, inner, p.accent);
}

/// A standard (or accent) button with an optional icon glyph.
#[allow(clippy::too_many_arguments)]
pub fn button(
    canvas: &mut Canvas,
    p: &Palette,
    rect: Rect,
    label: &str,
    glyph: Option<(&'static str, &'static str)>,
    primary: bool,
    state: State,
) {
    let (fill, text) = if primary {
        (
            if state.hover {
                p.accent_hover
            } else {
                p.accent
            },
            p.on_accent,
        )
    } else {
        (
            if state.pressed {
                p.control_pressed
            } else if state.hover {
                p.control_hover
            } else {
                p.control
            },
            p.text,
        )
    };
    canvas.round_rect(rect, 4.0, fill);
    if !primary {
        canvas.stroke_round_rect(rect.inset(0.5, 0.5), 3.5, 1.0, p.control_stroke);
    }
    let style = label_style(14.0).align(Align::Center);
    match glyph {
        Some((font, glyph)) => {
            let (w, _) = canvas.gfx().measure(label, &style);
            let total = 16.0 + 8.0 + w;
            let x = rect.x + (rect.w - total) / 2.0;
            canvas.text(
                glyph,
                Rect::new(x, rect.y, 16.0, rect.h),
                &TextStyle::new(font, 14.0).align(Align::Center),
                text,
            );
            canvas.text(
                label,
                Rect::new(x + 24.0, rect.y, w + 2.0, rect.h),
                &label_style(14.0),
                text,
            );
        }
        None => canvas.text(label, rect, &style, text),
    }
}

/// A drop-down (ComboBox) showing `text`, with its chevron.
pub fn dropdown(
    canvas: &mut Canvas,
    p: &Palette,
    rect: Rect,
    text: &str,
    icon_font: &'static str,
    state: State,
) {
    let fill = if state.pressed {
        p.control_pressed
    } else if state.hover {
        p.control_hover
    } else {
        p.control
    };
    canvas.round_rect(rect, 4.0, fill);
    canvas.stroke_round_rect(rect.inset(0.5, 0.5), 3.5, 1.0, p.control_stroke);
    canvas.text(
        text,
        Rect::new(rect.x + 11.0, rect.y, rect.w - 44.0, rect.h),
        &label_style(14.0).ellipsis(),
        p.text,
    );
    canvas.text(
        "\u{E70D}",
        Rect::new(rect.right() - 32.0, rect.y, 20.0, rect.h),
        &TextStyle::new(icon_font, 12.0).align(Align::Center),
        p.secondary,
    );
}

/// A colour swatch with a hairline border.
pub fn swatch(canvas: &mut Canvas, p: &Palette, rect: Rect, color: Rgba, state: State) {
    if state.hover {
        canvas.round_rect(rect.inset(-2.0, -2.0), 6.0, p.control_hover);
    }
    canvas.round_rect(rect, 4.0, color);
    canvas.stroke_round_rect(rect.inset(0.5, 0.5), 3.5, 1.0, p.control_stroke);
}

/// Key caps for a shortcut ("Ctrl+Alt+V" as three keys), right-aligned in
/// `rect`; returns their total width.
pub fn keycaps(canvas: &mut Canvas, p: &Palette, rect: Rect, text: &str) -> f32 {
    let style = label_style(12.0).weight(600).align(Align::Center);
    // "Ctrl++" keeps the trailing plus as a key.
    let mut parts: Vec<String> = vec![];
    let mut rest = text;
    while let Some(i) = rest.find('+').filter(|&i| i > 0 && i + 1 < rest.len()) {
        parts.push(rest[..i].to_owned());
        rest = &rest[i + 1..];
    }
    parts.push(rest.to_owned());
    let widths: Vec<f32> = parts
        .iter()
        .map(|part| (canvas.gfx().measure(part, &style).0 + 14.0).max(24.0))
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0 * (widths.len() as f32 - 1.0);
    let mut x = rect.right() - total;
    for (part, w) in parts.iter().zip(widths) {
        let key = Rect::new(x, rect.y + (rect.h - 24.0) / 2.0, w, 24.0);
        canvas.round_rect(key, 4.0, p.control);
        canvas.stroke_round_rect(key.inset(0.5, 0.5), 3.5, 1.0, p.control_stroke);
        canvas.text(part, key, &style, p.text);
        x += w + 4.0;
    }
    total
}

/// A single-line text box; `caret` draws the insertion point after the
/// text.
#[allow(clippy::too_many_arguments)]
pub fn text_box(
    canvas: &mut Canvas,
    p: &Palette,
    rect: Rect,
    text: &str,
    placeholder: &str,
    focused: bool,
    caret: bool,
    state: State,
) {
    let fill = if focused {
        p.field
    } else if state.hover {
        p.control_hover
    } else {
        p.control
    };
    canvas.round_rect(rect, 4.0, fill);
    canvas.stroke_round_rect(rect.inset(0.5, 0.5), 3.5, 1.0, p.control_stroke);
    // The bottom accent line of a focused Fluent text box.
    let line = if focused { p.accent } else { p.strong_stroke };
    let thickness = if focused { 2.0 } else { 1.0 };
    canvas.push_layer(1.0, Some((rect, 4.0)));
    canvas.rect(
        Rect::new(rect.x, rect.bottom() - thickness, rect.w, thickness),
        line,
    );
    canvas.pop();
    let inner = Rect::new(rect.x + 11.0, rect.y, rect.w - 22.0, rect.h);
    let style = label_style(14.0);
    if text.is_empty() {
        canvas.text(placeholder, inner, &style.ellipsis(), p.tertiary);
    } else {
        canvas.text(text, inner, &style.ellipsis(), p.text);
    }
    if focused && caret {
        let (w, _) = canvas.gfx().measure(text, &style);
        let x = (inner.x + w + 1.0).min(inner.right());
        canvas.rect(Rect::new(x, rect.y + 8.0, 1.0, rect.h - 16.0), p.text);
    }
}

/// HSV (0..1 each) to a colour.
pub fn hsv(h: f32, s: f32, v: f32) -> Rgba {
    let c = super::themes::hsv(f64::from(h), f64::from(s), f64::from(v));
    Rgba::rgb(c.0, c.1, c.2)
}

/// A colour's hue, saturation and value (0..1 each).
pub fn to_hsv(c: Rgba) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let h = if d <= f32::EPSILON {
        0.0
    } else if max == c.r {
        ((c.g - c.b) / d).rem_euclid(6.0) / 6.0
    } else if max == c.g {
        ((c.b - c.r) / d + 2.0) / 6.0
    } else {
        ((c.r - c.g) / d + 4.0) / 6.0
    };
    let s = if max <= f32::EPSILON { 0.0 } else { d / max };
    (h, s, max)
}

/// The colour picker's saturation/value square for `hue`, with the
/// current point.
pub fn sv_square(canvas: &mut Canvas, rect: Rect, hue: f32, s: f32, v: f32) {
    canvas.gradient(
        rect,
        4.0,
        &[(0.0, Rgba::WHITE), (1.0, hsv(hue, 1.0, 1.0))],
        false,
    );
    canvas.gradient(
        rect,
        4.0,
        &[(0.0, Rgba::TRANSPARENT), (1.0, Rgba::BLACK)],
        true,
    );
    let (x, y) = (rect.x + rect.w * s, rect.y + rect.h * (1.0 - v));
    canvas.circle(x, y, 7.0, Rgba::WHITE);
    canvas.circle(x, y, 5.0, hsv(hue, s, v));
}

/// The hue bar with its marker.
pub fn hue_bar(canvas: &mut Canvas, rect: Rect, hue: f32) {
    let stops: Vec<(f32, Rgba)> = (0..=6)
        .map(|i| (i as f32 / 6.0, hsv(i as f32 / 6.0, 1.0, 1.0)))
        .collect();
    canvas.gradient(rect, rect.h / 2.0, &stops, false);
    let x = rect.x + rect.w * hue;
    let y = rect.y + rect.h / 2.0;
    canvas.circle(x, y, rect.h / 2.0 + 3.0, Rgba::WHITE);
    canvas.circle(x, y, rect.h / 2.0, hsv(hue, 1.0, 1.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips() {
        let c = Rgba::rgb(0, 120, 215);
        let (h, s, v) = to_hsv(c);
        let back = hsv(h, s, v);
        assert!((back.r - c.r).abs() < 0.01 && (back.g - c.g).abs() < 0.01);
        assert!((back.b - c.b).abs() < 0.01);
    }
}
