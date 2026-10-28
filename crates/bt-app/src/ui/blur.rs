//! Software blur for the few places the original uses WPF's BlurEffect:
//! the flyout content behind an app or device popup (radius 10) and
//! low-resolution cover art in the media popup (radius 2). Each blur runs
//! once (when the popup opens or the art changes), never per frame, on
//! premultiplied BGRA pixels: three box passes per axis approximate a
//! Gaussian, with edges clamped.
use bt_app::win32::gfx::Image;
use std::rc::Rc;

/// WPF's Gaussian BlurEffect `Radius` as a standard deviation (its kernel
/// spans about three deviations each side).
pub fn sigma_for_radius(radius: f32) -> f32 {
    radius / 3.0
}

/// Widths of three box filters whose combination approximates a Gaussian
/// of deviation `sigma` (pixels).
fn boxes(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut lower = ideal.floor() as i32;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let upper = lower + 2;
    let lf = lower as f32;
    let m = ((12.0 * sigma * sigma - n * lf * lf - 4.0 * n * lf - 3.0 * n) / (-4.0 * lf - 4.0))
        .round() as i32;
    let mut out = [0usize; 3];
    for (i, b) in out.iter_mut().enumerate() {
        *b = if (i as i32) < m { lower } else { upper } as usize;
    }
    out
}

/// One box pass along rows (`stride` 4, `step` 4) or columns, `radius`
/// pixels each side, edges clamped. `len` samples per line, `lines` lines.
#[allow(clippy::too_many_arguments)]
fn box_pass(
    src: &[u8],
    dst: &mut [u8],
    len: usize,
    lines: usize,
    step: usize,
    line_step: usize,
    radius: usize,
) {
    if radius == 0 || len == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let width = (2 * radius + 1) as u32;
    let last = len - 1;
    for line in 0..lines {
        let base = line * line_step;
        let at = |i: usize| base + i * step;
        for c in 0..4 {
            // Window centred on sample 0: `radius` copies of the first
            // sample on the left, then samples 0..=radius (clamped).
            let mut sum = (radius as u32 + 1) * u32::from(src[at(0) + c]);
            for i in 1..=radius {
                sum += u32::from(src[at(i.min(last)) + c]);
            }
            for i in 0..len {
                dst[at(i) + c] = ((sum + width / 2) / width) as u8;
                let add = (i + radius + 1).min(last);
                let remove = i.saturating_sub(radius);
                sum += u32::from(src[at(add) + c]);
                sum -= u32::from(src[at(remove) + c]);
            }
        }
    }
}

/// Blurs premultiplied BGRA pixels in place.
pub fn gaussian(pixels: &mut [u8], width: usize, height: usize, sigma: f32) {
    if width == 0 || height == 0 || sigma < 0.2 || pixels.len() < width * height * 4 {
        return;
    }
    let pixels = &mut pixels[..width * height * 4];
    let mut scratch = vec![0u8; pixels.len()];
    for size in boxes(sigma) {
        let radius = (size - 1) / 2;
        box_pass(pixels, &mut scratch, width, height, 4, width * 4, radius);
        box_pass(&scratch, pixels, height, width, width * 4, 4, radius);
    }
}

/// Halves premultiplied BGRA pixels (2x2 averages), for blurring large
/// areas at a quarter of the cost; the result is drawn stretched back.
pub fn half(pixels: &[u8], width: usize, height: usize) -> (Vec<u8>, usize, usize) {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let sample = |sx: usize, sy: usize| {
                    let (sx, sy) = (sx.min(width - 1), sy.min(height - 1));
                    u32::from(pixels[(sy * width + sx) * 4 + c])
                };
                let sum = sample(2 * x, 2 * y)
                    + sample(2 * x + 1, 2 * y)
                    + sample(2 * x, 2 * y + 1)
                    + sample(2 * x + 1, 2 * y + 1);
                out[(y * w + x) * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    (out, w, h)
}

/// Scales premultiplied BGRA pixels to `to_w` x `to_h` (bilinear).
pub fn resize(pixels: &[u8], width: usize, height: usize, to_w: usize, to_h: usize) -> Vec<u8> {
    let mut out = vec![0u8; to_w * to_h * 4];
    if width == 0 || height == 0 {
        return out;
    }
    for y in 0..to_h {
        let fy = ((y as f32 + 0.5) * height as f32 / to_h as f32 - 0.5).max(0.0);
        let y0 = (fy as usize).min(height - 1);
        let y1 = (y0 + 1).min(height - 1);
        let ty = fy - y0 as f32;
        for x in 0..to_w {
            let fx = ((x as f32 + 0.5) * width as f32 / to_w as f32 - 0.5).max(0.0);
            let x0 = (fx as usize).min(width - 1);
            let x1 = (x0 + 1).min(width - 1);
            let tx = fx - x0 as f32;
            for c in 0..4 {
                let p = |sx: usize, sy: usize| f32::from(pixels[(sy * width + sx) * 4 + c]);
                let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * tx;
                let bottom = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * tx;
                out[(y * to_w + x) * 4 + c] = (top + (bottom - top) * ty).round() as u8;
            }
        }
    }
    out
}

/// The original's low-resolution cover art (under 300 px on its longer
/// side): shown through BlurEffect radius 2 at its 200 DIP size. The art is
/// scaled to 400 px (2 px per DIP) and blurred there; larger art is
/// returned unchanged.
pub fn soften_art(art: Rc<Image>) -> Rc<Image> {
    let (nw, nh) = art.natural;
    if nw.max(nh) >= 300 || art.width == 0 || art.height == 0 {
        return art;
    }
    let ratio = 400.0 / art.width.max(art.height) as f32;
    let to_w = ((art.width as f32 * ratio).round() as usize).max(1);
    let to_h = ((art.height as f32 * ratio).round() as usize).max(1);
    let mut pixels = resize(
        &art.pixels,
        art.width as usize,
        art.height as usize,
        to_w,
        to_h,
    );
    gaussian(&mut pixels, to_w, to_h, sigma_for_radius(2.0) * 2.0);
    Image::new(to_w as u32, to_h as u32, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, px: [u8; 4]) -> Vec<u8> {
        px.iter().copied().cycle().take(w * h * 4).collect()
    }

    #[test]
    fn boxes_grow_with_sigma() {
        assert_eq!(boxes(1.0), [1, 1, 3]);
        let b = boxes(5.0);
        assert!(b.iter().all(|s| s % 2 == 1));
        assert!(b[2] >= b[0]);
    }

    #[test]
    fn uniform_image_is_unchanged() {
        let mut p = solid(17, 9, [10, 20, 30, 200]);
        let before = p.clone();
        gaussian(&mut p, 17, 9, 3.0);
        assert_eq!(p, before);
    }

    #[test]
    fn spreads_a_point_and_keeps_its_energy() {
        let (w, h) = (31, 31);
        let mut p = vec![0u8; w * h * 4];
        let centre = (15 * w + 15) * 4;
        p[centre..centre + 4].copy_from_slice(&[255, 255, 255, 255]);
        // A wider point so rounding does not swallow it.
        for dy in 0..3 {
            for dx in 0..3 {
                let i = ((14 + dy) * w + 14 + dx) * 4;
                p[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        gaussian(&mut p, w, h, 2.0);
        let alpha = |x: usize, y: usize| p[(y * w + x) * 4 + 3];
        assert!(alpha(15, 15) < 255);
        assert!(alpha(15, 15) > alpha(18, 15));
        assert!(alpha(18, 15) > 0);
        assert_eq!(alpha(0, 0), 0);
        let total: u32 = (0..w * h).map(|i| u32::from(p[i * 4 + 3])).sum();
        let expected = 9 * 255;
        assert!(total.abs_diff(expected) < expected / 10, "{total}");
    }

    #[test]
    fn half_and_resize_keep_flat_colour() {
        let p = solid(8, 6, [1, 2, 3, 4]);
        let (h, w2, h2) = half(&p, 8, 6);
        assert_eq!((w2, h2), (4, 3));
        assert!(h.chunks(4).all(|c| c == [1, 2, 3, 4]));
        let r = resize(&p, 8, 6, 20, 15);
        assert!(r.chunks(4).all(|c| c == [1, 2, 3, 4]));
    }

    #[test]
    fn large_art_is_not_softened() {
        let art = Image::new(2, 2, solid(2, 2, [0, 0, 0, 255]));
        let big = Rc::new(Image {
            id: art.id,
            width: 2,
            height: 2,
            pixels: art.pixels.clone(),
            natural: (640, 640),
        });
        assert!(Rc::ptr_eq(&soften_art(big.clone()), &big));
        let small = soften_art(art);
        assert_eq!((small.width, small.height), (400, 400));
    }
}
