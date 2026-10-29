//! Drawing for the native windows: Direct2D (software) into a DIB that a
//! layered window shows with per-pixel alpha, like the original's WPF
//! `AllowsTransparency` windows. Text is DirectWrite. Coordinates are DIPs.
//!
//! Software rendering keeps the process free of a Direct3D device; the
//! windows are small and are only redrawn while something changes.
use std::{cell::RefCell, collections::HashMap, ffi::c_void, mem::ManuallyDrop, rc::Rc};
use windows::{
    Win32::{
        Foundation::{COLORREF, HWND, POINT, RECT, SIZE},
        Graphics::{
            Direct2D::{
                Common::{
                    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BEZIER_SEGMENT,
                    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
                    D2D1_FIGURE_END_OPEN, D2D1_FILL_MODE_WINDING, D2D1_GRADIENT_STOP,
                    D2D1_PIXEL_FORMAT,
                },
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE,
                D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
                D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
                D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_GAMMA_2_2,
                D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS,
                D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES,
                D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION_CLOCKWISE,
                D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
                D2D1CreateFactory, ID2D1Bitmap, ID2D1DCRenderTarget, ID2D1Factory, ID2D1Geometry,
                ID2D1PathGeometry, ID2D1SolidColorBrush,
            },
            DirectWrite::{
                DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_WEIGHT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_READING_DIRECTION_RIGHT_TO_LEFT,
                DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
                DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
                DWRITE_WORD_WRAPPING_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
                IDWriteTextLayout,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Gdi::{
                AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
                BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
                DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
            },
            Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapDecoder,
                IWICBitmapSource, IWICImagingFactory, IWICStream, WICBitmapDitherTypeNone,
                WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom,
                WICDecodeMetadataCacheOnDemand,
            },
        },
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow},
    },
    core::{HSTRING, Interface},
};
use windows_numerics::{Matrix3x2, Vector2};

/// A straight-alpha colour, components 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 1.0)
    }
    #[allow(clippy::self_named_constructors)]
    pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a,
        }
    }
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0.0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// This colour with its alpha multiplied by `a`.
    pub fn alpha(self, a: f32) -> Self {
        Self {
            a: self.a * a,
            ..self
        }
    }
    /// `#rrggbb` or `#aarrggbb`.
    pub fn hex(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
        match digits.len() {
            6 => Some(Self::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Self::rgba(
                byte(2)?,
                byte(4)?,
                byte(6)?,
                f32::from(byte(0)?) / 255.0,
            )),
            _ => None,
        }
    }
    pub fn lerp(self, to: Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            r: mix(self.r, to.r),
            g: mix(self.g, to.g),
            b: mix(self.b, to.b),
            a: mix(self.a, to.a),
        }
    }
    fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    /// A `w` x `h` rectangle centred in this one.
    pub fn centered(&self, w: f32, h: f32) -> Self {
        let (cx, cy) = self.center();
        Self::new(cx - w / 2.0, cy - h / 2.0, w, h)
    }
    pub fn offset(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }
    pub fn inset(&self, dx: f32, dy: f32) -> Self {
        Self::new(
            self.x + dx,
            self.y + dy,
            self.w - 2.0 * dx,
            self.h - 2.0 * dy,
        )
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Leading,
    Center,
    Trailing,
}

/// A text style: font, size (DIPs), weight and layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub family: &'static str,
    pub size: f32,
    pub weight: u16,
    pub align: Align,
    /// Vertically centred in the rectangle (else top-aligned).
    pub middle: bool,
    /// Trims with an ellipsis instead of overflowing.
    pub ellipsis: bool,
    pub wrap: bool,
    /// Right-to-left reading order: leading is the right edge. Set by a
    /// mirrored canvas.
    pub rtl: bool,
}

impl TextStyle {
    pub const fn new(family: &'static str, size: f32) -> Self {
        Self {
            family,
            size,
            weight: 400,
            align: Align::Leading,
            middle: true,
            ellipsis: false,
            wrap: false,
            rtl: false,
        }
    }
    pub const fn weight(self, weight: u16) -> Self {
        Self { weight, ..self }
    }
    pub const fn align(self, align: Align) -> Self {
        Self { align, ..self }
    }
    pub const fn top(self) -> Self {
        Self {
            middle: false,
            ..self
        }
    }
    pub const fn ellipsis(self) -> Self {
        Self {
            ellipsis: true,
            ..self
        }
    }
    pub const fn wrap(self) -> Self {
        Self { wrap: true, ..self }
    }
    fn key(&self) -> StyleKey {
        (
            self.family.to_owned(),
            self.size.to_bits(),
            self.weight,
            self.align,
            self.middle,
            self.ellipsis,
            self.wrap,
            self.rtl,
        )
    }
}

/// A text style as a cache key: family, size bits, weight, alignment and
/// the middle, ellipsis, wrap and rtl flags.
type StyleKey = (String, u32, u16, Align, bool, bool, bool, bool);

/// Premultiplied BGRA pixels with an identity for the per-window bitmap
/// cache.
pub struct Image {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Size of the source before any downscaling.
    pub natural: (u32, u32),
}

thread_local! {
    static NEXT_IMAGE: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

impl Image {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Rc<Self> {
        Self::with_natural(width, height, pixels, (width, height))
    }
    fn with_natural(width: u32, height: u32, pixels: Vec<u8>, natural: (u32, u32)) -> Rc<Self> {
        let id = NEXT_IMAGE.with(|n| {
            let id = n.get();
            n.set(id + 1);
            id
        });
        Rc::new(Self {
            id,
            width,
            height,
            pixels,
            natural,
        })
    }
    /// Premultiplies straight-alpha BGRA in place.
    pub fn from_straight_bgra(width: u32, height: u32, mut pixels: Vec<u8>) -> Rc<Self> {
        for p in pixels.chunks_exact_mut(4) {
            let a = u16::from(p[3]);
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
        Self::new(width, height, pixels)
    }
}

/// Device-independent resources shared by every window.
pub struct Gfx {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    formats: RefCell<HashMap<StyleKey, IDWriteTextFormat>>,
    layouts: RefCell<HashMap<(String, StyleKey, u32, u32), IDWriteTextLayout>>,
    paths: RefCell<HashMap<usize, Option<(ID2D1PathGeometry, D2D_RECT_F)>>>,
}

impl Gfx {
    pub fn new() -> windows::core::Result<Rc<Self>> {
        // SAFETY: plain factory creation; the factories are COM objects
        // used only on this (the UI) thread.
        unsafe {
            Ok(Rc::new(Self {
                d2d: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
                dwrite: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?,
                formats: RefCell::default(),
                layouts: RefCell::default(),
                paths: RefCell::default(),
            }))
        }
    }

    /// Drops cached text layouts (a closed window's labels); formats and
    /// paths are few and stay.
    pub fn trim(&self) {
        let mut layouts = self.layouts.borrow_mut();
        layouts.clear();
        layouts.shrink_to_fit();
    }

    /// Whether a font family is installed (for the icon font fallback).
    pub fn has_font(&self, family: &str) -> bool {
        let mut collection = None;
        // SAFETY: out-parameters are valid locals; the name is a NUL-
        // terminated HSTRING that outlives the call.
        unsafe {
            if self
                .dwrite
                .GetSystemFontCollection(&mut collection, false)
                .is_err()
            {
                return false;
            }
            let Some(collection) = collection else {
                return false;
            };
            let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
            collection
                .FindFamilyName(&HSTRING::from(family), &mut index, &mut exists)
                .is_ok()
                && exists.as_bool()
        }
    }

    fn format(&self, style: &TextStyle) -> Option<IDWriteTextFormat> {
        let key = style.key();
        if let Some(format) = self.formats.borrow().get(&key) {
            return Some(format.clone());
        }
        // SAFETY: creates and configures a text format; strings are HSTRINGs
        // that outlive the calls.
        let format = unsafe {
            let format = self
                .dwrite
                .CreateTextFormat(
                    &HSTRING::from(style.family),
                    None,
                    DWRITE_FONT_WEIGHT(i32::from(style.weight)),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size,
                    &HSTRING::from("en-us"),
                )
                .ok()?;
            let _ = format.SetTextAlignment(match style.align {
                Align::Leading => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Trailing => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            let _ = format.SetParagraphAlignment(if style.middle {
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER
            } else {
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR
            });
            if style.rtl {
                let _ = format.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT);
            }
            let _ = format.SetWordWrapping(if style.wrap {
                DWRITE_WORD_WRAPPING_WRAP
            } else {
                DWRITE_WORD_WRAPPING_NO_WRAP
            });
            if style.ellipsis {
                if let Ok(sign) = self.dwrite.CreateEllipsisTrimmingSign(&format) {
                    let trimming = DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    };
                    let _ = format.SetTrimming(&trimming, &sign);
                }
            }
            format
        };
        self.formats.borrow_mut().insert(key, format.clone());
        Some(format)
    }

    fn layout(&self, text: &str, style: &TextStyle, w: f32, h: f32) -> Option<IDWriteTextLayout> {
        let key = (text.to_owned(), style.key(), w.to_bits(), h.to_bits());
        if let Some(layout) = self.layouts.borrow().get(&key) {
            return Some(layout.clone());
        }
        let format = self.format(style)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: `wide` outlives the call, which copies it.
        let layout = unsafe {
            self.dwrite
                .CreateTextLayout(&wide, &format, w.max(0.0), h.max(0.0))
                .ok()?
        };
        let mut layouts = self.layouts.borrow_mut();
        // Text changes slowly (names, values); a bounded cache is plenty.
        if layouts.len() > 400 {
            layouts.clear();
        }
        layouts.insert(key, layout.clone());
        Some(layout)
    }

    /// Width and height of `text` laid out on one line.
    pub fn measure(&self, text: &str, style: &TextStyle) -> (f32, f32) {
        let style = TextStyle {
            ellipsis: false,
            wrap: false,
            ..*style
        };
        let Some(layout) = self.layout(text, &style, 10_000.0, 1_000.0) else {
            return (0.0, 0.0);
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: `metrics` is a valid out-parameter.
        unsafe {
            if layout.GetMetrics(&mut metrics).is_err() {
                return (0.0, 0.0);
            }
        }
        (metrics.widthIncludingTrailingWhitespace, metrics.height)
    }

    /// Height of `text` wrapped to `width`.
    pub fn measure_wrapped(&self, text: &str, style: &TextStyle, width: f32) -> f32 {
        let style = TextStyle {
            wrap: true,
            middle: false,
            ..*style
        };
        let Some(layout) = self.layout(text, &style, width, 10_000.0) else {
            return 0.0;
        };
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: `metrics` is a valid out-parameter.
        unsafe {
            let _ = layout.GetMetrics(&mut metrics);
        }
        metrics.height
    }

    /// A parsed SVG path (cached by the address of its static text).
    fn path(&self, data: &'static str) -> Option<(ID2D1PathGeometry, D2D_RECT_F)> {
        let key = data.as_ptr() as usize;
        if let Some(path) = self.paths.borrow().get(&key) {
            return path.clone();
        }
        let built = self.build_path(data);
        self.paths.borrow_mut().insert(key, built.clone());
        built
    }

    fn build_path(&self, data: &str) -> Option<(ID2D1PathGeometry, D2D_RECT_F)> {
        let commands = svg::parse(data);
        // SAFETY: builds a path geometry through its sink, which is closed
        // before the geometry is used.
        unsafe {
            let geometry = self.d2d.CreatePathGeometry().ok()?;
            let sink = geometry.Open().ok()?;
            sink.SetFillMode(D2D1_FILL_MODE_WINDING);
            let mut open = false;
            let v = |(x, y): (f32, f32)| Vector2 { X: x, Y: y };
            for command in commands {
                match command {
                    svg::Segment::Move(p) => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        }
                        sink.BeginFigure(v(p), D2D1_FIGURE_BEGIN_FILLED);
                        open = true;
                    }
                    svg::Segment::Line(p) => sink.AddLine(v(p)),
                    svg::Segment::Cubic(a, b, p) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: v(a),
                        point2: v(b),
                        point3: v(p),
                    }),
                    svg::Segment::Arc {
                        radius,
                        rotation,
                        large,
                        sweep,
                        to,
                    } => sink.AddArc(&D2D1_ARC_SEGMENT {
                        point: v(to),
                        size: windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_F {
                            width: radius.0,
                            height: radius.1,
                        },
                        rotationAngle: rotation,
                        sweepDirection: if sweep {
                            D2D1_SWEEP_DIRECTION_CLOCKWISE
                        } else {
                            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                        },
                        arcSize: if large {
                            D2D1_ARC_SIZE_LARGE
                        } else {
                            D2D1_ARC_SIZE_SMALL
                        },
                    }),
                    svg::Segment::Close => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            open = false;
                        }
                    }
                }
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            sink.Close().ok()?;
            let bounds = geometry.GetBounds(None).ok()?;
            Some((geometry, bounds))
        }
    }
}

/// Decodes an encoded image (PNG, JPEG, ...) to premultiplied BGRA, scaled
/// down to at most `max` pixels on its longer side.
pub fn decode_image(bytes: &[u8], max: u32) -> Option<Rc<Image>> {
    // SAFETY: WIC objects are created, used and released on this thread
    // (COM is initialised by the app); `bytes` outlives the stream, which is
    // released before returning; the pixel buffer is sized for the copy.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = factory.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let natural = (w, h);
        let mut source: IWICBitmapSource = frame.cast().ok()?;
        if w.max(h) > max {
            let ratio = max as f32 / w.max(h) as f32;
            let (sw, sh) = (
                ((w as f32 * ratio).round() as u32).max(1),
                ((h as f32 * ratio).round() as u32).max(1),
            );
            let scaler = factory.CreateBitmapScaler().ok()?;
            scaler
                .Initialize(&source, sw, sh, WICBitmapInterpolationModeFant)
                .ok()?;
            source = scaler.cast().ok()?;
            (w, h) = (sw, sh);
        }
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        converter
            .CopyPixels(std::ptr::null(), w * 4, &mut pixels)
            .ok()?;
        Some(Image::with_natural(w, h, pixels, natural))
    }
}

/// The frames of an image file with several (an animated GIF), decoded
/// one at a time on demand so only the frame being shown is in memory.
pub struct Frames {
    factory: IWICImagingFactory,
    decoder: IWICBitmapDecoder,
    _stream: IWICStream,
    pub count: u32,
}

impl Frames {
    /// Opens `bytes` (kept for the program's life, as the stream reads
    /// from them).
    pub fn open(bytes: &'static [u8]) -> Option<Self> {
        // SAFETY: WIC objects are created and used on this thread (COM is
        // initialised by the app); `bytes` is static, so it outlives the
        // stream that reads it.
        unsafe {
            let factory: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            let stream = factory.CreateStream().ok()?;
            stream.InitializeFromMemory(bytes).ok()?;
            let decoder = factory
                .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)
                .ok()?;
            let count = decoder.GetFrameCount().ok()?;
            Some(Self {
                factory,
                decoder,
                _stream: stream,
                count,
            })
        }
    }

    /// Frame `index` as it is stored (for a GIF, its own rectangle, not
    /// composited), premultiplied BGRA: width, height, pixels.
    pub fn frame(&self, index: u32) -> Option<(u32, u32, Vec<u8>)> {
        // SAFETY: as in `open`; the pixel buffer is sized for the copy.
        unsafe {
            let frame = self.decoder.GetFrame(index).ok()?;
            let (mut w, mut h) = (0u32, 0u32);
            frame.GetSize(&mut w, &mut h).ok()?;
            if w == 0 || h == 0 {
                return None;
            }
            let source: IWICBitmapSource = frame.cast().ok()?;
            let converter = self.factory.CreateFormatConverter().ok()?;
            converter
                .Initialize(
                    &source,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .ok()?;
            let mut pixels = vec![0u8; (w * h * 4) as usize];
            converter
                .CopyPixels(std::ptr::null(), w * 4, &mut pixels)
                .ok()?;
            Some((w, h, pixels))
        }
    }
}

/// A layered window's backing store: a 32-bit DIB in a memory DC with a
/// Direct2D render target bound to it.
pub struct Surface {
    gfx: Rc<Gfx>,
    dc: HDC,
    bitmap: Option<(HBITMAP, HGDIOBJ, *mut c_void)>,
    size: (i32, i32),
    target: ID2D1DCRenderTarget,
    brush: ID2D1SolidColorBrush,
    images: RefCell<HashMap<u64, ID2D1Bitmap>>,
    /// Draws right to left (`set_mirrored`).
    mirrored: bool,
}

impl Surface {
    pub fn new(gfx: Rc<Gfx>) -> windows::core::Result<Self> {
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        // SAFETY: creates a memory DC and a render target for it; both are
        // released in Drop.
        unsafe {
            let target = gfx.d2d.CreateDCRenderTarget(&properties)?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let brush = target.CreateSolidColorBrush(&Rgba::WHITE.d2d(), None)?;
            let dc = CreateCompatibleDC(None);
            Ok(Self {
                gfx,
                dc,
                bitmap: None,
                size: (0, 0),
                target,
                brush,
                images: RefCell::default(),
                mirrored: false,
            })
        }
    }

    pub fn gfx(&self) -> &Gfx {
        &self.gfx
    }

    /// Mirrors what is drawn left to right, like WPF's
    /// `FlowDirection="RightToLeft"`: layout coordinates stay as they are
    /// and x runs from the right edge, while text and images keep their
    /// own direction (text then reads right to left).
    pub fn set_mirrored(&mut self, mirrored: bool) {
        self.mirrored = mirrored;
    }

    fn ensure(&mut self, width: i32, height: i32) -> bool {
        if self.size == (width, height) && self.bitmap.is_some() {
            return true;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: the previous DIB is deselected and deleted before the new
        // one replaces it; the DC is ours.
        unsafe {
            if let Some((bitmap, previous, _)) = self.bitmap.take() {
                SelectObject(self.dc, previous);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
            let mut bits = std::ptr::null_mut();
            let Ok(bitmap) =
                CreateDIBSection(Some(self.dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                return false;
            };
            let previous = SelectObject(self.dc, HGDIOBJ(bitmap.0));
            self.bitmap = Some((bitmap, previous, bits));
            self.size = (width, height);
            let rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            self.target.BindDC(self.dc, &rect).is_ok()
        }
    }

    /// Redraws the whole surface at `scale` (physical pixels per DIP) and
    /// size `width` x `height` physical pixels.
    pub fn draw(&mut self, width: i32, height: i32, scale: f32, paint: impl FnOnce(&mut Canvas)) {
        if width <= 0 || height <= 0 || !self.ensure(width, height) {
            return;
        }
        // SAFETY: drawing calls between BeginDraw and EndDraw on our target,
        // which is bound to the DIB selected into our DC.
        unsafe {
            self.target.SetDpi(96.0 * scale, 96.0 * scale);
            self.target.BeginDraw();
            self.target.Clear(Some(&Rgba::TRANSPARENT.d2d()));
            let base = if self.mirrored {
                Matrix3x2 {
                    M11: -1.0,
                    M31: width as f32 / scale,
                    ..identity()
                }
            } else {
                identity()
            };
            let mut canvas = Canvas {
                surface: self,
                transforms: vec![base],
                layers: 0,
            };
            canvas.apply();
            paint(&mut canvas);
            while canvas.layers > 0 {
                canvas.pop();
            }
            if self.target.EndDraw(None, None).is_err() {
                // The target was lost; rebind on the next draw.
                self.size = (0, 0);
                self.images.borrow_mut().clear();
            }
        }
    }

    /// Shows the surface in `hwnd` at screen position (`x`, `y`), with the
    /// whole window at `opacity`.
    pub fn present(&self, hwnd: isize, x: i32, y: i32, opacity: f32) {
        if self.bitmap.is_none() {
            return;
        }
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let position = POINT { x, y };
        let size = SIZE {
            cx: self.size.0,
            cy: self.size.1,
        };
        let origin = POINT { x: 0, y: 0 };
        // SAFETY: `hwnd` is a layered window of this process (a stale handle
        // only makes the call fail); the DC holds our DIB of `size`.
        unsafe {
            let _ = UpdateLayeredWindow(
                HWND(hwnd as *mut c_void),
                None,
                Some(&position),
                Some(&size),
                Some(self.dc),
                Some(&origin),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    /// Copies the surface into a normal window's client area (top-left
    /// aligned). Alpha is kept, so transparent pixels show a DWM backdrop
    /// extended into the client area.
    pub fn blit(&self, hwnd: isize) {
        if self.bitmap.is_none() {
            return;
        }
        let hwnd = HWND(hwnd as *mut c_void);
        // SAFETY: `hwnd` is a window of this process (a stale handle only
        // makes the calls fail); the window DC is released once and our DC
        // holds the DIB of `size`.
        unsafe {
            let dc = GetDC(Some(hwnd));
            if dc.is_invalid() {
                return;
            }
            let _ = BitBlt(
                dc,
                0,
                0,
                self.size.0,
                self.size.1,
                Some(self.dc),
                0,
                0,
                SRCCOPY,
            );
            ReleaseDC(Some(hwnd), dc);
        }
    }

    /// A copy of the last drawing as premultiplied BGRA (the `Image`
    /// layout), for effects computed once from a drawing.
    pub fn bgra(&self) -> Option<(u32, u32, Vec<u8>)> {
        let (_, _, bits) = self.bitmap?;
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        // SAFETY: `bits` is the DIB's w*h*4 byte buffer, alive while the
        // bitmap is selected (until `release` or the next resize).
        let pixels = unsafe { std::slice::from_raw_parts(bits as *const u8, w * h * 4) };
        Some((w as u32, h as u32, pixels.to_vec()))
    }

    /// The last drawing as straight-alpha RGBA (debug renders).
    pub fn rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        let (w, h, mut out) = self.bgra()?;
        for p in out.chunks_exact_mut(4) {
            let a = u16::from(p[3]);
            if a > 0 && a < 255 {
                for c in &mut p[..3] {
                    *c = ((u16::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
            p.swap(0, 2);
        }
        Some((w, h, out))
    }

    /// Releases the DIB (a hidden window keeps no pixels).
    pub fn release(&mut self) {
        // SAFETY: as in `ensure`.
        unsafe {
            if let Some((bitmap, previous, _)) = self.bitmap.take() {
                SelectObject(self.dc, previous);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
            }
        }
        self.size = (0, 0);
        self.images.borrow_mut().clear();
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.release();
        // SAFETY: the DC was created in `new` and is deleted once.
        unsafe {
            let _ = DeleteDC(self.dc);
        }
    }
}

fn identity() -> Matrix3x2 {
    Matrix3x2 {
        M11: 1.0,
        M12: 0.0,
        M21: 0.0,
        M22: 1.0,
        M31: 0.0,
        M32: 0.0,
    }
}

fn multiply(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}

/// Drawing operations on a surface during `Surface::draw`.
pub struct Canvas<'a> {
    surface: &'a Surface,
    transforms: Vec<Matrix3x2>,
    layers: usize,
}

impl Canvas<'_> {
    pub fn gfx(&self) -> &Gfx {
        &self.surface.gfx
    }

    fn brush(&self, color: Rgba) -> &ID2D1SolidColorBrush {
        // SAFETY: the brush belongs to this surface's target.
        unsafe { self.surface.brush.SetColor(&color.d2d()) };
        &self.surface.brush
    }

    fn apply(&self) {
        let top = self.transforms.last().copied().unwrap_or_else(identity);
        // SAFETY: sets the transform on the target being drawn.
        unsafe { self.surface.target.SetTransform(&top) };
    }

    /// Pushes a translation and scale about (`ox`, `oy`) onto the transform
    /// stack; pop with `pop_transform`.
    pub fn push_transform(&mut self, dx: f32, dy: f32, scale: f32, ox: f32, oy: f32) {
        let local = Matrix3x2 {
            M11: scale,
            M12: 0.0,
            M21: 0.0,
            M22: scale,
            M31: ox - ox * scale + dx,
            M32: oy - oy * scale + dy,
        };
        let top = self.transforms.last().copied().unwrap_or_else(identity);
        self.transforms.push(multiply(&local, &top));
        self.apply();
    }
    /// A rotation by `degrees` about (`cx`, `cy`).
    pub fn push_rotation(&mut self, degrees: f32, cx: f32, cy: f32) {
        let (s, c) = degrees.to_radians().sin_cos();
        let local = Matrix3x2 {
            M11: c,
            M12: s,
            M21: -s,
            M22: c,
            M31: cx - c * cx + s * cy,
            M32: cy - s * cx - c * cy,
        };
        let top = self.transforms.last().copied().unwrap_or_else(identity);
        self.transforms.push(multiply(&local, &top));
        self.apply();
    }
    pub fn pop_transform(&mut self) {
        if self.transforms.len() > 1 {
            self.transforms.pop();
        }
        self.apply();
    }

    /// Draws what follows at `opacity` (optionally clipped to a rounded
    /// rectangle) until `pop`.
    pub fn push_layer(&mut self, opacity: f32, clip: Option<(Rect, f32)>) {
        let mask: Option<ID2D1Geometry> = clip.and_then(|(rect, radius)| {
            // SAFETY: creates a device-independent geometry.
            unsafe {
                self.surface
                    .gfx
                    .d2d
                    .CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT {
                        rect: rect.d2d(),
                        radiusX: radius,
                        radiusY: radius,
                    })
                    .ok()
                    .and_then(|g| g.cast().ok())
            }
        });
        let parameters = D2D1_LAYER_PARAMETERS {
            contentBounds: D2D_RECT_F {
                left: f32::MIN,
                top: f32::MIN,
                right: f32::MAX,
                bottom: f32::MAX,
            },
            geometricMask: ManuallyDrop::new(mask),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: identity(),
            opacity,
            opacityBrush: ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS_NONE,
        };
        // SAFETY: a layer without a layer object (Windows 8+); the target
        // takes its own reference to the mask, which we release after.
        unsafe {
            self.surface.target.PushLayer(
                &parameters,
                None::<&windows::Win32::Graphics::Direct2D::ID2D1Layer>,
            );
        }
        drop(ManuallyDrop::into_inner(parameters.geometricMask));
        self.layers += 1;
    }
    /// Pops the last layer.
    pub fn pop(&mut self) {
        if self.layers > 0 {
            // SAFETY: matches a PushLayer on this target.
            unsafe { self.surface.target.PopLayer() };
            self.layers -= 1;
        }
    }

    pub fn rect(&mut self, rect: Rect, color: Rgba) {
        if color.a <= 0.0 {
            return;
        }
        let brush = self.brush(color);
        // SAFETY: draws with our brush on our target.
        unsafe { self.surface.target.FillRectangle(&rect.d2d(), brush) };
    }

    pub fn round_rect(&mut self, rect: Rect, radius: f32, color: Rgba) {
        if color.a <= 0.0 {
            return;
        }
        let brush = self.brush(color);
        let rounded = D2D1_ROUNDED_RECT {
            rect: rect.d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: as above.
        unsafe { self.surface.target.FillRoundedRectangle(&rounded, brush) };
    }

    pub fn stroke_round_rect(&mut self, rect: Rect, radius: f32, width: f32, color: Rgba) {
        if color.a <= 0.0 {
            return;
        }
        let brush = self.brush(color);
        let half = width / 2.0;
        let rounded = D2D1_ROUNDED_RECT {
            rect: rect.inset(half, half).d2d(),
            radiusX: (radius - half).max(0.0),
            radiusY: (radius - half).max(0.0),
        };
        // SAFETY: as above.
        unsafe {
            self.surface.target.DrawRoundedRectangle(
                &rounded,
                brush,
                width,
                None::<&windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle>,
            )
        };
    }

    pub fn stroke_rect(&mut self, rect: Rect, width: f32, color: Rgba) {
        self.stroke_round_rect(rect, 0.0, width, color);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: Rgba) {
        if color.a <= 0.0 {
            return;
        }
        let brush = self.brush(color);
        let ellipse = D2D1_ELLIPSE {
            point: Vector2 { X: cx, Y: cy },
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: as above.
        unsafe { self.surface.target.FillEllipse(&ellipse, brush) };
    }

    /// A horizontal (`vertical == false`) or vertical linear gradient.
    pub fn gradient(&mut self, rect: Rect, radius: f32, stops: &[(f32, Rgba)], vertical: bool) {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, color)| D2D1_GRADIENT_STOP {
                position: *position,
                color: color.d2d(),
            })
            .collect();
        let (start, end) = if vertical {
            ((rect.x, rect.y), (rect.x, rect.bottom()))
        } else {
            ((rect.x, rect.y), (rect.right(), rect.y))
        };
        // SAFETY: creates a transient gradient brush on our target and fills
        // with it.
        unsafe {
            let Ok(collection) = self.surface.target.CreateGradientStopCollection(
                &stops,
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            ) else {
                return;
            };
            let properties = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 {
                    X: start.0,
                    Y: start.1,
                },
                endPoint: Vector2 { X: end.0, Y: end.1 },
            };
            let Ok(brush) =
                self.surface
                    .target
                    .CreateLinearGradientBrush(&properties, None, &collection)
            else {
                return;
            };
            let rounded = D2D1_ROUNDED_RECT {
                rect: rect.d2d(),
                radiusX: radius,
                radiusY: radius,
            };
            self.surface.target.FillRoundedRectangle(&rounded, &brush);
        }
    }

    /// Fills `rect` (corner `radius`) with an elliptical radial gradient
    /// centred at (`cx`, `cy`) with radii `rx`, `ry` (WPF
    /// `RadialGradientBrush` with its origin at the centre).
    #[allow(clippy::too_many_arguments)]
    pub fn radial(
        &mut self,
        rect: Rect,
        radius: f32,
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        stops: &[(f32, Rgba)],
    ) {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|(position, color)| D2D1_GRADIENT_STOP {
                position: *position,
                color: color.d2d(),
            })
            .collect();
        // SAFETY: creates a transient gradient brush on our target and fills
        // with it.
        unsafe {
            let Ok(collection) = self.surface.target.CreateGradientStopCollection(
                &stops,
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            ) else {
                return;
            };
            let properties = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center: Vector2 { X: cx, Y: cy },
                gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
                radiusX: rx,
                radiusY: ry,
            };
            let Ok(brush) =
                self.surface
                    .target
                    .CreateRadialGradientBrush(&properties, None, &collection)
            else {
                return;
            };
            let rounded = D2D1_ROUNDED_RECT {
                rect: rect.d2d(),
                radiusX: radius,
                radiusY: radius,
            };
            self.surface.target.FillRoundedRectangle(&rounded, &brush);
        }
    }

    /// Whether the surface draws right to left (`Surface::set_mirrored`).
    pub fn mirrored(&self) -> bool {
        self.surface.mirrored
    }

    /// On a mirrored surface, flips `rect`'s contents back so text and
    /// images are not drawn mirror-written; pop with `pop_transform`.
    fn unmirror(&mut self, rect: Rect) -> bool {
        if !self.surface.mirrored {
            return false;
        }
        let local = Matrix3x2 {
            M11: -1.0,
            M31: 2.0 * rect.x + rect.w,
            ..identity()
        };
        let top = self.transforms.last().copied().unwrap_or_else(identity);
        self.transforms.push(multiply(&local, &top));
        self.apply();
        true
    }

    /// Draws `text` in `rect` with `style`.
    pub fn text(&mut self, text: &str, rect: Rect, style: &TextStyle, color: Rgba) {
        if text.is_empty() || color.a <= 0.0 {
            return;
        }
        let style = &TextStyle {
            rtl: style.rtl || self.surface.mirrored,
            ..*style
        };
        let Some(layout) = self.surface.gfx.layout(text, style, rect.w, rect.h) else {
            return;
        };
        let flipped = self.unmirror(rect);
        let brush = self.brush(color);
        // SAFETY: draws a layout we created with our brush.
        unsafe {
            self.surface.target.DrawTextLayout(
                Vector2 {
                    X: rect.x,
                    Y: rect.y,
                },
                &layout,
                brush,
                if style.ellipsis {
                    D2D1_DRAW_TEXT_OPTIONS_CLIP
                } else {
                    Default::default()
                },
            )
        };
        if flipped {
            self.pop_transform();
        }
    }

    /// Fills an SVG path scaled uniformly to fit `rect` by its own bounds
    /// (WPF `Stretch="Uniform"`).
    pub fn path(&mut self, data: &'static str, rect: Rect, color: Rgba) {
        let Some((geometry, bounds)) = self.surface.gfx.path(data) else {
            return;
        };
        let (bw, bh) = (bounds.right - bounds.left, bounds.bottom - bounds.top);
        if bw <= 0.0 || bh <= 0.0 {
            return;
        }
        let scale = (rect.w / bw).min(rect.h / bh);
        let dx = rect.x + (rect.w - bw * scale) / 2.0 - bounds.left * scale;
        let dy = rect.y + (rect.h - bh * scale) / 2.0 - bounds.top * scale;
        let local = Matrix3x2 {
            M11: scale,
            M12: 0.0,
            M21: 0.0,
            M22: scale,
            M31: dx,
            M32: dy,
        };
        let top = self.transforms.last().copied().unwrap_or_else(identity);
        self.transforms.push(multiply(&local, &top));
        self.apply();
        let brush = self.brush(color);
        // SAFETY: fills a geometry we built with our brush.
        unsafe {
            self.surface.target.FillGeometry(
                &geometry,
                brush,
                None::<&windows::Win32::Graphics::Direct2D::ID2D1Brush>,
            )
        };
        self.pop_transform();
    }

    /// Draws an image stretched to `rect`.
    pub fn image(&mut self, image: &Image, rect: Rect, opacity: f32) {
        self.image_part(image, rect, None, opacity);
    }

    /// Draws an image filling `rect`, cropped to keep its aspect ratio
    /// (CSS `object-fit: cover`).
    pub fn image_cover(&mut self, image: &Image, rect: Rect, opacity: f32) {
        let (iw, ih) = (image.width as f32, image.height as f32);
        if iw <= 0.0 || ih <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let scale = (rect.w / iw).max(rect.h / ih);
        let (sw, sh) = (rect.w / scale, rect.h / scale);
        let source = Rect::new((iw - sw) / 2.0, (ih - sh) / 2.0, sw, sh);
        self.image_part(image, rect, Some(source), opacity);
    }

    fn image_part(&mut self, image: &Image, rect: Rect, source: Option<Rect>, opacity: f32) {
        let bitmap = {
            let mut images = self.surface.images.borrow_mut();
            match images.get(&image.id) {
                Some(bitmap) => bitmap.clone(),
                None => {
                    let properties = D2D1_BITMAP_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.0,
                        dpiY: 96.0,
                    };
                    // SAFETY: `pixels` holds width*height BGRA pixels with
                    // the given pitch; the bitmap copies them.
                    let Ok(bitmap) = (unsafe {
                        self.surface.target.CreateBitmap(
                            D2D_SIZE_U {
                                width: image.width,
                                height: image.height,
                            },
                            Some(image.pixels.as_ptr().cast()),
                            image.width * 4,
                            &properties,
                        )
                    }) else {
                        return;
                    };
                    if images.len() > 96 {
                        images.clear();
                    }
                    images.insert(image.id, bitmap.clone());
                    bitmap
                }
            }
        };
        let flipped = self.unmirror(rect);
        // SAFETY: draws a bitmap created on our target.
        unsafe {
            self.surface.target.DrawBitmap(
                &bitmap,
                Some(&rect.d2d()),
                opacity,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                source.map(|s| s.d2d()).as_ref().map(|s| s as *const _),
            )
        };
        if flipped {
            self.pop_transform();
        }
    }
}

/// A small SVG path-data parser (M L H V C S Q T A Z, absolute and
/// relative), enough for the original's Phosphor icons.
mod svg {
    pub enum Segment {
        Move((f32, f32)),
        Line((f32, f32)),
        Cubic((f32, f32), (f32, f32), (f32, f32)),
        Arc {
            radius: (f32, f32),
            rotation: f32,
            large: bool,
            sweep: bool,
            to: (f32, f32),
        },
        Close,
    }

    struct Reader<'a> {
        s: &'a [u8],
        i: usize,
    }

    impl Reader<'_> {
        fn skip(&mut self) {
            while self.i < self.s.len()
                && matches!(self.s[self.i], b' ' | b',' | b'\n' | b'\r' | b'\t')
            {
                self.i += 1;
            }
        }
        fn command(&mut self) -> Option<u8> {
            self.skip();
            let c = *self.s.get(self.i)?;
            if c.is_ascii_alphabetic() {
                self.i += 1;
                Some(c)
            } else {
                None
            }
        }
        fn at_number(&mut self) -> bool {
            self.skip();
            self.s
                .get(self.i)
                .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
        }
        fn number(&mut self) -> Option<f32> {
            self.skip();
            let start = self.i;
            let mut seen_dot = false;
            let mut seen_exp = false;
            if matches!(self.s.get(self.i), Some(b'-' | b'+')) {
                self.i += 1;
            }
            while let Some(&c) = self.s.get(self.i) {
                match c {
                    b'0'..=b'9' => {}
                    b'.' if !seen_dot && !seen_exp => seen_dot = true,
                    b'e' | b'E' if !seen_exp => {
                        seen_exp = true;
                        if matches!(self.s.get(self.i + 1), Some(b'-' | b'+')) {
                            self.i += 1;
                        }
                    }
                    _ => break,
                }
                self.i += 1;
            }
            std::str::from_utf8(&self.s[start..self.i])
                .ok()?
                .parse()
                .ok()
        }
        fn flag(&mut self) -> Option<bool> {
            self.skip();
            let c = *self.s.get(self.i)?;
            self.i += 1;
            Some(c == b'1')
        }
    }

    pub fn parse(data: &str) -> Vec<Segment> {
        let mut out = vec![];
        let mut r = Reader {
            s: data.as_bytes(),
            i: 0,
        };
        let (mut x, mut y) = (0.0f32, 0.0f32);
        let (mut sx, mut sy) = (0.0f32, 0.0f32);
        // Last control point for S/T reflection.
        let mut last_cubic: Option<(f32, f32)> = None;
        let mut last_quad: Option<(f32, f32)> = None;
        let mut command = 0u8;
        loop {
            if let Some(c) = r.command() {
                command = c;
            } else if !r.at_number() {
                break;
            } else if command == b'M' {
                command = b'L';
            } else if command == b'm' {
                command = b'l';
            }
            let relative = command.is_ascii_lowercase();
            let (bx, by) = if relative { (x, y) } else { (0.0, 0.0) };
            let point = |r: &mut Reader| -> Option<(f32, f32)> {
                Some((r.number()? + bx, r.number()? + by))
            };
            let upper = command.to_ascii_uppercase();
            let mut cubic = None;
            let mut quad = None;
            match upper {
                b'M' => {
                    let Some(p) = point(&mut r) else { break };
                    (x, y) = p;
                    (sx, sy) = p;
                    out.push(Segment::Move(p));
                }
                b'L' => {
                    let Some(p) = point(&mut r) else { break };
                    (x, y) = p;
                    out.push(Segment::Line(p));
                }
                b'H' => {
                    let Some(v) = r.number() else { break };
                    x = v + bx;
                    out.push(Segment::Line((x, y)));
                }
                b'V' => {
                    let Some(v) = r.number() else { break };
                    y = v + by;
                    out.push(Segment::Line((x, y)));
                }
                b'C' => {
                    let (Some(a), Some(b), Some(p)) = (point(&mut r), point(&mut r), point(&mut r))
                    else {
                        break;
                    };
                    out.push(Segment::Cubic(a, b, p));
                    cubic = Some(b);
                    (x, y) = p;
                }
                b'S' => {
                    let (Some(b), Some(p)) = (point(&mut r), point(&mut r)) else {
                        break;
                    };
                    let a = last_cubic.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy));
                    out.push(Segment::Cubic(a, b, p));
                    cubic = Some(b);
                    (x, y) = p;
                }
                b'Q' | b'T' => {
                    let control = if upper == b'Q' {
                        let Some(c) = point(&mut r) else { break };
                        c
                    } else {
                        last_quad.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy))
                    };
                    let Some(p) = point(&mut r) else { break };
                    // As a cubic: control points 2/3 of the way to the
                    // quadratic control point.
                    let a = (
                        x + 2.0 / 3.0 * (control.0 - x),
                        y + 2.0 / 3.0 * (control.1 - y),
                    );
                    let b = (
                        p.0 + 2.0 / 3.0 * (control.0 - p.0),
                        p.1 + 2.0 / 3.0 * (control.1 - p.1),
                    );
                    out.push(Segment::Cubic(a, b, p));
                    quad = Some(control);
                    (x, y) = p;
                }
                b'A' => {
                    let (Some(rx), Some(ry), Some(rotation), Some(large), Some(sweep)) =
                        (r.number(), r.number(), r.number(), r.flag(), r.flag())
                    else {
                        break;
                    };
                    let Some(p) = point(&mut r) else { break };
                    out.push(Segment::Arc {
                        radius: (rx.abs(), ry.abs()),
                        rotation,
                        large,
                        sweep,
                        to: p,
                    });
                    (x, y) = p;
                }
                b'Z' => {
                    out.push(Segment::Close);
                    (x, y) = (sx, sy);
                }
                _ => break,
            }
            last_cubic = cubic;
            last_quad = quad;
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_phosphor_commands() {
            let segments = parse(
                "M234.49,111.07,90.41,22.94A20,20,0,0,0,60,39.87V216.13a20,20,0,0,0,30.41,16.93Z",
            );
            assert!(matches!(segments[0], Segment::Move((x, y)) if x == 234.49 && y == 111.07));
            assert!(matches!(segments[1], Segment::Line((x, _)) if x == 90.41));
            assert!(matches!(segments[2], Segment::Arc { to: (x, _), .. } if x == 60.0));
            assert!(matches!(segments[3], Segment::Line((x, y)) if x == 60.0 && y == 216.13));
            assert!(
                matches!(segments[4], Segment::Arc { to: (x, _), .. } if (x - 90.41).abs() < 1e-3)
            );
            assert!(matches!(segments[5], Segment::Close));
            assert_eq!(parse("M1-2l3.5.5").len(), 2);
        }
    }
}
