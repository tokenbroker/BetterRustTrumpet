//! The original's VolumeSlider: 32 DIPs high, a 4 DIP track (accent fill),
//! a 16 DIP thumb and the peak meter in one of five styles.
use super::{
    anim::{self, EASE_OUT},
    theme::Theme,
};
use bt_app::win32::gfx::{Canvas, Rect};
use std::time::{Duration, Instant};

/// How long external updates are ignored after the user moves a slider.
const BUSY: Duration = Duration::from_millis(500);
/// Each frame the drawn meter moves this share of the way to its target.
const PEAK_SMOOTHING: f32 = 0.35;

#[derive(Debug, Default)]
pub struct Slider {
    pub value: i32,
    busy_until: Option<Instant>,
    pressed: bool,
    dragging: bool,
    /// Track-click slide: from, to, start, duration (ms).
    slide: Option<(f32, f32, Instant, f32)>,
    peaks: [f32; 2],
    drawn: [f32; 2],
}

fn value_at(x: f32, width: f32) -> f32 {
    (x - 8.0) / (width - 16.0).max(1.0) * 100.0
}

impl Slider {
    pub fn new(value: i32) -> Self {
        Self {
            value,
            ..Self::default()
        }
    }

    fn set(&mut self, value: f32) -> Option<i32> {
        let value = value.round().clamp(0.0, 100.0) as i32;
        if value == self.value {
            return None;
        }
        self.value = value;
        Some(value)
    }

    /// A value from Windows; ignored while the user is using the slider.
    pub fn update(&mut self, value: i32, now: Instant) {
        if self.busy_until.is_some_and(|t| now < t) || self.pressed {
            return;
        }
        self.value = value.clamp(0, 100);
    }

    fn thumb_x(&self, width: f32) -> f32 {
        (width - 16.0) * self.value as f32 / 100.0
    }

    /// Pointer press at `x`, `y` within the slider's `width` x 32 box: on the
    /// thumb it starts a drag, on the track a slide there (CubicEase EaseOut,
    /// its duration from VolumeAnimationSpeed). Returns a value to apply.
    pub fn press(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        theme: &Theme,
        now: Instant,
    ) -> Option<i32> {
        self.pressed = true;
        let left = self.thumb_x(width);
        let on_thumb = x >= left && x < left + 16.0 && (8.0..24.0).contains(&y);
        self.dragging = on_thumb;
        if on_thumb {
            self.slide = None;
            return None;
        }
        let target = value_at(x, width).clamp(0.0, 100.0);
        if !theme.smooth_volume {
            return self.set(target);
        }
        let speed = theme.animation_speed.clamp(0.02, 0.5) as f32;
        let duration = 320.0 - (speed - 0.02) / 0.48 * 220.0;
        self.slide = Some((self.value as f32, target, now, duration));
        None
    }

    /// Pointer moved while pressed; dragging after a track press takes over
    /// from the slide.
    pub fn drag(&mut self, x: f32, width: f32) -> Option<i32> {
        if !self.pressed {
            return None;
        }
        self.dragging = true;
        self.slide = None;
        self.set(value_at(x, width))
    }

    pub fn release(&mut self, now: Instant) {
        self.pressed = false;
        self.dragging = false;
        self.busy_until = Some(now + BUSY);
    }

    /// Wheel or keyboard step.
    pub fn step(&mut self, delta: i32, now: Instant) -> Option<i32> {
        self.slide = None;
        self.busy_until = Some(now + BUSY);
        self.set((self.value + delta) as f32)
    }

    /// Advances a track-click slide; returns the value to apply.
    pub fn tick(&mut self, now: Instant) -> Option<i32> {
        let (from, to, start, ms) = self.slide?;
        let t = anim::progress(start, 0.0, ms, now);
        let value = anim::lerp(from, to, EASE_OUT.at(t));
        if t >= 1.0 {
            self.slide = None;
        }
        self.busy_until = Some(now + BUSY);
        self.set(value)
    }

    pub fn animating(&self) -> bool {
        self.slide.is_some()
    }

    /// Meter targets: channel peaks scaled by the volume.
    pub fn set_peaks(&mut self, channels: [f32; 2], volume: f32) {
        self.peaks = [channels[0] * volume, channels[1] * volume];
    }

    /// Moves the drawn meter towards its target. Returns whether anything is
    /// still showing.
    pub fn animate_peaks(&mut self) -> bool {
        let mut showing = false;
        for i in 0..2 {
            self.drawn[i] += (self.peaks[i] - self.drawn[i]) * PEAK_SMOOTHING;
            if self.drawn[i] < 0.001 {
                self.drawn[i] = 0.0;
            }
            showing |= self.drawn[i] > 0.0 || self.peaks[i] > 0.0;
        }
        showing
    }

    pub fn reset_peaks(&mut self) {
        self.peaks = [0.0; 2];
        self.drawn = [0.0; 2];
    }

    /// Draws the slider in `rect` (32 high) at `opacity`.
    pub fn paint(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, opacity: f32) {
        if opacity < 1.0 {
            canvas.push_layer(opacity, None);
        }
        let (x, y, w) = (rect.x, rect.y, rect.w);
        let left = self.thumb_x(w);
        let classic = theme.peak_style == 0;
        canvas.round_rect(Rect::new(x, y + 14.0, w, 4.0), 2.0, theme.track());
        let fill = theme.fill().alpha(if classic { 1.0 } else { 0.4 });
        canvas.round_rect(Rect::new(x, y + 14.0, left + 8.0, 4.0), 2.0, fill);
        canvas.circle(x + left + 8.0, y + 16.0, 8.0, theme.thumb());
        let span = (w - 16.0).max(0.0);
        let peak = theme.peak();
        match theme.peak_style {
            1 => segments(
                canvas,
                x,
                y + 14.5,
                span * self.drawn[0],
                3.0,
                3.0,
                2.0,
                peak.alpha(0.7),
            ),
            2 => segments(
                canvas,
                x,
                y + 14.0,
                span * self.drawn[0],
                4.0,
                6.0,
                2.0,
                peak.alpha(0.7),
            ),
            3 => canvas.round_rect(
                Rect::new(x, y + 15.0, span * self.drawn[0], 2.0),
                1.0,
                peak.alpha(0.55),
            ),
            4 => segments(
                canvas,
                x,
                y + 14.5,
                span * self.drawn[0],
                3.0,
                5.0,
                4.0,
                peak.alpha(0.65),
            ),
            _ => {
                for (i, top) in [12.5, 15.0].into_iter().enumerate() {
                    let width = span * self.drawn[i];
                    if width > 0.0 {
                        canvas.round_rect(Rect::new(x, y + top, width, 4.0), 2.0, peak.alpha(0.6));
                    }
                }
            }
        }
        if opacity < 1.0 {
            canvas.pop();
        }
    }
}

/// A dashed meter bar (`on` DIPs drawn, `off` skipped) up to `width`.
#[allow(clippy::too_many_arguments)]
fn segments(
    canvas: &mut Canvas,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    on: f32,
    off: f32,
    color: bt_app::win32::gfx::Rgba,
) {
    let mut at = 0.0;
    while at < width {
        canvas.rect(Rect::new(x + at, y, on.min(width - at), height), color);
        at += on + off;
    }
}
