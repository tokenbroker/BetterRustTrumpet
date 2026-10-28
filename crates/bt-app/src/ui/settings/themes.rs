//! Theme actions from the Appearance page, after the original's
//! EarTrumpetColorsSettingsPageViewModel: a theme is applied by writing
//! its seven colours (and its opacity, if it has one) to the colour
//! settings, turning custom colours on and naming it in ActiveThemeName.
//! The original animates the change over 300 ms; here it is immediate.
use super::{super::theme, pages};
use crate::core::Core;
use bt_app::win32::gfx::Rgba;
use bt_core::{
    colors::{Color, NumberCulture},
    settings::{Key, Value},
    themes::{BUILTIN_THEMES, Rgb, Theme},
};

/// WPF `Color.ToString()` of an opaque colour: `#FFRRGGBB`.
pub fn color_text(color: Rgba) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let argb = 0xff00_0000
        | (u32::from(byte(color.r)) << 16)
        | (u32::from(byte(color.g)) << 8)
        | u32::from(byte(color.b));
    Color::from_argb(argb).to_text(&NumberCulture::INVARIANT)
}

/// `Colors.Transparent.ToString()`: an unset colour.
const TRANSPARENT: &str = "#00FFFFFF";

fn set_name(core: &mut Core, name: &str) {
    core.store
        .set(Key::ActiveThemeName, Value::Text(Some(name.into())));
}

/// Changes one colour channel; editing implies custom colours.
pub fn set_color(core: &mut Core, key: &str, color: Rgba) {
    core.store.set_text(key, &color_text(color));
    if !core.store.bool(Key::UseCustomSliderColors) {
        core.store
            .set(Key::UseCustomSliderColors, Value::Bool(true));
    }
}

fn apply(core: &mut Core, theme: &Theme) {
    core.store
        .set(Key::UseCustomSliderColors, Value::Bool(true));
    // Selecting a preset turns the dynamic album art colours off.
    if core.store.bool(Key::UseDynamicAlbumArtTheme) {
        core.store
            .set(Key::UseDynamicAlbumArtTheme, Value::Bool(false));
    }
    if let Some(opacity) = theme.opacity {
        core.store
            .set(Key::WindowBackgroundOpacity, Value::Float(opacity));
    }
    for (i, (_, key)) in theme::CHANNEL_KEYS.iter().enumerate() {
        let Rgb(r, g, b) = theme.colors[i];
        core.store.set_text(key, &color_text(Rgba::rgb(r, g, b)));
    }
}

/// A built-in or saved theme by name.
pub fn select(core: &mut Core, name: &str) {
    let found = BUILTIN_THEMES
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.to_theme())
        .or_else(|| {
            theme::custom_themes(&core.store)
                .into_iter()
                .find(|t| t.name.as_deref() == Some(name))
        });
    if let Some(found) = found {
        apply(core, &found);
        set_name(core, name);
    }
}

/// A small xorshift seeded from the clock (no extra dependency).
struct Random(u64);

impl Random {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9e37_79b9, |d| d.as_nanos() as u64);
        Self(seed | 1)
    }
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub fn hsv(h: f64, s: f64, v: f64) -> Rgb {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let byte = |c: f64| (c.clamp(0.0, 1.0) * 255.0) as u8;
    Rgb(byte(r), byte(g), byte(b))
}

/// RandomizeColors: a coherent palette around a random hue.
pub fn randomize(core: &mut Core) {
    let mut rng = Random::new();
    let base_hue = rng.next();
    let base_sat = 0.5 + rng.next() * 0.4;
    let base_val = 0.6 + rng.next() * 0.35;
    let thumb = hsv(base_hue, base_sat, base_val);
    let fill_hue = (base_hue + 0.05 + rng.next() * 0.1) % 1.0;
    let fill = hsv(fill_hue, base_sat * 0.85, base_val * 0.9);
    let track = hsv(base_hue, base_sat * 0.6, 0.08 + rng.next() * 0.08);
    let peak_hue = (base_hue + 0.4 + rng.next() * 0.2) % 1.0;
    let peak = hsv(peak_hue, 0.4 + rng.next() * 0.3, 0.5 + rng.next() * 0.3);
    let window = hsv(base_hue, base_sat * 0.5, 0.05 + rng.next() * 0.06);
    let text = hsv(
        base_hue,
        (base_sat * 0.12).max(0.05),
        0.85 + rng.next() * 0.12,
    );
    let theme = Theme {
        name: Some("Random".into()),
        category: Some("Custom".into()),
        colors: [thumb, fill, track, peak, window, text, fill],
        opacity: None,
        is_custom: false,
    };
    set_name(core, "");
    apply(core, &theme);
}

/// ResetToDefault: custom colours off, the extended colours unset and the
/// default opacity.
pub fn reset(core: &mut Core) {
    let store = &mut core.store;
    store.set(Key::UseCustomSliderColors, Value::Bool(false));
    store.set(Key::UseDynamicAlbumArtTheme, Value::Bool(false));
    store.set(Key::ActiveThemeName, Value::Text(Some(String::new())));
    for key in ["WindowBackgroundColor", "TextColor", "AccentGlowColor"] {
        store.set_text(key, TRANSPARENT);
    }
    store.set(Key::WindowBackgroundOpacity, Value::Float(0.7));
}

fn persist(core: &mut Core, themes: &[Theme]) {
    let items: Vec<String> = themes.iter().filter_map(|t| t.to_json().ok()).collect();
    let json = format!("[{}]", items.join(","));
    core.store
        .set(Key::CustomThemesJson, Value::Text(Some(json)));
}

/// SaveCustomTheme: the current colours and opacity under `name` (or "My
/// Theme n"), then selected.
pub fn save(core: &mut Core, name: &str) {
    let mut saved = theme::custom_themes(&core.store);
    let name = match name.trim() {
        "" => format!("My Theme {}", saved.len() + 1),
        name => name.to_owned(),
    };
    let colors = std::array::from_fn(|i| {
        let c = pages::channel_color(core, i).0;
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Rgb(byte(c.r), byte(c.g), byte(c.b))
    });
    saved.push(Theme {
        name: Some(name.clone()),
        category: Some("Custom".into()),
        colors,
        opacity: Some(core.store.float(Key::WindowBackgroundOpacity)),
        is_custom: true,
    });
    persist(core, &saved);
    select(core, &name);
}

/// DeleteCustomTheme; the active theme name is cleared if it was this one.
pub fn delete(core: &mut Core, name: &str) {
    let mut saved = theme::custom_themes(&core.store);
    let before = saved.len();
    saved.retain(|t| t.name.as_deref() != Some(name));
    if saved.len() == before {
        return;
    }
    persist(core, &saved);
    if matches!(core.store.get(Key::ActiveThemeName), Value::Text(Some(n)) if n == name) {
        set_name(core, "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_written_as_wpf_text() {
        assert_eq!(color_text(Rgba::rgb(0, 120, 215)), "#FF0078D7");
        assert_eq!(hsv(0.0, 1.0, 1.0), Rgb(255, 0, 0));
    }
}
