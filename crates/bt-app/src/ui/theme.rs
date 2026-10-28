//! The flyout's colours for the system theme (App.xaml): dark, light
//! (SystemUsesLightTheme) or accent ("Show accent colour on Start and
//! taskbar"), plus the user's appearance and behaviour settings, including
//! named themes (ActiveThemeName, CustomThemesJson).
//!
//! High contrast (SPI_GETHIGHCONTRAST) replaces all of that with Windows'
//! system colours, as the original's `HighContrast=` brush branches do;
//! every window takes its high-contrast colours from [`HighContrast`].
use crate::store::{self, Store};
use bt_app::win32::{clock::WindowsZone, gfx::Rgba, theme};
use bt_core::{
    colors::{self, Color, NumberCulture},
    settings::{Key, Value},
    themes::{self, BUILTIN_THEMES, Channel},
};

/// Windows' high-contrast system colours (WPF SystemColors names).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HighContrast {
    pub window: Rgba,
    pub window_text: Rgba,
    pub highlight: Rgba,
    pub highlight_text: Rgba,
    pub control_text: Rgba,
    pub gray_text: Rgba,
    pub hot_track: Rgba,
    pub menu: Rgba,
    pub menu_text: Rgba,
    pub window_frame: Rgba,
}

fn rgb([r, g, b]: [u8; 3]) -> Rgba {
    Rgba::rgb(r, g, b)
}

fn bytes(color: Rgba) -> [u8; 3] {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(color.r), byte(color.g), byte(color.b)]
}

impl HighContrast {
    /// The system colours while high contrast is on (re-read on
    /// WM_SETTINGCHANGE).
    pub fn current() -> Option<Self> {
        theme::high_contrast().map(|c| Self {
            window: rgb(c.window),
            window_text: rgb(c.window_text),
            highlight: rgb(c.highlight),
            highlight_text: rgb(c.highlight_text),
            control_text: rgb(c.control_text),
            gray_text: rgb(c.gray_text),
            hot_track: rgb(c.hot_track),
            menu: rgb(c.menu),
            menu_text: rgb(c.menu_text),
            window_frame: rgb(c.window_frame),
        })
    }

    /// Windows 11's "Night sky" contrast theme, for `--render`.
    pub fn sample() -> Self {
        Self {
            window: Rgba::rgb(0, 0, 0),
            window_text: Rgba::rgb(255, 255, 255),
            highlight: Rgba::rgb(0x8e, 0xe3, 0xf0),
            highlight_text: Rgba::rgb(0x26, 0x3b, 0x50),
            control_text: Rgba::rgb(255, 255, 255),
            gray_text: Rgba::rgb(0xa6, 0xa6, 0xa6),
            hot_track: Rgba::rgb(0x75, 0xe9, 0xfc),
            menu: Rgba::rgb(0, 0, 0),
            menu_text: Rgba::rgb(255, 255, 255),
            window_frame: Rgba::rgb(255, 255, 255),
        }
    }

    /// A light Window colour: what the original's fallback to `Light=`
    /// brushes assumes for anything without a high-contrast branch.
    pub fn light(&self) -> bool {
        let w = self.window;
        0.2126 * w.r + 0.7152 * w.g + 0.0722 * w.b > 0.5
    }

    /// ColorIconForHighContrast: WindowText, HighlightText under the
    /// pointer.
    pub fn tray_icon(&self, hover: bool) -> [u8; 3] {
        bytes(if hover {
            self.highlight_text
        } else {
            self.window_text
        })
    }

    /// The highlight behind a hovered row whose text keeps its colour (the
    /// original also swaps the text to HighlightText, which these rows do
    /// not do): Highlight, partly see-through so WindowText stays legible.
    pub fn hover(&self) -> Rgba {
        self.highlight.alpha(0.35)
    }
}

/// UseCustomSliderColors. Each colour is its setting, else the active
/// named theme's channel, else the original's default (slider colours) or
/// no override (window background, text, glow).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Custom {
    pub thumb: Rgba,
    pub fill: Rgba,
    pub track: Rgba,
    pub peak: Rgba,
    pub window: Option<Rgba>,
    pub text: Option<Rgba>,
    pub glow: Option<Rgba>,
}

/// The colour setting behind each theme channel.
pub const CHANNEL_KEYS: [(Channel, &str); 7] = [
    (Channel::Thumb, "SliderThumbColor"),
    (Channel::Fill, "SliderTrackFillColor"),
    (Channel::Background, "SliderTrackBackgroundColor"),
    (Channel::Peak, "PeakMeterColor"),
    (Channel::WindowBackground, "WindowBackgroundColor"),
    (Channel::Text, "TextColor"),
    (Channel::Glow, "AccentGlowColor"),
];

/// The original's FlyoutContentTintOpacity: the content layer over the
/// acrylic with a custom window background.
const CONTENT_TINT: f32 = 0.2;

/// Saved custom themes (CustomThemesJson); invalid entries are skipped.
pub fn custom_themes(store: &Store) -> Vec<themes::Theme> {
    match store.get(Key::CustomThemesJson) {
        Value::Text(Some(json)) => themes::Theme::load_custom(&json, &WindowsZone),
        _ => vec![],
    }
}

/// The theme named by ActiveThemeName: a built-in palette, then a saved
/// custom theme.
pub fn active_theme(store: &Store) -> Option<themes::Theme> {
    let Value::Text(Some(name)) = store.get(Key::ActiveThemeName) else {
        return None;
    };
    if name.is_empty() {
        return None;
    }
    BUILTIN_THEMES
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.to_theme())
        .or_else(|| {
            custom_themes(store)
                .into_iter()
                .find(|t| t.name.as_deref() == Some(name.as_str()))
        })
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
    /// The flyout's content layer over the acrylic: all but invisible (it
    /// keeps the window clickable), or a custom window background tint.
    pub flyout_tint: Rgba,
    pub custom: Option<Custom>,
    /// The DWM acrylic tint (ABGR), `None` with transparency effects off.
    pub acrylic: Option<u32>,
    pub icon_font: &'static str,
    /// Windows' high contrast is on: its system colours.
    pub hc: Option<HighContrast>,
    // Behaviour settings.
    pub peak_fps: i32,
    pub peak_style: i32,
    pub tick_sound: bool,
    pub smooth_volume: bool,
    pub animation_speed: f64,
    pub show_tooltips: bool,
}

pub fn setting_color(store: &Store, key: &str) -> Option<Rgba> {
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
        let custom = store.bool(Key::UseCustomSliderColors).then(|| {
            let named = active_theme(store);
            let channel = |channel: Channel| {
                let key = CHANNEL_KEYS[channel as usize].1;
                setting_color(store, key).or_else(|| {
                    named.as_ref().map(|t| {
                        let c = t.colors[channel as usize];
                        Rgba::rgb(c.0, c.1, c.2)
                    })
                })
            };
            Custom {
                thumb: channel(Channel::Thumb).unwrap_or(default_blue),
                fill: channel(Channel::Fill).unwrap_or(default_blue),
                track: channel(Channel::Background).unwrap_or(Rgba::rgb(80, 80, 80)),
                peak: channel(Channel::Peak).unwrap_or(default_blue),
                window: channel(Channel::WindowBackground),
                text: channel(Channel::Text),
                glow: channel(Channel::Glow),
            }
        });
        let window = custom.and_then(|c| c.window);
        let opacity = store.float(Key::WindowBackgroundOpacity) as f32;
        // AcrylicColor_Flyout: accent mode SystemAccentDark1 at 0.8, light
        // LightChromeMedium (#E6E6E6) at 0.8, dark #000 at 0.7; a custom
        // window background at WindowBackgroundOpacity.
        let acrylic = transparency.then(|| {
            if let Some(bg) = window {
                abgr(bg, opacity)
            } else if accent_mode {
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
        // ApplyExtendedThemeColors: Text, Background and PopupBackground.
        let text = custom.and_then(|c| c.text).unwrap_or(text);
        if let Some(bg) = window {
            popup_bg = bg;
        }
        // FlyoutThemeTrackRightBackground: accent mode, light, dark.
        let track_rest = if accent_mode {
            Rgba::rgba(255, 255, 255, 0.5)
        } else if light {
            Rgba::rgba(0, 0, 0, 0.224)
        } else {
            Rgba::rgba(255, 255, 255, 0.224)
        };
        let opaque_bg = if let Some(bg) = window {
            bg
        } else if accent_mode {
            accent_dark1
        } else if light {
            Rgba::rgb(242, 242, 242)
        } else {
            Rgba::rgb(23, 23, 23)
        };
        let theme = Self {
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
            flyout_tint: window.map_or(Rgba::rgba(0, 0, 0, 1.0 / 255.0), |bg| {
                bg.alpha(CONTENT_TINT)
            }),
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
            hc: None,
        };
        match HighContrast::current() {
            Some(hc) => theme.high_contrast(hc),
            None => theme,
        }
    }

    /// The high-contrast look: no acrylic or accent colouring
    /// (BrushValueParser turns Transparency and UseAccentColor off), the
    /// custom colours set aside (OverrideRef keeps high contrast alone), and
    /// the system colours: Text and backgrounds WindowText on Window, list
    /// hover Highlight, the slider thumb and track ControlText, peaks
    /// HotTrack. The track fill stays SystemAccent, as in the original.
    pub fn high_contrast(self, hc: HighContrast) -> Self {
        Self {
            transparency: false,
            light: hc.light(),
            accent_mode: false,
            text: hc.window_text,
            hover: hc.hover(),
            pressed: hc.highlight.alpha(0.5),
            popup_bg: hc.window,
            dismiss: hc.window.alpha(0.72),
            track_rest: hc.control_text,
            opaque_bg: hc.window,
            flyout_tint: hc.window,
            custom: None,
            acrylic: None,
            hc: Some(hc),
            ..self
        }
    }

    pub fn thumb(&self) -> Rgba {
        match self.hc {
            Some(hc) => hc.control_text,
            None => self.custom.map_or(self.accent, |c| c.thumb),
        }
    }
    pub fn fill(&self) -> Rgba {
        self.custom.map_or(self.accent, |c| c.fill)
    }
    pub fn track(&self) -> Rgba {
        self.custom.map_or(self.track_rest, |c| c.track)
    }
    pub fn peak(&self) -> Rgba {
        match self.hc {
            Some(hc) => hc.hot_track,
            None => self.custom.map_or(self.accent, |c| c.peak),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_contrast_uses_the_system_colours() {
        let hc = HighContrast::sample();
        assert!(!hc.light());
        assert_eq!(hc.tray_icon(false), [255, 255, 255]);
        assert_eq!(hc.tray_icon(true), [0x26, 0x3b, 0x50]);
        let white = HighContrast {
            window: Rgba::WHITE,
            ..hc
        };
        assert!(white.light());
    }
}
