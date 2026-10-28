//! The flyout's colours for the system theme (App.xaml): dark, light
//! (SystemUsesLightTheme) or accent ("Show accent colour on Start and
//! taskbar"), plus the user's appearance and behaviour settings.
use crate::store::{self, Store};
use bt_app::win32::{gfx::Rgba, theme};
use bt_core::{
    colors::{self, Color, NumberCulture},
    settings::Key,
};

/// UseCustomSliderColors; each colour falls back to the original's
/// default when unset (Transparent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Custom {
    pub thumb: Rgba,
    pub fill: Rgba,
    pub track: Rgba,
    pub peak: Rgba,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub accent: Rgba,
    pub accent_dark1: Rgba,
    pub transparency: bool,
    pub light: bool,
    pub accent_mode: bool,
    pub text: Rgba,
    pub hover: Rgba,
    pub pressed: Rgba,
    pub popup_bg: Rgba,
    pub dismiss: Rgba,
    pub track_rest: Rgba,
    /// The device block and popup header (accent at 0.3, 0.5 when opaque).
    pub accent_30: Rgba,
    pub accent_22: Rgba,
    /// Solid flyout background with transparency effects off.
    pub opaque_bg: Rgba,
    pub custom: Option<Custom>,
    /// The DWM acrylic tint (ABGR), `None` with transparency effects off.
    pub acrylic: Option<u32>,
    pub icon_font: &'static str,
    // Behaviour settings.
    pub peak_fps: i32,
    pub peak_style: i32,
    pub tick_sound: bool,
    pub smooth_volume: bool,
    pub animation_speed: f64,
    pub show_tooltips: bool,
}

fn setting_color(store: &Store, key: &str) -> Option<Rgba> {
    let c = colors::parse_setting(store.text(key).as_deref(), &NumberCulture::INVARIANT);
    let t = Color::TRANSPARENT;
    if (c.a, c.r, c.g, c.b) == (t.a, t.r, t.g, t.b) {
        return None;
    }
    Some(Rgba::rgba(c.r, c.g, c.b, f32::from(c.a) / 255.0))
}

fn abgr(color: Rgba, alpha: f32) -> u32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(alpha) << 24) | (byte(color.b) << 16) | (byte(color.g) << 8) | byte(color.r)
}

impl Theme {
    pub fn load(store: &Store, icon_font: &'static str) -> Self {
        let accent = Rgba::hex(&theme::accent()).unwrap_or(Rgba::rgb(0, 120, 215));
        let accent_dark1 = Rgba::hex(&theme::accent_dark1()).unwrap_or(Rgba::rgb(0, 90, 158));
        let transparency = theme::transparency();
        let light = theme::system_light();
        let accent_mode = theme::accent_on_taskbar() && !light;
        let default_blue = Rgba::rgb(0, 120, 215);
        let custom = store.bool(Key::UseCustomSliderColors).then(|| Custom {
            thumb: setting_color(store, "SliderThumbColor").unwrap_or(default_blue),
            fill: setting_color(store, "SliderTrackFillColor").unwrap_or(default_blue),
            track: setting_color(store, "SliderTrackBackgroundColor")
                .unwrap_or(Rgba::rgb(80, 80, 80)),
            peak: setting_color(store, "PeakMeterColor").unwrap_or(default_blue),
        });
        // AcrylicColor_Flyout: accent mode SystemAccentDark1 at 0.8, light
        // LightChromeMedium (#E6E6E6) at 0.8, dark #000 at 0.7.
        let acrylic = transparency.then(|| {
            if accent_mode {
                abgr(accent_dark1, 0.8)
            } else if light {
                abgr(Rgba::rgb(230, 230, 230), 0.8)
            } else {
                abgr(Rgba::BLACK, 0.7)
            }
        });
        let (text, hover, pressed, mut popup_bg, dismiss) = if light {
            (
                Rgba::BLACK,
                Rgba::rgba(0, 0, 0, 0.1),
                Rgba::rgba(0, 0, 0, 0.16),
                Rgba::WHITE,
                Rgba::rgba(255, 255, 255, 0.72),
            )
        } else {
            (
                Rgba::WHITE,
                Rgba::rgba(255, 255, 255, 0.125),
                Rgba::rgba(255, 255, 255, 0.184),
                Rgba::rgb(21, 21, 21),
                Rgba::rgba(21, 21, 21, 0.72),
            )
        };
        if accent_mode {
            popup_bg = accent_dark1;
        }
        // FlyoutThemeTrackRightBackground: accent mode, light, dark.
        let track_rest = if accent_mode {
            Rgba::rgba(255, 255, 255, 0.5)
        } else if light {
            Rgba::rgba(0, 0, 0, 0.224)
        } else {
            Rgba::rgba(255, 255, 255, 0.224)
        };
        let opaque_bg = if accent_mode {
            accent_dark1
        } else if light {
            Rgba::rgb(242, 242, 242)
        } else {
            Rgba::rgb(23, 23, 23)
        };
        Self {
            accent,
            accent_dark1,
            transparency,
            light,
            accent_mode,
            text,
            hover,
            pressed,
            popup_bg,
            dismiss,
            track_rest,
            accent_30: accent.alpha(if transparency { 0.3 } else { 0.5 }),
            accent_22: accent.alpha(0.22),
            opaque_bg,
            custom,
            acrylic,
            icon_font,
            peak_fps: store
                .settings
                .effective_peak_fps(store::context())
                .clamp(1, 120),
            peak_style: store.int(Key::PeakMeterStyle),
            tick_sound: store.bool(Key::UseVolumeTickSound),
            smooth_volume: store.bool(Key::UseSmoothVolumeAnimation),
            animation_speed: store.float(Key::VolumeAnimationSpeed),
            show_tooltips: store.bool(Key::ShowAppTooltips),
        }
    }

    pub fn thumb(&self) -> Rgba {
        self.custom.map_or(self.accent, |c| c.thumb)
    }
    pub fn fill(&self) -> Rgba {
        self.custom.map_or(self.accent, |c| c.fill)
    }
    pub fn track(&self) -> Rgba {
        self.custom.map_or(self.track_rest, |c| c.track)
    }
    pub fn peak(&self) -> Rgba {
        self.custom.map_or(self.accent, |c| c.peak)
    }
}
