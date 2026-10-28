//! The tray icon's image (TaskbarIconSource + VolumeIconGenerator): which
//! icon the default output calls for, and the original's vector glyphs
//! drawn in its 24-unit space with 4x4 supersampling, like its 4x render.
//!
//! The modern icon is the speaker with both waves (any non-zero level),
//! the speaker with an X when muted, or headphones/monitor shapes when
//! TrayIconShowsDeviceType is on. UseLegacyIcon and "no device" use
//! Windows' own SndVolSSO.dll icons, recoloured black in the light theme
//! (or to WindowText / HighlightText under high contrast).
//!
//! While media plays the modern icon animates (VolumeIconGenerator): the
//! speaker's waves fade in over a 16-frame loop, a waveform scrolls inside
//! the headphones (48 frames) and an equalizer bounces on the monitor (16
//! frames; both hold each pose for two frames), at 50 ms a frame.
use bt_app::win32::trayicon::{self, SndVol};
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
    /// The playback animation's pose (a canonical frame), while it runs.
    pub frame: Option<u16>,
    /// High contrast: the colour the SndVolSSO icons are painted in
    /// (ColorIconForHighContrast).
    pub contrast: Option<[u8; 3]>,
}

/// The animation's frame interval (the original's DispatcherTimer).
pub const FRAME_MS: u32 = 50;
/// Speaker frames; the waves fade in over frames 0-6 and 3-10.
const SPEAKER_FRAMES: u16 = 16;
const WAVE1: (u16, u16) = (0, 6);
const WAVE2: (u16, u16) = (3, 10);
/// HeadphonesAnimationFrames (2.4 s) and MonitorAnimationFrames (0.8 s).
const HEADPHONES_FRAMES: u16 = 48;
const MONITOR_FRAMES: u16 = 16;
/// Device glyph poses are held for two frames.
const DEVICE_STEP: u16 = 2;

/// CanAnimate: the modern icon, except a muted headphones or monitor (a
/// muted or absent speaker still shows its waves while media plays).
pub fn can_animate(state: &State) -> bool {
    !state.legacy && !(state.glyph != Glyph::Speaker && state.kind == Kind::Muted)
}

/// GetFrameCount: frames in one loop of the glyph's animation.
pub fn frame_count(glyph: Glyph) -> u16 {
    match glyph {
        Glyph::Speaker => SPEAKER_FRAMES,
        Glyph::Headphones => HEADPHONES_FRAMES,
        Glyph::Monitor => MONITOR_FRAMES,
    }
}

/// CalculateWaveOpacity with its ease-out cubic.
fn wave_opacity(frame: u16, (start, end): (u16, u16)) -> f32 {
    if frame < start {
        return 0.0;
    }
    if frame >= end {
        return 1.0;
    }
    let t = f32::from(frame - start) / f32::from(end - start);
    1.0 - (1.0 - t).powi(3)
}

/// GetCanonicalFrame: the lowest frame drawn exactly like `frame`, so
/// repeated poses are not pushed to the shell again.
pub fn canonical(glyph: Glyph, frame: u16) -> u16 {
    let frame = if frame >= frame_count(glyph) {
        0
    } else {
        frame
    };
    match glyph {
        Glyph::Speaker => {
            let pose = |f| (wave_opacity(f, WAVE1), wave_opacity(f, WAVE2));
            (0..frame)
                .find(|&j| pose(j) == pose(frame))
                .unwrap_or(frame)
        }
        _ => frame / DEVICE_STEP * DEVICE_STEP,
    }
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

/// TaskbarIconSource's animation state: whether media plays, and the
/// running loop. The UI runs a FRAME_MS timer only while `running()`.
#[derive(Default)]
pub struct Animator {
    /// A Windows media session is playing (MediaSessionService).
    pub session_playing: bool,
    /// A legacy player is playing (LegacyMediaPlayerService).
    pub legacy_playing: bool,
    /// The glyph being animated and the current frame.
    running: Option<(Glyph, u16)>,
    /// Rendered poses of the running loop, so each is drawn once.
    cache: Vec<(State, Vec<u8>)>,
}

impl Animator {
    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    /// Starts or stops the loop for the static icon `base` (StartAnimation
    /// / StopAnimation); `true` when that changed.
    pub fn update(&mut self, base: &State) -> bool {
        let wanted = (self.session_playing || self.legacy_playing) && can_animate(base);
        if wanted == self.running() {
            return false;
        }
        self.running = wanted.then_some((base.glyph, 0));
        self.cache.clear();
        true
    }

    /// OnAnimationTick: the next frame; a different glyph (the default
    /// output changed kind) restarts from its first frame.
    pub fn tick(&mut self, base: &State) {
        if let Some((glyph, frame)) = &mut self.running {
            if *glyph != base.glyph {
                *glyph = base.glyph;
                *frame = 0;
            } else {
                *frame = (*frame + 1) % frame_count(*glyph);
            }
        }
    }

    /// What to show for `base`: the current pose while running.
    pub fn shown(&self, base: State) -> State {
        State {
            frame: self.running.map(|(glyph, frame)| canonical(glyph, frame)),
            ..base
        }
    }

    /// `pixels`, drawing each pose of the running loop only once.
    pub fn pixels(&mut self, state: State) -> Option<Vec<u8>> {
        if state.frame.is_none() {
            return pixels(state);
        }
        if let Some((_, cached)) = self.cache.iter().find(|(s, _)| *s == state) {
            return Some(cached.clone());
        }
        let drawn = pixels(state)?;
        // A theme, size or glyph change starts a new set of poses.
        self.cache.retain(|(s, _)| {
            State { frame: None, ..*s }
                == State {
                    frame: None,
                    ..state
                }
        });
        self.cache.push((state, drawn.clone()));
        Some(drawn)
    }
}

/// Straight-alpha BGRA pixels for `state`.
pub fn pixels(state: State) -> Option<Vec<u8>> {
    if let (Some(frame), false) = (state.frame, state.legacy) {
        let (ops, scale) = animation(state.glyph, frame);
        return Some(render(state.size, &ops, scale, ink(state.light)));
    }
    if !state.legacy && state.kind != Kind::NoDevice {
        let color = ink(state.light);
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
    // ColorIconForHighContrast / ColorIconForLightTheme: near-white pixels
    // take the colour, only in the left 40% for "no device" so its red X
    // stays.
    let fill = if id == SndVol::NoDevice { 0.4 } else { 1.0 };
    if let Some(color) = state.contrast {
        recolor(&mut pixels, state.size, fill, color);
    } else if state.light {
        recolor(&mut pixels, state.size, fill, [0, 0, 0]);
    }
    Some(pixels)
}

/// The generated icons' colour: near-black on a light taskbar, white on a
/// dark one.
fn ink(light: bool) -> [u8; 3] {
    if light { [20, 20, 20] } else { [255, 255, 255] }
}

/// One row of icons as an RGBA image (for `--render`); all states share
/// the first one's size.
pub fn sheet(states: &[State]) -> Option<(u32, u32, Vec<u8>)> {
    let size = states.first()?.size;
    let columns = states.len() as u32;
    let mut sheet = vec![0u8; (size * columns * size * 4) as usize];
    for (column, state) in states.iter().enumerate() {
        let Some(pixels) = pixels(*state) else {
            continue;
        };
        for y in 0..size {
            for x in 0..size {
                let from = ((y * size + x) * 4) as usize;
                let to = ((y * size * columns + column as u32 * size + x) * 4) as usize;
                // BGRA to RGBA.
                sheet[to] = pixels[from + 2];
                sheet[to + 1] = pixels[from + 1];
                sheet[to + 2] = pixels[from];
                sheet[to + 3] = pixels[from + 3];
            }
        }
    }
    Some((size * columns, size, sheet))
}

/// IconHelper.ColorIcon: pixels whose red is above 220 take `color` (RGB)
/// and keep their alpha.
fn recolor(pixels: &mut [u8], size: u32, fill: f32, color: [u8; 3]) {
    let columns = (size as f32 * fill).ceil() as usize;
    for (i, p) in pixels.chunks_exact_mut(4).enumerate() {
        // BGRA: red is byte 2.
        if i % size as usize >= columns || p[2] <= 220 {
            continue;
        }
        p[0] = color[2];
        p[1] = color[1];
        p[2] = color[0];
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

/// One animation pose and its glyph scale (GetFrame).
fn animation(glyph: Glyph, frame: u16) -> (Vec<Op>, f32) {
    let frame = canonical(glyph, frame);
    match glyph {
        Glyph::Speaker => (
            speaker(wave_opacity(frame, WAVE1), wave_opacity(frame, WAVE2)),
            1.0,
        ),
        Glyph::Headphones => {
            let mut ops = device(glyph, false);
            ops.push(scope(frame));
            (ops, 0.9)
        }
        Glyph::Monitor => {
            let mut ops = device(glyph, false);
            ops.extend(equalizer(frame));
            (ops, 0.9)
        }
    }
}

/// DrawScope: three wave cycles scrolling between the headphone cups per
/// loop, the amplitude breathing with a music-like level, the ends tapered.
fn scope(frame: u16) -> Op {
    use std::f64::consts::PI;
    let looped = f64::from(frame) / f64::from(HEADPHONES_FRAMES) * PI * 2.0;
    let level = 0.55
        + 0.25 * (looped * 3.0 + 1.1).sin()
        + 0.15 * (looped * 7.0 + 1.87).sin()
        + 0.08 * (looped * 13.0 + 0.66).sin();
    let amplitude = 2.4 * (0.6 + 0.4 * level.clamp(0.0, 1.0));
    let phase = looped * 3.0;
    let mut points = vec![];
    let mut x = 9.2f32;
    while x <= 14.8 + 0.001 {
        let u = (f64::from(x) - 9.2) / 5.6;
        let y = 14.6 - amplitude * (u * PI).sin() * (u * PI * 3.0 - phase).sin();
        points.push((x, y as f32));
        x += 0.35;
    }
    paint(Shape::Lines(points, 1.6))
}

/// DrawEqualizer: four phase-shifted bars, one sine cycle per loop.
fn equalizer(frame: u16) -> Vec<Op> {
    let phases = [0.0, 2.1, 4.2, 1.0];
    let xs = [7.5f32, 10.5, 13.5, 16.5];
    let t = f64::from(frame) / f64::from(MONITOR_FRAMES) * std::f64::consts::PI * 2.0;
    xs.iter()
        .zip(phases)
        .map(|(&x, phase)| {
            let height = 1.0 + 5.0 * (0.5 + 0.5 * (t + phase).sin()) as f32;
            paint(Shape::Lines(vec![(x, 13.4), (x, 13.4 - height)], 1.8))
        })
        .collect()
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
    fn animation_frames_and_poses() {
        assert_eq!(frame_count(Glyph::Speaker), 16);
        assert_eq!(frame_count(Glyph::Headphones), 48);
        // Both waves are full from frame 10 on.
        assert_eq!(canonical(Glyph::Speaker, 9), 9);
        assert_eq!(canonical(Glyph::Speaker, 10), 10);
        assert_eq!(canonical(Glyph::Speaker, 15), 10);
        assert_eq!(canonical(Glyph::Monitor, 7), 6);
        assert_eq!(canonical(Glyph::Headphones, 47), 46);
        let base = State {
            kind: Kind::Muted,
            glyph: Glyph::Speaker,
            light: false,
            legacy: false,
            size: 24,
            frame: None,
            contrast: None,
        };
        assert!(can_animate(&base));
        let headphones = State {
            glyph: Glyph::Headphones,
            ..base
        };
        assert!(!can_animate(&headphones));
        assert!(!can_animate(&State {
            legacy: true,
            ..base
        }));
        // Frame 0 has no waves, frame 10 both.
        let first = pixels(State {
            frame: Some(0),
            ..base
        })
        .unwrap();
        let full = pixels(State {
            frame: Some(10),
            ..base
        })
        .unwrap();
        assert_eq!(alpha_at(&first, 24, 15, 12), 0);
        assert!(alpha_at(&full, 24, 15, 12) > 200);
    }

    #[test]
    fn recolors_for_high_contrast() {
        let mut pixels = vec![255, 255, 255, 128, 10, 10, 10, 255];
        recolor(&mut pixels, 2, 1.0, [1, 2, 3]);
        assert_eq!(pixels, [3, 2, 1, 128, 10, 10, 10, 255]);
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
