//! ColorTheme.cs file format and the exact ThemeRegistry.cs palette values.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const WHITE: Self = Self(255, 255, 255);
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }
    pub fn parse_legacy_hex(value: &str) -> Result<Self, ThemeError> {
        let text = value.trim_start_matches('#');
        if text.encode_utf16().count() != 6 {
            return Ok(Self::WHITE);
        }
        if text.len() != 6 || !text.is_ascii() {
            return Err(ThemeError::InvalidColor(value.into()));
        }
        let channel = |start| {
            u8::from_str_radix(&text[start..start + 2], 16)
                .map_err(|_| ThemeError::InvalidColor(value.into()))
        };
        Ok(Self(channel(0)?, channel(2)?, channel(4)?))
    }
}

#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("invalid theme JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing theme color: {0}")]
    MissingColor(&'static str),
    #[error("invalid six-digit theme color: {0}")]
    InvalidColor(String),
    #[error("invalid theme opacity")]
    InvalidOpacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Channel {
    Thumb,
    Fill,
    Background,
    Peak,
    WindowBackground,
    Text,
    Glow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: Option<String>,
    pub category: String,
    pub colors: [Rgb; 7],
    pub opacity: Option<f64>,
    pub is_custom: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ThemeFile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    thumb: Option<String>,
    #[serde(default)]
    fill: Option<String>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    peak: Option<String>,
    #[serde(default, rename = "WindowBg")]
    window_bg: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    glow: Option<String>,
    #[serde(default)]
    opacity: Option<Value>,
}

impl Theme {
    pub fn from_json(json: &str) -> Result<Self, ThemeError> {
        let file: ThemeFile = serde_json::from_str(json)?;
        let required = |value: Option<&str>, key| {
            value
                .ok_or(ThemeError::MissingColor(key))
                .and_then(Rgb::parse_legacy_hex)
        };
        let opacity = file
            .opacity
            .map(|value| {
                let number = match value {
                    Value::Number(n) => n.as_f64(),
                    Value::String(s) => s.trim().parse::<f64>().ok(),
                    Value::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
                    _ => None,
                }
                .ok_or(ThemeError::InvalidOpacity)?;
                Ok::<_, ThemeError>(number.clamp(0.05, 1.0))
            })
            .transpose()?;
        let thumb = required(file.thumb.as_deref(), "Thumb")?;
        let fill = required(file.fill.as_deref(), "Fill")?;
        Ok(Self {
            name: file.name,
            category: file.category.unwrap_or_else(|| "Custom".into()),
            colors: [
                thumb,
                fill,
                required(file.background.as_deref(), "Background")?,
                required(file.peak.as_deref(), "Peak")?,
                Rgb::parse_legacy_hex(file.window_bg.as_deref().unwrap_or("#000000"))?,
                Rgb::parse_legacy_hex(file.text.as_deref().unwrap_or("#FFFFFF"))?,
                file.glow
                    .as_deref()
                    .map(Rgb::parse_legacy_hex)
                    .transpose()?
                    .unwrap_or(fill),
            ],
            opacity,
            is_custom: true,
        })
    }

    pub fn to_json(&self) -> Result<String, ThemeError> {
        let color = |channel: Channel| Some(self.colors[channel as usize].to_hex());
        let opacity = self.opacity.map(|n| {
            if n.is_nan() {
                Value::String("NaN".into())
            } else if n == f64::INFINITY {
                Value::String("Infinity".into())
            } else if n == f64::NEG_INFINITY {
                Value::String("-Infinity".into())
            } else {
                serde_json::json!(n)
            }
        });
        Ok(serde_json::to_string_pretty(&ThemeFile {
            name: self.name.clone(),
            category: Some(self.category.clone()),
            thumb: color(Channel::Thumb),
            fill: color(Channel::Fill),
            background: color(Channel::Background),
            peak: color(Channel::Peak),
            window_bg: color(Channel::WindowBackground),
            text: color(Channel::Text),
            glow: color(Channel::Glow),
            opacity,
        })?)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BuiltinTheme {
    pub name: &'static str,
    pub category: &'static str,
    pub colors: [Rgb; 7],
    pub opacity: Option<f64>,
}

impl BuiltinTheme {
    pub fn to_theme(self) -> Theme {
        Theme {
            name: Some(self.name.into()),
            category: self.category.into(),
            colors: self.colors,
            opacity: self.opacity,
            is_custom: false,
        }
    }
}

pub const CATEGORIES: [&str; 5] = ["Minimal", "Apps", "Editors", "Atmosphere", "Accessibility"];
pub const BUILTIN_THEMES: [BuiltinTheme; 12] = [
    BuiltinTheme {
        name: "Default (Windows Accent)",
        category: "Minimal",
        colors: [
            Rgb(0, 120, 215),
            Rgb(0, 120, 215),
            Rgb(80, 80, 80),
            Rgb(0, 120, 215),
            Rgb(32, 32, 32),
            Rgb(255, 255, 255),
            Rgb(0, 120, 215),
        ],
        opacity: None,
    },
    BuiltinTheme {
        name: "Midnight Studio",
        category: "Minimal",
        colors: [
            Rgb(92, 145, 238),
            Rgb(58, 111, 206),
            Rgb(28, 31, 39),
            Rgb(242, 181, 86),
            Rgb(16, 18, 24),
            Rgb(242, 244, 248),
            Rgb(104, 158, 248),
        ],
        opacity: Some(0.45),
    },
    BuiltinTheme {
        name: "Graphite",
        category: "Minimal",
        colors: [
            Rgb(222, 174, 112),
            Rgb(174, 123, 76),
            Rgb(39, 40, 45),
            Rgb(111, 194, 165),
            Rgb(22, 23, 27),
            Rgb(238, 239, 234),
            Rgb(231, 181, 117),
        ],
        opacity: Some(0.75),
    },
    BuiltinTheme {
        name: "Spotify",
        category: "Apps",
        colors: [
            Rgb(30, 215, 96),
            Rgb(25, 175, 80),
            Rgb(40, 40, 40),
            Rgb(255, 120, 100),
            Rgb(18, 18, 18),
            Rgb(240, 240, 240),
            Rgb(30, 215, 96),
        ],
        opacity: Some(0.85),
    },
    BuiltinTheme {
        name: "Discord",
        category: "Apps",
        colors: [
            Rgb(88, 101, 242),
            Rgb(87, 242, 135),
            Rgb(30, 31, 34),
            Rgb(240, 178, 50),
            Rgb(43, 45, 49),
            Rgb(227, 229, 232),
            Rgb(88, 101, 242),
        ],
        opacity: Some(0.9),
    },
    BuiltinTheme {
        name: "Dracula",
        category: "Editors",
        colors: [
            Rgb(189, 147, 249),
            Rgb(255, 121, 198),
            Rgb(33, 34, 44),
            Rgb(139, 233, 253),
            Rgb(40, 42, 54),
            Rgb(248, 248, 242),
            Rgb(189, 147, 249),
        ],
        opacity: Some(0.75),
    },
    BuiltinTheme {
        name: "Catppuccin",
        category: "Editors",
        colors: [
            Rgb(203, 166, 247),
            Rgb(137, 180, 250),
            Rgb(24, 24, 37),
            Rgb(166, 227, 161),
            Rgb(30, 30, 46),
            Rgb(205, 214, 244),
            Rgb(245, 194, 231),
        ],
        opacity: Some(0.7),
    },
    BuiltinTheme {
        name: "Nord",
        category: "Editors",
        colors: [
            Rgb(136, 192, 208),
            Rgb(163, 190, 140),
            Rgb(36, 41, 51),
            Rgb(208, 135, 112),
            Rgb(46, 52, 64),
            Rgb(216, 222, 233),
            Rgb(136, 192, 208),
        ],
        opacity: Some(0.65),
    },
    BuiltinTheme {
        name: "Synthwave",
        category: "Atmosphere",
        colors: [
            Rgb(255, 16, 128),
            Rgb(0, 255, 255),
            Rgb(25, 5, 45),
            Rgb(255, 160, 0),
            Rgb(15, 2, 30),
            Rgb(255, 130, 220),
            Rgb(180, 0, 255),
        ],
        opacity: Some(0.5),
    },
    BuiltinTheme {
        name: "Aurora",
        category: "Atmosphere",
        colors: [
            Rgb(0, 255, 170),
            Rgb(120, 60, 220),
            Rgb(8, 15, 25),
            Rgb(255, 140, 180),
            Rgb(5, 10, 18),
            Rgb(190, 240, 220),
            Rgb(0, 220, 150),
        ],
        opacity: Some(0.45),
    },
    BuiltinTheme {
        name: "High Contrast",
        category: "Accessibility",
        colors: [
            Rgb(255, 255, 255),
            Rgb(255, 255, 0),
            Rgb(0, 0, 0),
            Rgb(0, 255, 255),
            Rgb(0, 0, 0),
            Rgb(255, 255, 255),
            Rgb(255, 255, 0),
        ],
        opacity: Some(1.0),
    },
    BuiltinTheme {
        name: "Color Blind Safe",
        category: "Accessibility",
        colors: [
            Rgb(0, 114, 178),
            Rgb(230, 159, 0),
            Rgb(15, 15, 18),
            Rgb(204, 121, 167),
            Rgb(16, 16, 20),
            Rgb(220, 220, 230),
            Rgb(86, 180, 233),
        ],
        opacity: Some(0.95),
    },
];
