//! The tray icon's image (TaskbarIconSource + VolumeIconGenerator): which
//! icon the default output calls for, and the original's vector glyphs
//! drawn in its 24-unit space with 4x4 supersampling, like its 4x render.
//!
//! The modern icon is the speaker with both waves (any non-zero level),
//! the speaker with an X when muted, or headphones/monitor shapes when
//! TrayIconShowsDeviceType is on. UseLegacyIcon and "no device" use
//! Windows' own SndVolSSO.dll icons, recoloured black in the light theme.
use bt_app::win32::trayicon::{self, Icon, SndVol};
use bt_audio::form_factor;

/// DeviceViewModel.DeviceIconKind for the default output, or no device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Muted,
    Bars(u8),
    NoDevice,
}

/// The default output's shape (TrayDeviceGlyph).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Speaker,
    Headphones,
    Monitor,
}

/// Everything the icon depends on (TaskbarIconSource.GetHash).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct State {
    pub kind: Kind,
    pub glyph: Glyph,
    pub light: bool,
    pub legacy: bool,
    pub size: u32,
}

/// UpdateMasterVolumeIcon: Windows 11 uses strict thresholds, 10 inclusive.
pub fn kind(default: Option<(f32, bool)>, windows_11: bool) -> Kind {
    let Some((volume, muted)) = default else {
        return Kind::NoDevice;
    };
    if muted {
        return Kind::Muted;
    }
    let above = |t: f32| {
        if windows_11 { volume > t } else { volume >= t }
    };
    Kind::Bars(if above(0.66) {
        3
    } else if above(0.33) {
        2
    } else if volume > 0.0 {
        1
    } else {
        0
    })
}

/// GlyphFromDeviceCollection: only with TrayIconShowsDeviceType, never with
/// the legacy icon.
pub fn glyph(form: Option<u32>, shows_device_type: bool, legacy: bool) -> Glyph {
    if !shows_device_type || legacy {
        return Glyph::Speaker;
    }
    match form {
        Some(form_factor::HEADPHONES | form_factor::HEADSET | form_factor::HANDSET) => {
            Glyph::Headphones
        }
        Some(form_factor::DIGITAL_AUDIO_DISPLAY_DEVICE) => Glyph::Monitor,
        _ => Glyph::Speaker,
    }
}

/// The icon for `state`, or `None` if it could not be made.
pub fn make(state: State) -> Option<Icon> {
    let pixels = pixels(state)?;
    trayicon::from_bgra(state.size, &pixels)
}

/// Straight-alpha BGRA pixels for `state`.
pub fn pixels(state: State) -> Option<Vec<u8>> {
    if !state.legacy && state.kind != Kind::NoDevice {
        let color = if state.light {
            [20, 20, 20]
        } else {
            [255, 255, 255]
        };
        let muted = state.kind == Kind::Muted;
        let ops = match state.glyph {
            Glyph::Speaker if muted => muted_speaker(),
            Glyph::Speaker => speaker(1.0, 1.0),
            glyph => device(glyph, muted),
        };
        let scale = if state.glyph == Glyph::Speaker {
            1.0
        } else {
            0.9
        };
        return Some(render(state.size, &ops, scale, color));
    }
    let id = match state.kind {
        Kind::Muted => SndVol::Muted,
        Kind::Bars(0) => SndVol::SpeakerZeroBars,
        Kind::Bars(1) => SndVol::SpeakerOneBar,
        Kind::Bars(2) => SndVol::SpeakerTwoBars,
        Kind::Bars(_) => SndVol::SpeakerThreeBars,
        Kind::NoDevice => SndVol::NoDevice,
    };
    let mut pixels = trayicon::sndvol_bgra(id, state.size)?;
    if state.light {
        // ColorIconForLightTheme: near-white pixels turn black, only in the
        // left 40% for "no device" so its red X stays.
        let fill = if id == SndVol::NoDevice { 0.4 } else { 1.0 };
        recolor(&mut pixels, state.size, fill);
    }
    Some(pixels)
}

/// IconHelper.ColorIcon to black.
fn recolor(pixels: &mut [u8], size: u32, fill: f32) {
    let columns = (size as f32 * fill).ceil() as usize;
    for (i, p) in pixels.chunks_exact_mut(4).enumerate() {
        // BGRA: red is byte 2.
        if i % size as usize >= columns || p[2] <= 220 {
            continue;
        }
        p[0] = 0;
        p[1] = 0;
        p[2] = 0;
    }
}

// ---------- shapes ----------

type Point = (f32, f32);

#[derive(Clone, Debug)]
enum Shape {
    Polygon(Vec<Point>),
    /// Circle centre, radius, start and sweep in degrees (clockwise, y down,
    /// as GDI+ DrawArc), pen width; round caps.
    Arc(Point, f32, f32, f32, f32),
    /// Polyline with round caps and joins, pen width.
    Lines(Vec<Point>, f32),
    /// x, y, width, height, corner radius (GDI+ arcs of diameter 2r).
    RoundRect(f32, f32, f32, f32, f32),
    /// The same outline stroked with a pen width.
    RoundRectStroke(f32, f32, f32, f32, f32, f32),
}

#[derive(Clone, Debug)]
struct Op {
    shape: Shape,
    alpha: f32,
    /// SourceCopy with a transparent pen: clears what is below.
    erase: bool,
}

fn paint(shape: Shape) -> Op {
    Op {
        shape,
        alpha: 1.0,
        erase: false,
    }
}

fn distance_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    };
    let (x, y) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (x * x + y * y).sqrt()
}

/// Signed distance to a rounded rectangle's outline (negative inside).
fn round_rect_distance(p: Point, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let r = r.min(w / 2.0).min(h / 2.0);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let qx = (p.0 - cx).abs() - (w / 2.0 - r);
    let qy = (p.1 - cy).abs() - (h / 2.0 - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

impl Shape {
    fn contains(&self, p: Point) -> bool {
        match self {
            Shape::Polygon(points) => {
                let mut inside = false;
                let mut j = points.len() - 1;
                for i in 0..points.len() {
                    let (a, b) = (points[i], points[j]);
                    if (a.1 > p.1) != (b.1 > p.1)
                        && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0
                    {
                        inside = !inside;
                    }
                    j = i;
                }
                inside
            }
            Shape::Arc((cx, cy), r, start, sweep, width) => {
                let (dx, dy) = (p.0 - cx, p.1 - cy);
                let angle = dy.atan2(dx).to_degrees();
                let half = width / 2.0;
                if (angle - start).rem_euclid(360.0) <= *sweep {
                    return ((dx * dx + dy * dy).sqrt() - r).abs() <= half;
                }
                [*start, start + sweep].iter().any(|a| {
                    let a = a.to_radians();
                    let (ex, ey) = (cx + r * a.cos(), cy + r * a.sin());
                    ((p.0 - ex).powi(2) + (p.1 - ey).powi(2)).sqrt() <= half
                })
            }
            Shape::Lines(points, width) => points
                .windows(2)
                .any(|s| distance_to_segment(p, s[0], s[1]) <= width / 2.0),
            Shape::RoundRect(x, y, w, h, r) => round_rect_distance(p, *x, *y, *w, *h, *r) <= 0.0,
            Shape::RoundRectStroke(x, y, w, h, r, width) => {
                round_rect_distance(p, *x, *y, *w, *h, *r).abs() <= width / 2.0
            }
        }
    }
}

/// Draws `ops` in the 24-unit space scaled by `glyph_scale` about the
/// centre (ScaleDeviceGlyph) into `size` x `size` BGRA pixels of `color`.
fn render(size: u32, ops: &[Op], glyph_scale: f32, color: [u8; 3]) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let unit = 24.0 / size as f32;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let mut total = 0.0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = (px as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * unit;
                    let y = (py as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * unit;
                    let p = (
                        12.0 + (x - 12.0) / glyph_scale,
                        12.0 + (y - 12.0) / glyph_scale,
                    );
                    let mut alpha = 0.0;
                    for op in ops {
                        if op.shape.contains(p) {
                            alpha = if op.erase {
                                0.0
                            } else {
                                op.alpha + alpha * (1.0 - op.alpha)
                            };
                        }
                    }
                    total += alpha;
                }
            }
            let alpha = total / (SAMPLES * SAMPLES) as f32;
            let i = ((py * size + px) * 4) as usize;
            out[i] = color[2];
            out[i + 1] = color[1];
            out[i + 2] = color[0];
            out[i + 3] = (alpha * 255.0).round() as u8;
        }
    }
    out
}

fn speaker_body() -> Op {
    paint(Shape::Polygon(vec![
        (11.0, 4.7),
        (11.0, 19.3),
        (6.4, 16.5),
        (5.4, 16.0),
        (3.0, 16.0),
        (2.0, 15.0),
        (2.0, 9.0),
        (3.0, 8.0),
        (5.4, 8.0),
        (6.4, 7.5),
    ]))
}

/// The speaker with its waves at the given opacities (animation frames use
/// partial ones; the static icon shows both fully).
fn speaker(wave1: f32, wave2: f32) -> Vec<Op> {
    let mut ops = vec![speaker_body()];
    // Color.FromArgb((int)(255 * opacity)).
    let quantize = |o: f32| (255.0 * o).floor() / 255.0;
    if wave1 > 0.0 {
        ops.push(Op {
            alpha: quantize(wave1),
            ..paint(Shape::Arc((13.0, 12.0), 3.0, -50.0, 100.0, 2.0))
        });
    }
    if wave2 > 0.0 {
        ops.push(Op {
            alpha: quantize(wave2),
            ..paint(Shape::Arc((13.0, 12.0), 6.5, -55.0, 110.0, 2.0))
        });
    }
    ops
}

fn muted_speaker() -> Vec<Op> {
    vec![
        speaker_body(),
        paint(Shape::Lines(vec![(16.0, 9.0), (22.0, 15.0)], 2.0)),
        paint(Shape::Lines(vec![(16.0, 15.0), (22.0, 9.0)], 2.0)),
    ]
}

fn device(glyph: Glyph, muted: bool) -> Vec<Op> {
    let mut ops = match glyph {
        Glyph::Headphones => vec![
            paint(Shape::Arc((12.0, 11.0), 9.0, 180.0, 180.0, 2.0)),
            paint(Shape::RoundRect(2.0, 10.5, 6.0, 9.5, 2.0)),
            paint(Shape::RoundRect(16.0, 10.5, 6.0, 9.5, 2.0)),
        ],
        _ => vec![
            paint(Shape::RoundRectStroke(2.5, 4.0, 19.0, 12.5, 2.0, 2.0)),
            paint(Shape::Lines(vec![(12.0, 16.5), (12.0, 20.5)], 2.0)),
            paint(Shape::Lines(vec![(8.0, 20.5), (16.0, 20.5)], 2.0)),
        ],
    };
    if muted {
        ops.push(Op {
            erase: true,
            ..paint(Shape::Lines(vec![(3.0, 3.0), (21.0, 21.0)], 4.0))
        });
        ops.push(paint(Shape::Lines(vec![(3.0, 3.0), (21.0, 21.0)], 2.0)));
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_the_windows_thresholds() {
        assert_eq!(kind(None, true), Kind::NoDevice);
        assert_eq!(kind(Some((0.5, true)), true), Kind::Muted);
        assert_eq!(kind(Some((0.66, false)), true), Kind::Bars(2));
        assert_eq!(kind(Some((0.66, false)), false), Kind::Bars(3));
        assert_eq!(kind(Some((0.33, false)), true), Kind::Bars(1));
        assert_eq!(kind(Some((0.33, false)), false), Kind::Bars(2));
        assert_eq!(kind(Some((0.01, false)), true), Kind::Bars(1));
        assert_eq!(kind(Some((0.0, false)), true), Kind::Bars(0));
    }

    #[test]
    fn glyphs_need_the_setting_and_the_modern_icon() {
        let headset = Some(form_factor::HEADSET);
        assert_eq!(glyph(headset, true, false), Glyph::Headphones);
        assert_eq!(glyph(headset, false, false), Glyph::Speaker);
        assert_eq!(glyph(headset, true, true), Glyph::Speaker);
        assert_eq!(
            glyph(Some(form_factor::DIGITAL_AUDIO_DISPLAY_DEVICE), true, false),
            Glyph::Monitor
        );
        assert_eq!(glyph(Some(1), true, false), Glyph::Speaker);
    }

    fn alpha_at(pixels: &[u8], size: u32, x: u32, y: u32) -> u8 {
        pixels[((y * size + x) * 4 + 3) as usize]
    }

    #[test]
    fn draws_the_speaker_waves_and_mute_cross() {
        let size = 24;
        let white = [255, 255, 255];
        let plain = render(size, &speaker(1.0, 1.0), 1.0, white);
        // Speaker body, small wave (x = 16 on the arc), no X.
        assert_eq!(alpha_at(&plain, size, 7, 12), 255);
        assert!(alpha_at(&plain, size, 15, 12) > 200);
        let muted = render(size, &muted_speaker(), 1.0, white);
        assert!(alpha_at(&muted, size, 18, 11) > 100);
        assert_eq!(alpha_at(&muted, size, 23, 0), 0);
        let slashed = render(size, &device(Glyph::Headphones, true), 0.9, white);
        // The eraser leaves a gap beside the slash over the left cup.
        assert!(alpha_at(&slashed, size, 4, 15) > 200);
    }
}
