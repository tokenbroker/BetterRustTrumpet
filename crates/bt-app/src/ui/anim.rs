//! Timing curves for the original's storyboards (as the CSS cubic-bezier
//! equivalents the web preview used) and a small tween helper.
use std::time::{Duration, Instant};

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function.
#[derive(Clone, Copy, Debug)]
pub struct Curve(pub f32, pub f32, pub f32, pub f32);

/// CubicEase EaseOut.
pub const EASE_OUT: Curve = Curve(0.215, 0.61, 0.355, 1.0);
/// CubicEase EaseIn (the flyout's close).
pub const EASE_IN: Curve = Curve(0.32, 0.0, 0.67, 0.0);
/// The media popup's slide-out.
pub const EASE_IN_SHARP: Curve = Curve(0.55, 0.055, 0.675, 0.19);
/// BackEase (Amplitude 0.35) for the flyout's open scale.
pub const BACK_OUT: Curve = Curve(0.3, 1.3, 0.6, 1.0);
/// QuadraticEase EaseOut-ish fade (the open fade).
pub const FADE_OUT_QUAD: Curve = Curve(0.5, 1.0, 0.89, 1.0);
/// The popup's rise and grow (BackEase).
pub const RISE: Curve = Curve(0.34, 1.3, 0.64, 1.0);
pub const GROW: Curve = Curve(0.34, 1.4, 0.64, 1.0);

impl Curve {
    fn sample(a: f32, b: f32, t: f32) -> f32 {
        // B(t) for control values a, b with endpoints 0 and 1.
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    }
    /// Eased progress for linear progress `x` (0..1).
    pub fn at(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.0 || x >= 1.0 {
            return x;
        }
        // Solve x(t) = x by bisection (monotonic in t for x1, x2 in 0..1).
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        let mut t = x;
        for _ in 0..24 {
            let v = Self::sample(self.0, self.2, t);
            if (v - x).abs() < 1e-5 {
                break;
            }
            if v < x {
                lo = t;
            } else {
                hi = t;
            }
            t = (lo + hi) / 2.0;
        }
        Self::sample(self.1, self.3, t)
    }
}

/// Linear progress of an animation that started at `start` and lasts
/// `ms`, after an optional delay; 1 when finished.
pub fn progress(start: Instant, delay_ms: f32, ms: f32, now: Instant) -> f32 {
    let elapsed = now.saturating_duration_since(start).as_secs_f32() * 1000.0 - delay_ms;
    (elapsed / ms).clamp(0.0, 1.0)
}

/// Whether an animation of `ms` (plus delay) begun at `start` is running.
pub fn running(start: Option<Instant>, ms: f32, now: Instant) -> bool {
    start.is_some_and(|s| now.saturating_duration_since(s) < Duration::from_secs_f32(ms / 1000.0))
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_hit_their_ends_and_overshoot() {
        assert_eq!(EASE_OUT.at(0.0), 0.0);
        assert_eq!(EASE_OUT.at(1.0), 1.0);
        assert!(EASE_OUT.at(0.5) > 0.5);
        assert!(EASE_IN.at(0.5) < 0.5);
        let peak = (1..100)
            .map(|i| BACK_OUT.at(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.0, "back ease overshoots");
    }
}
