//! An animated GIF played like XamlAnimatedGif (the original's
//! `gif:AnimationBehavior`, repeating forever): the file's frame records
//! are read here, WIC decodes one frame at a time and the frames are
//! composited onto one canvas with each frame's disposal method. Only the
//! canvas and the frame being decoded are in memory.
use bt_app::win32::gfx::{Frames, Image};
use std::rc::Rc;

/// One frame's placement and timing from its image descriptor and
/// graphic control extension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameInfo {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    /// Hundredths of a second, as stored.
    pub delay: u16,
    /// 0/1 keep, 2 clear to transparent, 3 restore the previous canvas.
    pub disposal: u8,
}

impl FrameInfo {
    /// The delay in milliseconds; 0 and 1 (as browsers and
    /// XamlAnimatedGif treat them) mean 100 ms.
    pub fn delay_ms(&self) -> u32 {
        if self.delay <= 1 {
            100
        } else {
            u32::from(self.delay) * 10
        }
    }
}

/// The logical screen size and every frame's record, or None for a file
/// that is not a GIF.
pub fn parse(bytes: &[u8]) -> Option<((u32, u32), Vec<FrameInfo>)> {
    if bytes.len() < 13 || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return None;
    }
    let u16_at = |i: usize| -> Option<u16> {
        Some(u16::from_le_bytes([*bytes.get(i)?, *bytes.get(i + 1)?]))
    };
    let size = (u32::from(u16_at(6)?), u32::from(u16_at(8)?));
    let packed = bytes[10];
    let mut i = 13;
    if packed & 0x80 != 0 {
        i += 3 << ((packed & 7) + 1);
    }
    // Skips data sub-blocks from `i` (a length byte, then that many bytes,
    // until a zero length).
    let skip_blocks = |mut i: usize| -> Option<usize> {
        loop {
            let len = usize::from(*bytes.get(i)?);
            i += 1;
            if len == 0 {
                return Some(i);
            }
            i += len;
        }
    };
    let mut frames = vec![];
    let mut control = (0u16, 0u8);
    loop {
        match *bytes.get(i)? {
            0x21 => {
                let label = *bytes.get(i + 1)?;
                if label == 0xf9 && *bytes.get(i + 2)? == 4 {
                    let flags = *bytes.get(i + 3)?;
                    control = (u16_at(i + 4)?, (flags >> 2) & 7);
                }
                i = skip_blocks(i + 2)?;
            }
            0x2c => {
                let info = FrameInfo {
                    left: u32::from(u16_at(i + 1)?),
                    top: u32::from(u16_at(i + 3)?),
                    width: u32::from(u16_at(i + 5)?),
                    height: u32::from(u16_at(i + 7)?),
                    delay: control.0,
                    disposal: control.1,
                };
                let flags = *bytes.get(i + 9)?;
                i += 10;
                if flags & 0x80 != 0 {
                    i += 3 << ((flags & 7) + 1);
                }
                // The LZW minimum code size, then the image data.
                i = skip_blocks(i + 1)?;
                frames.push(info);
                control = (0, 0);
            }
            0x3b => return Some((size, frames)),
            _ => return (!frames.is_empty()).then_some((size, frames)),
        }
    }
}

pub struct Animation {
    frames: Frames,
    info: Vec<FrameInfo>,
    size: (u32, u32),
    canvas: Vec<u8>,
    /// The canvas before a frame that restores it (disposal 3).
    saved: Option<Vec<u8>>,
    /// The frame drawn last, whose disposal applies before the next.
    last: Option<FrameInfo>,
    next: usize,
}

impl Animation {
    pub fn new(bytes: &'static [u8]) -> Option<Self> {
        let (size, mut info) = parse(bytes)?;
        let frames = Frames::open(bytes)?;
        info.truncate(frames.count as usize);
        if info.is_empty() || size.0 == 0 || size.1 == 0 {
            return None;
        }
        Some(Self {
            frames,
            info,
            size,
            canvas: vec![0; (size.0 * size.1 * 4) as usize],
            saved: None,
            last: None,
            next: 0,
        })
    }

    /// Draws the next frame and returns the picture and how long it shows.
    pub fn step(&mut self) -> (Rc<Image>, u32) {
        let (w, h) = self.size;
        if let Some(last) = self.last.take() {
            match last.disposal {
                2 => self.clear(last),
                3 => {
                    if let Some(saved) = self.saved.take() {
                        self.canvas = saved;
                    }
                }
                _ => {}
            }
        }
        if self.next == 0 {
            self.canvas.fill(0);
        }
        let info = self.info[self.next];
        if info.disposal == 3 {
            self.saved = Some(self.canvas.clone());
        }
        if let Some((fw, fh, pixels)) = self.frames.frame(self.next as u32) {
            composite(
                &mut self.canvas,
                (w, h),
                &pixels,
                (fw, fh),
                (info.left, info.top),
            );
        }
        self.last = Some(info);
        self.next = (self.next + 1) % self.info.len();
        (Image::new(w, h, self.canvas.clone()), info.delay_ms())
    }

    fn clear(&mut self, info: FrameInfo) {
        let (w, h) = self.size;
        for y in info.top..(info.top + info.height).min(h) {
            let from = ((y * w + info.left.min(w)) * 4) as usize;
            let to = ((y * w + (info.left + info.width).min(w)) * 4) as usize;
            self.canvas[from..to].fill(0);
        }
    }
}

/// Draws premultiplied BGRA `pixels` over `canvas` at `at`, clipped.
fn composite(
    canvas: &mut [u8],
    size: (u32, u32),
    pixels: &[u8],
    frame: (u32, u32),
    at: (u32, u32),
) {
    let (w, h) = size;
    for y in 0..frame.1 {
        let cy = at.1 + y;
        if cy >= h {
            break;
        }
        for x in 0..frame.0 {
            let cx = at.0 + x;
            if cx >= w {
                break;
            }
            let s = ((y * frame.0 + x) * 4) as usize;
            let d = ((cy * w + cx) * 4) as usize;
            let a = u16::from(pixels[s + 3]);
            if a == 255 {
                canvas[d..d + 4].copy_from_slice(&pixels[s..s + 4]);
            } else if a > 0 {
                for c in 0..4 {
                    let under = u16::from(canvas[d + c]) * (255 - a) / 255;
                    canvas[d + c] = (u16::from(pixels[s + c]) + under).min(255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tray_pin_animation_parses() {
        let (size, frames) = parse(super::super::onboarding::TRAY_PIN).unwrap();
        assert_eq!(size, (576, 542));
        assert_eq!(frames.len(), 144);
        assert!(
            frames
                .iter()
                .all(|f| f.left + f.width <= 576 && f.top + f.height <= 542)
        );
    }

    #[test]
    fn delays_and_disposal_come_from_the_control_extension() {
        // A 2x1 GIF with one frame: delay 7, disposal 2.
        let bytes = [
            b'G',
            b'I',
            b'F',
            b'8',
            b'9',
            b'a',
            2,
            0,
            1,
            0,
            0,
            0,
            0, // screen
            0x21,
            0xf9,
            4,
            2 << 2,
            7,
            0,
            0,
            0, // control
            0x2c,
            1,
            0,
            0,
            0,
            1,
            0,
            1,
            0,
            0, // descriptor
            2,
            2,
            0x44,
            0x01,
            0, // image data
            0x3b,
        ];
        let (size, frames) = parse(&bytes).unwrap();
        assert_eq!(size, (2, 1));
        assert_eq!(
            frames,
            vec![FrameInfo {
                left: 1,
                top: 0,
                width: 1,
                height: 1,
                delay: 7,
                disposal: 2
            }]
        );
        assert_eq!(frames[0].delay_ms(), 70);
        assert!(parse(b"PNG").is_none());
    }

    #[test]
    fn composite_blends_premultiplied_pixels() {
        let mut canvas = vec![0, 0, 200, 255];
        composite(&mut canvas, (1, 1), &[100, 0, 0, 128], (1, 1), (0, 0));
        assert_eq!(canvas, vec![100, 0, 99, 255]);
    }
}
