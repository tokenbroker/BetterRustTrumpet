//! Executable icons for the flyout.
use windows::{
    Win32::{
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
            DIB_RGB_COLORS, DeleteDC, DeleteObject, HGDIOBJ, SelectObject,
        },
        UI::{
            Shell::SHDefExtractIconW,
            WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON},
        },
    },
    core::PCWSTR,
};

/// Renders `icon` into a 32-bit top-down DIB over a solid background and
/// returns BGRA pixels.
pub(super) fn render(icon: HICON, size: u32, background: u8) -> Option<Vec<u8>> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: the DC and DIB are created, selected and released in balanced
    // order; `bits` points at size*size*4 bytes owned by the DIB, read only
    // while it is alive; `icon` is a valid icon owned by the caller.
    unsafe {
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            return None;
        };
        let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
        let length = (size * size * 4) as usize;
        std::ptr::write_bytes(bits as *mut u8, background, length);
        let drawn = DrawIconEx(dc, 0, 0, icon, size as i32, size as i32, 0, None, DI_NORMAL);
        let pixels = drawn
            .is_ok()
            .then(|| std::slice::from_raw_parts(bits as *const u8, length).to_vec());
        SelectObject(dc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
        pixels
    }
}

/// RGBA pixels of the executable's main icon at `size` pixels.
fn rgba(path: &str, size: u32) -> Option<Vec<u8>> {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let mut icon = HICON::default();
    // SAFETY: `wide` is NUL-terminated and outlives the call; the large-icon
    // out-parameter receives an icon we destroy below.
    unsafe {
        SHDefExtractIconW(PCWSTR(wide.as_ptr()), 0, 0, Some(&mut icon), None, size)
            .ok()
            .ok()?;
    }
    if icon.is_invalid() {
        return None;
    }
    let mut pixels = render(icon, size, 0);
    // Icons without an alpha channel: derive coverage by drawing on black and
    // on white; unchanged pixels are opaque.
    if let Some(dark) = &mut pixels
        && dark.as_chunks::<4>().0.iter().all(|p| p[3] == 0)
        && let Some(light) = render(icon, size, 255)
    {
        for (d, l) in dark
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(light.as_chunks::<4>().0)
        {
            d[3] = if d[..3] == l[..3] { 255 } else { 0 };
        }
    }
    // SAFETY: the icon came from SHDefExtractIconW and is destroyed once.
    unsafe {
        let _ = DestroyIcon(icon);
    }
    let mut pixels = pixels?;
    for p in pixels.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Some(pixels)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}
/// Minimal RGBA PNG with stored (uncompressed) deflate blocks.
fn png(rgba: &[u8], size: u32) -> Vec<u8> {
    png_rect(rgba, size, size)
}

/// A `width` x `height` RGBA PNG (uncompressed), for debug renders.
pub fn png_rect(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks_exact(width as usize * 4) {
        raw.push(0);
        raw.extend(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (i, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend(len.to_le_bytes());
        zlib.extend((!len).to_le_bytes());
        zlib.extend(*block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = vec![];
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let n = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= group.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The executable's icon as straight-alpha BGRA pixels, `size` square.
pub fn bgra(path: &str, size: u32) -> Option<Vec<u8>> {
    let mut pixels = rgba(path, size)?;
    for p in pixels.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    Some(pixels)
}

/// The executable's icon as an RGBA PNG, `size` square (a media player's
/// icon standing in for missing cover art).
pub fn png_file_icon(path: &str, size: u32) -> Option<Vec<u8>> {
    rgba(path, size).map(|pixels| png(&pixels, size))
}

/// `data:image/png;base64,...` for the executable's icon, if it has one.
pub fn data_url(path: &str, size: u32) -> Option<String> {
    let pixels = rgba(path, size)?;
    Some(format!(
        "data:image/png;base64,{}",
        base64(&png(&pixels, size))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_base64_and_png_structure() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        let image = png(&[255, 0, 0, 255], 1);
        assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(image.ends_with(&[0xae, 0x42, 0x60, 0x82]));
    }

    #[test]
    fn extracts_icons_from_system_executables() {
        let windir = std::env::var("WINDIR").unwrap();
        let url = data_url(&format!(r"{windir}\explorer.exe"), 32).unwrap();
        assert!(url.starts_with("data:image/png;base64,iVBORw0KGgo"));
        let pixels = rgba(&format!(r"{windir}\explorer.exe"), 32).unwrap();
        assert!(
            pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0),
            "icon is fully transparent"
        );
        assert!(data_url(r"C:\definitely\missing.exe", 32).is_none());
    }
}
