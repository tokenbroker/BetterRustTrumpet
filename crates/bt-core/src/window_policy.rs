//! Portable WindowSizePolicy.cs and DefaultDeviceChangePolicy.cs behavior.
use crate::text::{is_blank, ordinal_ignore_case_optional};

pub const MIN_WIDTH: f64 = 360.0;
pub const MIN_HEIGHT: f64 = 200.0;
pub const MAX_WIDTH: f64 = 4096.0;
pub const MAX_HEIGHT: f64 = 4096.0;
pub const SMALL_DEVICE_COUNT_LIMIT: i32 = 3;

pub fn should_restore_user_size(device_count: i32) -> bool {
    device_count > SMALL_DEVICE_COUNT_LIMIT
}
pub fn normalize_size(width: f64, height: f64) -> Option<(f64, f64)> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some((
        width.clamp(MIN_WIDTH, MAX_WIDTH),
        height.clamp(MIN_HEIGHT, MAX_HEIGHT),
    ))
}
pub fn should_notify_default_change(
    previous: Option<&str>,
    new: Option<&str>,
    enabled: bool,
) -> bool {
    enabled && !is_blank(previous) && !is_blank(new) && !ordinal_ignore_case_optional(previous, new)
}
