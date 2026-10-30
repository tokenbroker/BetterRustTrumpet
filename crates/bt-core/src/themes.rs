//! ColorTheme.cs file format and the exact ThemeRegistry.cs palette values.
use crate::{
    colors::{self, NumberCulture},
    newtonsoft::{self, LocalZone, Token},
};
use serde::Serialize;
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
    #[error("invalid Newtonsoft theme value")]
    Legacy(#[from] newtonsoft::Error),
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
    pub category: Option<String>,
    pub colors: [Rgb; 7],
    pub opacity: Option<f64>,
    pub is_custom: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct ThemeFile {
    name: Option<String>,
    category: Option<String>,
    thumb: Option<String>,
    fill: Option<String>,
    background: Option<String>,
    peak: Option<String>,
    #[serde(rename = "WindowBg")]
    window_bg: Option<String>,
    text: Option<String>,
    glow: Option<String>,
    opacity: Option<Value>,
}

impl Theme {
    pub fn from_json(json: &str, zone: &dyn LocalZone) -> Result<Self, ThemeError> {
        let token = newtonsoft::parse_dynamic_document(json, zone)?.ok_or(newtonsoft::Error)?;
        Self::from_token(&token)
    }

    fn from_token(token: &Token) -> Result<Self, ThemeError> {
        let Token::Object(members) = token else {
            return Err(newtonsoft::Error.into());
        };
        // Dynamic JObject lookup is case-sensitive; explicit null is a JValue,
        // so C#'s ?? fallback only applies when the property is absent.
        let get = |name| {
            members
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
        };
        let string = |name| {
            get(name)
                .map(dynamic_string)
                .transpose()
                .map(Option::flatten)
        };
        let color = |name: &'static str, fallback: Option<&str>| {
            let text = if get(name).is_none() {
                fallback.map(str::to_owned)
            } else {
                string(name)?
            };
            text.ok_or(ThemeError::MissingColor(name))
                .and_then(|s| Rgb::parse_legacy_hex(&s))
        };
        let opacity = match get("Opacity") {
            None | Some(Token::Null | Token::Undefined) => None,
            Some(token) => Some(dynamic_double(token)?.clamp(0.05, 1.0)),
        };
        let fill_text = string("Fill")?;
        Ok(Self {
            name: string("Name")?,
            category: if get("Category").is_none() {
                Some("Custom".into())
            } else {
                string("Category")?
            },
            colors: [
                color("Thumb", None)?,
                color("Fill", None)?,
                color("Background", None)?,
                color("Peak", None)?,
                color("WindowBg", Some("#000000"))?,
                color("Text", Some("#FFFFFF"))?,
                color("Glow", fill_text.as_deref())?,
            ],
            opacity,
            is_custom: true,
        })
    }

    /// LoadCustomThemes parses the complete list first, then catches each
    /// individual FromJson failure so valid neighbours survive.
    pub fn load_custom(json: &str, zone: &dyn LocalZone) -> Vec<Self> {
        let Ok(Some(Token::Array(items))) = newtonsoft::parse_dynamic_document(json, zone) else {
            return vec![];
        };
        items
            .iter()
            .filter_map(|item| Self::from_token(item).ok())
            .collect()
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
            category: self.category.clone(),
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

// JToken explicit string casts use invariant Convert.ToString, unlike typed
// JsonReader.ReadAsString (notably True/False, round-trip floats and DateTime).
fn dynamic_string(token: &Token) -> Result<Option<String>, newtonsoft::Error> {
    Ok(match token {
        Token::Null | Token::Undefined => None,
        Token::String(s) => Some(s.clone()),
        Token::Integer(i) => Some(i.to_string()),
        Token::Bool(b) => Some(if *b { "True" } else { "False" }.into()),
        Token::Float(f) => {
            let text = newtonsoft::format_double(*f);
            Some(text.strip_suffix(".0").unwrap_or(&text).to_owned())
        }
        Token::Date(d) => {
            let iso = newtonsoft::format_date(d);
            Some(format!(
                "{}/{}/{} {}",
                &iso[5..7],
                &iso[8..10],
                &iso[..4],
                &iso[11..19]
            ))
        }
        _ => return Err(newtonsoft::Error),
    })
}

fn dynamic_double(token: &Token) -> Result<f64, ThemeError> {
    match token {
        Token::String(s) => {
            colors::parse_double(s, &NumberCulture::INVARIANT).ok_or(ThemeError::InvalidOpacity)
        }
        Token::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Token::Integer(i) => Ok(*i as f64),
        Token::BigInteger(s) => s.parse().map_err(|_| ThemeError::InvalidOpacity),
        Token::Float(f) => Ok(*f),
        _ => Err(ThemeError::InvalidOpacity),
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
            category: Some(self.category.into()),
            colors: self.colors,
            opacity: self.opacity,
            is_custom: false,
        }
    }
}

pub const CATEGORIES: [&str; 5] = ["Minimal", "Apps", "Editors", "Atmosphere", "Accessibility"];
/// The original's twelve palettes, plus Alucard and Blood Moon (D60, ours:
/// not in the original).
pub const BUILTIN_THEMES: [BuiltinTheme; 14] = [
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
        // Dracula's official light variant, mapped as Dracula is.
        name: "Alucard",
        category: "Editors",
        colors: [
            Rgb(100, 74, 201),
            Rgb(163, 20, 77),
            Rgb(207, 207, 222),
            Rgb(3, 106, 150),
            Rgb(255, 251, 235),
            Rgb(31, 31, 31),
            Rgb(100, 74, 201),
        ],
        opacity: Some(0.85),
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
        // Crimson on near-black, with a copper peak.
        name: "Blood Moon",
        category: "Atmosphere",
        colors: [
            Rgb(230, 57, 70),
            Rgb(160, 16, 32),
            Rgb(38, 10, 13),
            Rgb(255, 122, 61),
            Rgb(20, 5, 7),
            Rgb(242, 216, 212),
            Rgb(200, 18, 40),
        ],
        opacity: Some(0.8),
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
