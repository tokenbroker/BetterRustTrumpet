//! The media popup, after the original's MediaPopupWindow: a 300 DIP
//! window holding a rounded card (margin 8, radius 12, 20 DIP shadow) with
//! the track title, timeline, the playing app's volume and the transport
//! controls; expanded, the cover art above them.
//!
//! High contrast only changes the card (FlyoutBackground: Window, with a
//! WindowFrame edge); the original hard-codes the rest of the popup white.
use super::anim::{self, EASE_IN_SHARP, EASE_OUT, GROW, RISE};
use crate::core::Core;
use bt_app::i18n::tr;
use bt_app::win32::{
    gfx::{self, Align, Canvas, Gfx, Image, Rect, Rgba, Surface, TextStyle},
    wnd::{self, Button, Event, Window},
};
use bt_media::{Command, NowPlaying, Repeat};
use std::{
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

pub const WIDTH: f32 = 300.0;
pub const COLLAPSED: f32 = 185.0;
pub const EXPANDED: f32 = 405.0;
const CARD_COLLAPSED: f32 = 169.0;
const CARD_EXPANDED: f32 = 389.0;
const FRAME: usize = 1;
const UI_FONT: &str = "Segoe UI Variable Text";
const DEFAULT_ACCENT: Rgba = Rgba::rgb(107, 77, 230);

// Phosphor (MIT) path data, as the original's PhosphorIconData.
const PLAY: &str = "M234.49,111.07,90.41,22.94A20,20,0,0,0,60,39.87V216.13a20,20,0,0,0,30.41,16.93l144.08-88.13a19.82,19.82,0,0,0,0-33.86ZM84,208.85V47.15L216.16,128Z";
const PAUSE: &str = "M200,28H160a20,20,0,0,0-20,20V208a20,20,0,0,0,20,20h40a20,20,0,0,0,20-20V48A20,20,0,0,0,200,28Zm-4,176H164V52h32ZM96,28H56A20,20,0,0,0,36,48V208a20,20,0,0,0,20,20H96a20,20,0,0,0,20-20V48A20,20,0,0,0,96,28ZM92,204H60V52H92Z";
const SPEAKER_HIGH: &str = "M157.27,21.22a12,12,0,0,0-12.64,1.31L75.88,76H32A20,20,0,0,0,12,96v64a20,20,0,0,0,20,20H75.88l68.75,53.47A12,12,0,0,0,164,224V32A12,12,0,0,0,157.27,21.22ZM36,100H68v56H36Zm104,99.46L92,162.13V93.87l48-37.33ZM212,128a44,44,0,0,1-11,29.11,12,12,0,1,1-18-15.88,20,20,0,0,0,0-26.43,12,12,0,0,1,18-15.86A43.94,43.94,0,0,1,212,128Zm40,0a83.87,83.87,0,0,1-21.39,56,12,12,0,0,1-17.89-16,60,60,0,0,0,0-80,12,12,0,1,1,17.88-16A83.87,83.87,0,0,1,252,128Z";
const SPEAKER_LOW: &str = "M157.27,21.22a12,12,0,0,0-12.64,1.31L75.88,76H32A20,20,0,0,0,12,96v64a20,20,0,0,0,20,20H75.88l68.75,53.47A12,12,0,0,0,164,224V32A12,12,0,0,0,157.27,21.22ZM36,100H68v56H36Zm104,99.46L92,162.13V93.87l48-37.33ZM212,128a44,44,0,0,1-11,29.11,12,12,0,0,1-18-15.88,20,20,0,0,0,0-26.44,12,12,0,0,1,18-15.85A43.94,43.94,0,0,1,212,128Z";
const SPEAKER_NONE: &str = "M157.27,21.22a12,12,0,0,0-12.64,1.31L75.88,76H32A20,20,0,0,0,12,96v64a20,20,0,0,0,20,20H75.88l68.75,53.47A12,12,0,0,0,164,224V32A12,12,0,0,0,157.27,21.22ZM36,100H68v56H36Zm104,99.46L92,162.12V93.88l48-37.34Z";
const SPEAKER_SLASH: &str = "M56.88,31.93A12,12,0,1,0,39.12,48.07L64.51,76H32A20,20,0,0,0,12,96v64a20,20,0,0,0,20,20H75.88l68.75,53.47A12,12,0,0,0,164,224V185.44l35.12,38.63a12,12,0,0,0,17.76-16.14ZM36,100H68v56H36Zm104,99.46L92,162.13V106.24L140,159Zm-31-134a12,12,0,0,1,2.11-16.84l33.51-26.07A12,12,0,0,1,164,32V94.94a12,12,0,0,1-24,0V56.54l-14.15,11A12,12,0,0,1,109,65.44Zm74,49.35a12,12,0,0,1,18-15.85,44,44,0,0,1,5.55,50.21,12,12,0,0,1-21-11.55A19.67,19.67,0,0,0,188,128,20,20,0,0,0,183,114.79ZM252,128a84.18,84.18,0,0,1-19.11,53.35,12,12,0,1,1-18.53-15.25A60,60,0,0,0,212.73,88a12,12,0,1,1,17.88-16A83.87,83.87,0,0,1,252,128Z";
const REPEAT: &str = "M20,128A76.08,76.08,0,0,1,96,52h99l-3.52-3.51a12,12,0,1,1,17-17l24,24a12,12,0,0,1,0,17l-24,24a12,12,0,0,1-17-17L195,76H96a52.06,52.06,0,0,0-52,52,12,12,0,0,1-24,0Zm204-12a12,12,0,0,0-12,12,52.06,52.06,0,0,1-52,52H61l3.52-3.51a12,12,0,1,0-17-17l-24,24a12,12,0,0,0,0,17l24,24a12,12,0,1,0,17-17L61,204h99a76.08,76.08,0,0,0,76-76A12,12,0,0,0,224,116Z";
const REPEAT_ONCE: &str = "M20,128A76.08,76.08,0,0,1,96,52h99l-3.52-3.51a12,12,0,1,1,17-17l24,24a12,12,0,0,1,0,17l-24,24a12,12,0,0,1-17-17L195,76H96a52.06,52.06,0,0,0-52,52,12,12,0,0,1-24,0Zm204-12a12,12,0,0,0-12,12,52.06,52.06,0,0,1-52,52H61l3.52-3.51a12,12,0,1,0-17-17l-24,24a12,12,0,0,0,0,17l24,24a12,12,0,1,0,17-17L61,204h99a76.08,76.08,0,0,0,76-76A12,12,0,0,0,224,116Zm-88,48a12,12,0,0,0,12-12V104a12,12,0,0,0-17.36-10.74l-16,8a12,12,0,0,0,9.36,22V152A12,12,0,0,0,136,164Z";
const SHUFFLE: &str = "M240.49,175.51a12,12,0,0,1,0,17l-24,24a12,12,0,0,1-17-17L203,196h-2.09a76.17,76.17,0,0,1-61.85-31.83L97.38,105.78A52.1,52.1,0,0,0,55.06,84H32a12,12,0,0,1,0-24H55.06a76.17,76.17,0,0,1,61.85,31.83l41.71,58.39A52.1,52.1,0,0,0,200.94,172H203l-3.52-3.51a12,12,0,0,1,17-17Zm-95.62-72.62a12,12,0,0,0,16.93-1.13A52,52,0,0,1,200.94,84H203l-3.52,3.51a12,12,0,0,0,17,17l24-24a12,12,0,0,0,0-17l-24-24a12,12,0,0,0-17,17L203,60h-2.09a76,76,0,0,0-57.2,26A12,12,0,0,0,144.87,102.89Zm-33.74,50.22a12,12,0,0,0-16.93,1.13A52,52,0,0,1,55.06,172H32a12,12,0,0,0,0,24H55.06a76,76,0,0,0,57.2-26A12,12,0,0,0,111.13,153.11Z";
const PREVIOUS: &str = "M201.75,30.52a20,20,0,0,0-20.3.53L68,102V40a12,12,0,0,0-24,0V216a12,12,0,0,0,24,0V154l113.45,71A20,20,0,0,0,212,208.12V47.88A19.86,19.86,0,0,0,201.75,30.52ZM188,200.73,71.7,128,188,55.27Z";
const NEXT: &str = "M200,28a12,12,0,0,0-12,12v62L74.55,31A20,20,0,0,0,44,47.88V208.12A20,20,0,0,0,74.55,225L188,154v62a12,12,0,0,0,24,0V40A12,12,0,0,0,200,28ZM68,200.73V55.27L184.3,128Z";
const CARET: &str = "M216.49,104.49l-80,80a12,12,0,0,1-17,0l-80-80a12,12,0,0,1,17-17L128,159l71.51-71.52a12,12,0,0,1,17,17Z";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Expand,
    Mute,
    Progress,
    Volume,
    Shuffle,
    Previous,
    Play,
    Next,
    Repeat,
}

/// The audio session of the app that owns the media session (the
/// original's FindCurrentMediaApp).
#[derive(Clone, Debug)]
struct MediaApp {
    device: String,
    key: String,
    volume: f32,
    muted: bool,
}

/// What the popup asks of the app after an event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reply {
    None,
    /// The pointer entered (true) or left (false) the popup.
    Hover(bool),
    /// The expand arrow changed the remembered expanded state.
    Expanded(bool),
}

struct Layout {
    card: Rect,
    pill: Rect,
    cover: Option<Rect>,
    marquee: Rect,
    position: Rect,
    progress: Rect,
    duration: Rect,
    mute: Rect,
    volume: Rect,
    volume_text: Rect,
    shuffle: Rect,
    previous: Rect,
    play: Rect,
    next: Rect,
    repeat: Rect,
}

pub struct Media {
    pub window: Window,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    pub visible: bool,
    shown: Instant,
    hiding: Option<Instant>,
    now_playing: Option<NowPlaying>,
    track: Option<String>,
    art_bytes: Option<Arc<Vec<u8>>>,
    art: Option<Rc<Image>>,
    accent_from: Rgba,
    accent_to: Rgba,
    accent_at: Instant,
    /// The current art's colour (the original's GetDominantColor).
    art_accent: Rgba,
    /// The theme's colour, used instead of the art's when set (D61).
    theme_accent: Option<Rgba>,
    /// Position anchor: when, and the position then.
    anchor: (Instant, i64),
    seek_pending: Option<Instant>,
    drag: Option<Hit>,
    drag_fraction: f32,
    app: Option<MediaApp>,
    pub expanded: bool,
    /// Expand/collapse animation: start and target.
    resize: Option<(Instant, bool)>,
    title: String,
    title_change: Option<(String, Instant)>,
    marquee: f32,
    marquee_at: Instant,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    pub hovering: bool,
    last_refresh: Instant,
    scale: f32,
    position: (i32, i32),
    tray: Option<(i32, i32, i32, i32)>,
    /// High contrast's system colours, set by the UI with the theme.
    pub contrast: Option<super::theme::HighContrast>,
}

fn time(ms: i64) -> String {
    let total = (ms / 1000).max(0);
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "Artist - Title", like GetCurrentMediaInfo.
fn title_text(now: Option<&NowPlaying>) -> String {
    let Some(now) = now else {
        return tr("MediaPopupNoMediaText").into();
    };
    let title = if now.title.is_empty() {
        now.source_app.clone()
    } else {
        now.title.clone()
    };
    if !now.artist.is_empty() {
        format!("{} - {}", now.artist, title)
    } else if title.is_empty() {
        tr("MediaPopupNoMediaText").into()
    } else {
        title
    }
}

/// The original's GetDominantColor: the average of a 32x32 copy, skipping
/// very dark and very bright pixels, scaled so the strongest channel is 80%.
fn dominant_color(image: &Image) -> Rgba {
    let (w, h) = (image.width as usize, image.height as usize);
    if w == 0 || h == 0 {
        return DEFAULT_ACCENT;
    }
    let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
    for y in 0..32 {
        for x in 0..32 {
            let (sx, sy) = (x * w / 32, y * h / 32);
            let p = &image.pixels[(sy * w + sx) * 4..][..4];
            let (pb, pg, pr) = (u32::from(p[0]), u32::from(p[1]), u32::from(p[2]));
            let sum = pr + pg + pb;
            if sum > 50 && sum < 700 {
                r += pr;
                g += pg;
                b += pb;
                n += 1;
            }
        }
    }
    if n == 0 {
        return DEFAULT_ACCENT;
    }
    let avg = [r / n, g / n, b / n];
    let max = avg.into_iter().max().unwrap_or(0);
    let factor = if max > 0 {
        255.0 / max as f32 * 0.8
    } else {
        1.0
    };
    let c = avg.map(|v| (v as f32 * factor).floor().min(255.0) as u8);
    Rgba::rgb(c[0], c[1], c[2])
}

fn lighter(c: Rgba) -> Rgba {
    let up = |v: f32| (v + 50.0 / 255.0).min(1.0);
    Rgba {
        r: up(c.r),
        g: up(c.g),
        b: up(c.b),
        a: c.a,
    }
}

impl Media {
    pub fn new(gfx: Rc<Gfx>, expanded: bool) -> windows::core::Result<Self> {
        let window = wnd::create(
            "BetterTrumpet media",
            wnd::Options {
                activate: false,
                click_through: false,
            },
        )?;
        let now = Instant::now();
        Ok(Self {
            window,
            surface: Some(Surface::new(gfx.clone())?),
            gfx,
            visible: false,
            shown: now,
            hiding: None,
            now_playing: None,
            track: None,
            art_bytes: None,
            art: None,
            accent_from: DEFAULT_ACCENT,
            accent_to: DEFAULT_ACCENT,
            accent_at: now,
            art_accent: DEFAULT_ACCENT,
            theme_accent: None,
            anchor: (now, 0),
            seek_pending: None,
            drag: None,
            drag_fraction: 0.0,
            app: None,
            expanded,
            resize: None,
            title: tr("MediaPopupNoMediaText").into(),
            title_change: None,
            marquee: 0.0,
            marquee_at: now,
            hover: None,
            pressed: None,
            hovering: false,
            last_refresh: now,
            scale: 1.0,
            position: (0, 0),
            tray: None,
            contrast: None,
        })
    }

    /// Shown and not fading out.
    pub fn showing(&self) -> bool {
        self.visible && self.hiding.is_none()
    }

    /// Window height in DIPs for the expanded state.
    pub fn height(&self) -> f32 {
        if self.expanded || self.resize.is_some_and(|(_, to)| !to) {
            EXPANDED
        } else {
            COLLAPSED
        }
    }

    fn card_height(&self, now: Instant) -> f32 {
        match self.resize {
            Some((start, true)) => anim::lerp(
                CARD_COLLAPSED,
                CARD_EXPANDED,
                EASE_OUT.at(anim::progress(start, 0.0, 240.0, now)),
            ),
            Some((start, false)) => anim::lerp(
                CARD_EXPANDED,
                CARD_COLLAPSED,
                EASE_OUT.at(anim::progress(start, 0.0, 200.0, now)),
            ),
            None if self.expanded => CARD_EXPANDED,
            None => CARD_COLLAPSED,
        }
    }

    fn cover_shown(&self) -> bool {
        self.expanded || self.resize.is_some()
    }

    fn layout(&self, now: Instant) -> Layout {
        let height = self.height();
        let card_h = self.card_height(now);
        let card = Rect::new(8.0, height - 8.0 - card_h, WIDTH - 16.0, card_h);
        let (left, width) = (card.x + 16.0, card.w - 32.0);
        let total = 156.0 + if self.cover_shown() { 214.0 } else { 0.0 };
        let mut y = card.bottom() - 5.0 - total;
        let pill = Rect::new(left + (width - 50.0) / 2.0, y, 50.0, 18.0);
        y += 22.0;
        let cover = self.cover_shown().then(|| {
            let r = Rect::new(left + (width - 202.0) / 2.0, y + 4.0, 202.0, 202.0);
            y += 214.0;
            r
        });
        let marquee = Rect::new(left, y, width, 20.0);
        y += 22.0 + 4.0;
        let time_style = TextStyle::new(UI_FONT, 10.0);
        let position_w = self
            .gfx
            .measure(&self.position_text(), &time_style)
            .0
            .ceil();
        let duration_w = self
            .gfx
            .measure(&self.duration_text(), &time_style)
            .0
            .ceil();
        let position = Rect::new(left, y, position_w, 20.0);
        let duration = Rect::new(left + width - duration_w, y, duration_w, 20.0);
        let progress = Rect::new(
            position.right() + 8.0,
            y,
            (duration.x - 8.0 - position.right() - 8.0).max(0.0),
            20.0,
        );
        y += 26.0 + 2.0;
        let mute = Rect::new(left, y, 28.0, 28.0);
        let volume_text = Rect::new(left + width - 24.0, y, 24.0, 28.0);
        let volume = Rect::new(
            mute.right() + 6.0,
            y + 4.0,
            (volume_text.x - 6.0 - mute.right() - 6.0).max(0.0),
            20.0,
        );
        y += 32.0;
        let main = 40.0 + 8.0 + 48.0 + 8.0 + 40.0;
        let main_left = left + (width - main) / 2.0;
        let previous = Rect::new(main_left, y + 4.0, 40.0, 40.0);
        let play = Rect::new(main_left + 48.0, y, 48.0, 48.0);
        let next = Rect::new(main_left + 104.0, y + 4.0, 40.0, 40.0);
        let shuffle = Rect::new(main_left - 8.0 - 32.0, y + 8.0, 32.0, 32.0);
        let repeat = Rect::new(main_left + main + 8.0, y + 8.0, 32.0, 32.0);
        Layout {
            card,
            pill,
            cover,
            marquee,
            position,
            progress,
            duration,
            mute,
            volume,
            volume_text,
            shuffle,
            previous,
            play,
            next,
            repeat,
        }
    }

    // ---------- state ----------

    /// Progress between reports runs from the last one, as the original's
    /// interpolated timeline.
    fn current_position(&self, now: Instant) -> i64 {
        let Some(np) = &self.now_playing else {
            return 0;
        };
        let mut position = self.anchor.1;
        if np.playing {
            position += now.duration_since(self.anchor.0).as_millis() as i64;
        }
        if np.duration_ms > 0 {
            position.min(np.duration_ms)
        } else {
            position
        }
    }

    fn shown_position(&self, now: Instant) -> i64 {
        match (self.drag, &self.now_playing) {
            (Some(Hit::Progress), Some(np)) => (self.drag_fraction * np.duration_ms as f32) as i64,
            _ => self.current_position(now),
        }
    }

    fn position_text(&self) -> String {
        time(self.shown_position(Instant::now()))
    }

    fn duration_text(&self) -> String {
        time(self.now_playing.as_ref().map_or(0, |n| n.duration_ms))
    }

    fn accent(&self, now: Instant) -> Rgba {
        let t = EASE_OUT.at(anim::progress(self.accent_at, 0.0, 320.0, now));
        self.accent_from.lerp(self.accent_to, t)
    }

    /// The theme's colour for the bars and toggles, or `None` to follow the
    /// album art as the original does (D61).
    pub fn set_theme_accent(&mut self, color: Option<Rgba>) {
        self.theme_accent = color;
        self.set_accent(color.unwrap_or(self.art_accent));
    }

    fn set_accent(&mut self, color: Rgba) {
        let now = Instant::now();
        if color == self.accent_to {
            return;
        }
        self.accent_from = self.accent(now);
        self.accent_to = color;
        self.accent_at = now;
    }

    /// Reads the media session and the media app's volume.
    pub fn refresh(&mut self, core: &Core) {
        self.last_refresh = Instant::now();
        let now_playing = crate::legacy_media::now_playing(core);
        let key = now_playing
            .as_ref()
            .map(|n| format!("{}\u{1}{}\u{1}{}", n.source_app, n.title, n.artist));
        let changed = key != self.track;
        self.track = key;
        let text = title_text(now_playing.as_ref());
        if text != self.title {
            if changed && self.title != tr("MediaPopupNoMediaText") && self.visible {
                self.title_change = Some((self.title.clone(), Instant::now()));
            }
            self.title = text;
            self.marquee = 0.0;
            self.marquee_at = Instant::now();
        }
        let art = now_playing.as_ref().and_then(|n| n.art.clone());
        let same_art = match (&art, &self.art_bytes) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same_art {
            let decoded = art.as_ref().and_then(|bytes| gfx::decode_image(bytes, 400));
            self.art_bytes = art;
            self.art_accent = decoded.as_deref().map_or(DEFAULT_ACCENT, dominant_color);
            self.set_accent(self.theme_accent.unwrap_or(self.art_accent));
            // Low-resolution art is shown slightly blurred (ApplyThumbnail).
            self.art = decoded.map(super::blur::soften_art);
        }
        if let Some(np) = &now_playing {
            // After a seek, ignore reports until the app catches up (or 2 s
            // pass).
            let now = Instant::now();
            if let Some(deadline) = self.seek_pending {
                let close = (np.position_ms - self.current_position(now)).abs() <= 2000;
                if close || now > deadline {
                    self.seek_pending = None;
                }
            }
            if self.seek_pending.is_none() && self.drag != Some(Hit::Progress) {
                self.anchor = (now, np.position_ms);
            }
        }
        self.now_playing = now_playing;
        self.app = self.find_app(core);
    }

    fn find_app(&self, core: &Core) -> Option<MediaApp> {
        let source = self.now_playing.as_ref()?.source_app.to_lowercase();
        if source.trim().is_empty() {
            return None;
        }
        let stem = |s: &str| {
            let name = s.rsplit(['\\', '/']).next().unwrap_or(s);
            name.strip_suffix(".exe").unwrap_or(name).to_string()
        };
        let source_stem = stem(&source);
        let snapshot = core.audio.snapshot().ok()?;
        for device in &snapshot.devices {
            for session in device.apps.iter().filter(|a| !a.is_system_sounds) {
                let exe = stem(
                    &session
                        .executable
                        .clone()
                        .unwrap_or_default()
                        .to_lowercase(),
                );
                if exe.len() >= 3 && (exe == source_stem || source.contains(&exe)) {
                    return Some(MediaApp {
                        device: device.id.clone(),
                        key: session.key.clone(),
                        volume: session.volume,
                        muted: session.muted,
                    });
                }
            }
        }
        None
    }

    // ---------- show / hide ----------

    /// Shows the popup with its slide-in, centred 5 DIP above the tray icon.
    pub fn show(&mut self, core: &Core, tray: (i32, i32, i32, i32)) {
        if self.visible && self.hiding.is_none() {
            return;
        }
        self.tray = Some(tray);
        self.hiding = None;
        self.resize = None;
        self.title_change = None;
        if !self.visible {
            self.shown = Instant::now();
        }
        self.refresh(core);
        self.place();
        self.paint();
        self.window.show(false);
        self.visible = true;
        self.window.set_timer(FRAME, 15);
    }

    /// Plays the fade-out, then hides.
    pub fn hide(&mut self) {
        if !self.visible || self.hiding.is_some() {
            return;
        }
        self.hiding = Some(Instant::now());
        if self.drag.take().is_some() {
            self.window.release_capture();
        }
        self.window.set_timer(FRAME, 15);
    }

    fn finish_hide(&mut self) {
        self.window.kill_timer(FRAME);
        self.window.hide();
        self.visible = false;
        self.hiding = None;
        self.hover = None;
        self.pressed = None;
        self.hovering = false;
        if let Some(surface) = &mut self.surface {
            surface.release();
        }
    }

    /// Window = card + 8 DIP shadow margin; its bottom sits 5 DIP above the
    /// icon, centred on it and kept 10 DIP inside the work area.
    pub fn place(&mut self) {
        let Some((tx, ty, tw, th)) = self.tray else {
            return;
        };
        let monitor = wnd::monitor_at(tx + tw / 2, ty + th / 2);
        self.scale = monitor.scale;
        let scale = self.scale;
        let width = (WIDTH * scale).round() as i32;
        let height = (self.height() * scale).round() as i32;
        let inset = (10.0 * scale) as i32;
        let (wx, wy, ww, _) = monitor.work;
        let screen_bottom = monitor.bounds.1 + monitor.bounds.3;
        let top = ty.min(screen_bottom - th);
        let left = wx + inset;
        let right = wx + ww - width - inset;
        let x = (tx + tw / 2 - width / 2).clamp(left, right.max(left));
        let y = (top - height - (5.0 * scale) as i32).max(wy + inset);
        self.position = (x, y);
    }

    /// Where the tray tooltip goes: above the popup (see `Tip`).
    pub fn tray_anchor(&self) -> Option<(i32, i32, i32, i32)> {
        self.tray
    }

    // ---------- frames ----------

    fn animating(&self, now: Instant) -> bool {
        anim::running(Some(self.shown), 350.0, now)
            || self.hiding.is_some()
            || self.resize.is_some()
            || anim::running(Some(self.accent_at), 320.0, now)
            || self.title_change.is_some()
    }

    pub fn timer(&mut self, id: usize, core: &Core) {
        if id != FRAME || !self.visible {
            return;
        }
        let now = Instant::now();
        if let Some(start) = self.hiding {
            if anim::progress(start, 0.0, 150.0, now) >= 1.0 {
                self.finish_hide();
                return;
            }
        }
        if let Some((start, to)) = self.resize {
            let ms = if to { 240.0 } else { 200.0 };
            if anim::progress(start, 0.0, ms, now) >= 1.0 {
                self.resize = None;
                if !to {
                    // Collapse: the window shrinks once the card has.
                    self.place();
                }
            }
        }
        if let Some((_, start)) = &self.title_change {
            if anim::progress(*start, 90.0, 160.0, now) >= 1.0 {
                self.title_change = None;
            }
        }
        if now.duration_since(self.last_refresh) >= Duration::from_secs(1) {
            self.refresh(core);
        }
        // The marquee scrolls 1 DIP every 50 ms and wraps in from the right
        // edge once it has run off the left.
        let steps = now.duration_since(self.marquee_at).as_millis() / 50;
        if steps > 0 {
            self.marquee_at += Duration::from_millis(50 * steps as u64);
            let style = TextStyle::new(UI_FONT, 13.0).weight(600);
            let (width, _) = self.gfx.measure(&self.title, &style);
            let room = self.layout(now).marquee.w;
            if width > room {
                for _ in 0..steps.min(1000) {
                    self.marquee -= 1.0;
                    if self.marquee < -width {
                        self.marquee = room;
                    }
                }
            }
        }
        self.paint();
        let interval = if self.animating(now) { 15 } else { 50 };
        self.window.set_timer(FRAME, interval);
    }

    // ---------- input ----------

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let l = self.layout(Instant::now());
        let np = self.now_playing.as_ref();
        let candidates = [
            (l.pill, Hit::Expand, true),
            (l.mute, Hit::Mute, true),
            (l.progress, Hit::Progress, true),
            (l.volume, Hit::Volume, true),
            (l.shuffle, Hit::Shuffle, np.is_some_and(|n| n.can_shuffle)),
            (l.previous, Hit::Previous, true),
            (l.play, Hit::Play, true),
            (l.next, Hit::Next, true),
            (l.repeat, Hit::Repeat, np.is_some_and(|n| n.can_repeat)),
        ];
        candidates
            .into_iter()
            .find(|(r, _, shown)| *shown && r.contains(x, y))
            .map(|(_, h, _)| h)
    }

    fn fraction(rect: Rect, x: f32) -> f32 {
        ((x - rect.x) / rect.w.max(1.0)).clamp(0.0, 1.0)
    }

    fn command(&mut self, core: &Core, command: Command) {
        if let Some(media) = &core.media {
            let _ = media.send(command);
        }
        self.refresh(core);
    }

    /// Volume of the media app's session; dragging also unmutes.
    fn apply_volume(&mut self, core: &Core, fraction: f32) {
        let Some(app) = &mut self.app else {
            return;
        };
        let volume = (fraction * 100.0).floor() / 100.0;
        app.volume = volume;
        let _ = core.audio.set_app_volume(&app.device, &app.key, volume);
        if app.muted && volume > 0.0 {
            app.muted = false;
            let _ = core.audio.set_app_mute(&app.device, &app.key, false);
        }
    }

    pub fn event(&mut self, core: &mut Core, event: Event) -> Reply {
        if !self.visible {
            return Reply::None;
        }
        let mut reply = Reply::None;
        let now = Instant::now();
        match event {
            Event::MouseMove(x, y) => {
                if !self.hovering {
                    self.hovering = true;
                    reply = Reply::Hover(true);
                }
                let l = self.layout(now);
                match self.drag {
                    Some(Hit::Progress) => self.drag_fraction = Self::fraction(l.progress, x),
                    Some(Hit::Volume) => self.apply_volume(core, Self::fraction(l.volume, x)),
                    _ => self.hover = self.hit(x, y),
                }
            }
            Event::MouseLeave => {
                self.hover = None;
                if self.hovering && self.drag.is_none() {
                    self.hovering = false;
                    reply = Reply::Hover(false);
                }
            }
            Event::MouseDown(Button::Left, x, y, _) => {
                let hit = self.hit(x, y);
                self.pressed = hit;
                let l = self.layout(now);
                match hit {
                    Some(Hit::Progress) => {
                        if let Some(np) = &self.now_playing {
                            if np.can_seek && np.duration_ms > 0 {
                                self.drag = Some(Hit::Progress);
                                self.drag_fraction = Self::fraction(l.progress, x);
                                self.window.capture();
                            }
                        }
                    }
                    Some(Hit::Volume) if self.app.is_some() => {
                        self.drag = Some(Hit::Volume);
                        self.apply_volume(core, Self::fraction(l.volume, x));
                        self.window.capture();
                    }
                    _ => {}
                }
            }
            Event::MouseUp(Button::Left, x, y, _) => {
                if let Some(drag) = self.drag.take() {
                    self.window.release_capture();
                    if drag == Hit::Progress {
                        if let Some(duration) = self.now_playing.as_ref().map(|n| n.duration_ms) {
                            let position = (self.drag_fraction * duration as f32).round() as i64;
                            self.anchor = (now, position);
                            self.seek_pending = Some(now + Duration::from_secs(2));
                            self.command(core, Command::Seek(position));
                        }
                    }
                    self.pressed = None;
                } else {
                    let hit = self.hit(x, y);
                    if hit.is_some() && hit == self.pressed.take() {
                        reply = self.click(core, hit.unwrap_or(Hit::Play));
                    }
                }
            }
            Event::CaptureLost => {
                self.drag = None;
            }
            _ => return Reply::None,
        }
        self.paint();
        reply
    }

    fn click(&mut self, core: &Core, hit: Hit) -> Reply {
        match hit {
            Hit::Play => self.command(core, Command::PlayPause),
            Hit::Next => self.command(core, Command::Next),
            Hit::Previous => self.command(core, Command::Previous),
            Hit::Shuffle => self.command(core, Command::ToggleShuffle),
            Hit::Repeat => self.command(core, Command::CycleRepeat),
            Hit::Mute => {
                if let Some(app) = &mut self.app {
                    app.muted = !app.muted;
                    let _ = core.audio.set_app_mute(&app.device, &app.key, app.muted);
                }
            }
            Hit::Expand => {
                // Expand: the window grows upwards first, then the card and
                // cover animate. Collapse: they animate, then it shrinks.
                self.expanded = !self.expanded;
                self.resize = Some((Instant::now(), self.expanded));
                self.place();
                self.window.set_timer(FRAME, 15);
                return Reply::Expanded(self.expanded);
            }
            Hit::Progress | Hit::Volume => {}
        }
        Reply::None
    }

    /// Draws the popup as it would show (animations finished) without
    /// showing it.
    pub fn render(
        &mut self,
        core: &Core,
        tray: (i32, i32, i32, i32),
        expanded: bool,
    ) -> Option<(u32, u32, Vec<u8>)> {
        self.tray = Some(tray);
        self.expanded = expanded;
        self.refresh(core);
        self.place();
        let past = Instant::now() - Duration::from_secs(5);
        self.shown = past;
        self.accent_at = past;
        self.paint();
        self.surface.as_ref()?.rgba()
    }

    // ---------- painting ----------

    pub fn paint(&mut self) {
        let now = Instant::now();
        let width = (WIDTH * self.scale).round() as i32;
        let height = (self.height() * self.scale).round() as i32;
        let opacity = match self.hiding {
            Some(start) => 1.0 - EASE_IN_SHARP.at(anim::progress(start, 0.0, 150.0, now)),
            None => EASE_OUT.at(anim::progress(self.shown, 0.0, 250.0, now)),
        };
        let Some(mut surface) = self.surface.take() else {
            return;
        };
        surface.draw(width, height, self.scale, |canvas| self.draw(canvas, now));
        surface.present(self.window.raw(), self.position.0, self.position.1, opacity);
        self.surface = Some(surface);
    }

    fn draw(&self, canvas: &mut Canvas, now: Instant) {
        let l = self.layout(now);
        // SlideIn: rise 20 -> 0 (350 ms) and grow 0.8 -> 1 (300 ms); SlideOut:
        // 8 DIP down and 0.95 (150 ms). Both about the card's bottom centre.
        let (dy, scale) = match self.hiding {
            Some(start) => {
                let t = EASE_IN_SHARP.at(anim::progress(start, 0.0, 150.0, now));
                (8.0 * t, anim::lerp(1.0, 0.95, t))
            }
            None => (
                20.0 * (1.0 - RISE.at(anim::progress(self.shown, 0.0, 350.0, now))),
                anim::lerp(
                    0.8,
                    1.0,
                    GROW.at(anim::progress(self.shown, 0.0, 300.0, now)),
                ),
            ),
        };
        let (ox, oy) = (l.card.x + l.card.w / 2.0, l.card.bottom());
        canvas.push_transform(0.0, dy, scale, ox, oy);
        // Drop shadow: 0 3 20 rgba(0, 0, 0, 0.5), in soft rings.
        for i in (1..=6).rev() {
            let spread = i as f32 * 1.4;
            let alpha = 0.5 / 6.0 * (1.0 - i as f32 / 7.0);
            canvas.round_rect(
                l.card.offset(0.0, 3.0).inset(-spread, -spread),
                12.0 + spread,
                Rgba::rgba(0, 0, 0, alpha),
            );
        }
        let (card, edge) = self.contrast.map_or(
            (
                Rgba::rgba(43, 43, 43, 0.96),
                Rgba::rgba(255, 255, 255, 0.08),
            ),
            |hc| (hc.window, hc.window_frame),
        );
        canvas.round_rect(l.card, 12.0, card);
        canvas.stroke_round_rect(l.card, 12.0, 1.0, edge);
        canvas.push_layer(1.0, Some((l.card, 12.0)));
        self.draw_content(canvas, &l, now);
        canvas.pop();
        canvas.pop_transform();
    }

    fn button_bg(
        &self,
        canvas: &mut Canvas,
        rect: Rect,
        hit: Hit,
        radius: f32,
        hover: f32,
        pressed: f32,
    ) {
        let alpha = match (self.hover, self.pressed) {
            (Some(h), Some(p)) if h == hit && p == hit => pressed,
            (Some(h), _) if h == hit => hover,
            _ => return,
        };
        canvas.round_rect(rect, radius, Rgba::rgba(255, 255, 255, alpha));
    }

    fn draw_content(&self, canvas: &mut Canvas, l: &Layout, now: Instant) {
        let accent = self.accent(now);
        let accent_light = lighter(accent);
        let np = self.now_playing.as_ref();
        // Expand arrow: a 50x18 pill with a 9x5 caret.
        let pill_alpha = if self.hover == Some(Hit::Expand) {
            0.25
        } else {
            0.125
        };
        canvas.round_rect(l.pill, 9.0, Rgba::rgba(255, 255, 255, pill_alpha));
        let (cx, cy) = l.pill.center();
        if !self.expanded {
            canvas.push_rotation(180.0, cx, cy);
        }
        canvas.path(CARET, l.pill.centered(9.0, 5.0), Rgba::WHITE);
        if !self.expanded {
            canvas.pop_transform();
        }
        if let Some(cover) = l.cover {
            self.draw_cover(canvas, cover, now);
        }
        self.draw_title(canvas, l.marquee, now);
        // Timeline.
        let time_style = TextStyle::new(UI_FONT, 10.0);
        let dim = Rgba::rgba(255, 255, 255, 0.5);
        canvas.text(&self.position_text(), l.position, &time_style, dim);
        canvas.text(
            &self.duration_text(),
            l.duration,
            &time_style.align(Align::Trailing),
            dim,
        );
        let fraction = match np {
            Some(n) if n.duration_ms > 0 => self.shown_position(now) as f32 / n.duration_ms as f32,
            _ => 0.0,
        };
        self.draw_track(
            canvas,
            l.progress,
            fraction.clamp(0.0, 1.0),
            &[(0.0, accent), (1.0, accent)],
            false,
        );
        // Volume row.
        let disabled = self.app.is_none();
        if disabled {
            canvas.push_layer(0.45, None);
        }
        let (volume, muted) = self
            .app
            .as_ref()
            .map_or((1.0, false), |a| (a.volume, a.muted));
        self.button_bg(canvas, l.mute, Hit::Mute, 4.0, 0.125, 0.25);
        let percent = (volume * 100.0).round() as i32;
        let icon = if muted || percent == 0 {
            SPEAKER_SLASH
        } else if percent < 33 {
            SPEAKER_NONE
        } else if percent < 66 {
            SPEAKER_LOW
        } else {
            SPEAKER_HIGH
        };
        canvas.path(
            icon,
            l.mute.centered(12.0, 12.0),
            Rgba::rgba(255, 255, 255, 0.78),
        );
        self.draw_track(
            canvas,
            l.volume,
            volume,
            &[(0.0, accent), (1.0, accent_light)],
            true,
        );
        canvas.text(
            &percent.to_string(),
            l.volume_text,
            &TextStyle::new(UI_FONT, 11.0).align(Align::Trailing),
            dim,
        );
        if disabled {
            canvas.pop();
        }
        // Controls.
        let toggle =
            |canvas: &mut Canvas, rect: Rect, hit: Hit, path: &'static str, active: bool| {
                self.button_bg(canvas, rect, hit, 4.0, 0.125, 0.25);
                let color = if active {
                    accent
                } else if self.hover == Some(hit) {
                    Rgba::WHITE
                } else {
                    Rgba::rgba(255, 255, 255, 0.56)
                };
                canvas.path(path, rect.centered(13.0, 13.0), color);
            };
        if np.is_some_and(|n| n.can_shuffle) {
            toggle(
                canvas,
                l.shuffle,
                Hit::Shuffle,
                SHUFFLE,
                np.and_then(|n| n.shuffle) == Some(true),
            );
        }
        if np.is_some_and(|n| n.can_repeat) {
            let repeat = np.and_then(|n| n.repeat);
            let path = if repeat == Some(Repeat::Track) {
                REPEAT_ONCE
            } else {
                REPEAT
            };
            toggle(
                canvas,
                l.repeat,
                Hit::Repeat,
                path,
                matches!(repeat, Some(Repeat::List | Repeat::Track)),
            );
        }
        for (rect, hit, path, size) in [
            (l.previous, Hit::Previous, PREVIOUS, 14.0),
            (l.next, Hit::Next, NEXT, 14.0),
        ] {
            self.button_bg(canvas, rect, hit, 6.0, 0.19, 0.31);
            canvas.path(path, rect.centered(size, size), Rgba::WHITE);
        }
        self.button_bg(canvas, l.play, Hit::Play, 6.0, 0.19, 0.31);
        let playing = np.is_some_and(|n| n.playing);
        let icon = l.play.centered(18.0, 18.0);
        canvas.path(
            if playing { PAUSE } else { PLAY },
            if playing { icon } else { icon.offset(1.0, 0.0) },
            Rgba::WHITE,
        );
    }

    /// A 20 DIP track: a 4 DIP rail, the fill and a 10 DIP thumb.
    fn draw_track(
        &self,
        canvas: &mut Canvas,
        rect: Rect,
        fraction: f32,
        fill: &[(f32, Rgba)],
        glow: bool,
    ) {
        canvas.round_rect(
            Rect::new(rect.x, rect.y + 8.0, rect.w, 4.0),
            2.0,
            Rgba::rgba(255, 255, 255, 0.19),
        );
        let filled = Rect::new(rect.x, rect.y + 8.0, rect.w * fraction, 4.0);
        if filled.w > 0.0 {
            canvas.gradient(filled, 2.0, fill, false);
        }
        let (tx, ty) = (rect.x + rect.w * fraction, rect.y + 10.0);
        if glow {
            canvas.circle(tx, ty, 7.0, Rgba::rgba(255, 255, 255, 0.12));
            canvas.circle(tx, ty, 5.0, Rgba::WHITE);
        } else {
            canvas.circle(tx, ty, 5.0, Rgba::rgba(255, 255, 255, 0.25));
            canvas.circle(tx, ty, 4.0, Rgba::rgb(247, 248, 252));
        }
    }

    /// Cover art: a 200x200 image with radius 8 over a #30000000 offset
    /// shadow, darkened at the top and bottom.
    fn draw_cover(&self, canvas: &mut Canvas, cover: Rect, now: Instant) {
        let (opacity, scale, dy) = match self.resize {
            Some((start, true)) => {
                let t = EASE_OUT.at(anim::progress(start, 0.0, 220.0, now));
                (t, anim::lerp(0.985, 1.0, t), 6.0 * (1.0 - t))
            }
            Some((start, false)) => {
                let t = EASE_IN_SHARP.at(anim::progress(start, 0.0, 140.0, now));
                (1.0 - t, anim::lerp(1.0, 0.99, t), -4.0 * t)
            }
            None => (1.0, 1.0, 0.0),
        };
        if opacity <= 0.0 {
            return;
        }
        let (cx, cy) = cover.center();
        canvas.push_layer(opacity, None);
        canvas.push_transform(0.0, dy, scale, cx, cy);
        let frame = Rect::new(cover.x, cover.y, 200.0, 200.0);
        canvas.round_rect(frame.offset(2.0, 2.0), 8.0, Rgba::rgba(0, 0, 0, 0.19));
        canvas.round_rect(frame, 8.0, Rgba::rgba(255, 255, 255, 0.06));
        if let Some(art) = &self.art {
            canvas.push_layer(1.0, Some((frame, 8.0)));
            canvas.image_cover(art, frame, 1.0);
            canvas.pop();
        }
        canvas.gradient(
            frame,
            8.0,
            &[
                (0.0, Rgba::rgba(0, 0, 0, 0.125)),
                (0.3, Rgba::TRANSPARENT),
                (0.7, Rgba::TRANSPARENT),
                (1.0, Rgba::rgba(0, 0, 0, 0.19)),
            ],
            true,
        );
        canvas.pop_transform();
        canvas.pop();
    }

    /// The marquee title: 13 DIP semibold, centred when it fits, else
    /// scrolling and wrapping in from the right edge. A new track slides the
    /// old title out (90 ms) and the new one in (160 ms).
    fn draw_title(&self, canvas: &mut Canvas, rect: Rect, now: Instant) {
        let style = TextStyle::new(UI_FONT, 13.0).weight(600);
        canvas.push_layer(1.0, Some((rect, 0.0)));
        let draw = |canvas: &mut Canvas, text: &str, dx: f32, opacity: f32, scroll: bool| {
            let (w, _) = self.gfx.measure(text, &style);
            let x = if w > rect.w && scroll {
                self.marquee
            } else if w > rect.w {
                0.0
            } else {
                (rect.w - w) / 2.0
            };
            canvas.text(
                text,
                Rect::new(rect.x + x + dx, rect.y, w + 2.0, rect.h),
                &style,
                Rgba::WHITE.alpha(opacity),
            );
        };
        match &self.title_change {
            Some((old, start)) if anim::progress(*start, 0.0, 90.0, now) < 1.0 => {
                let t = EASE_IN_SHARP.at(anim::progress(*start, 0.0, 90.0, now));
                draw(canvas, old, -6.0 * t, 1.0 - t, false);
            }
            Some((_, start)) => {
                let t = EASE_OUT.at(anim::progress(*start, 90.0, 160.0, now));
                draw(canvas, &self.title, 6.0 * (1.0 - t), t, true);
            }
            None => draw(canvas, &self.title, 0.0, 1.0, true),
        }
        canvas.pop();
    }
}
