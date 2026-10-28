//! The volume tick sound (UseVolumeTickSound), played at the new volume
//! like the original. PlaySound plays from memory asynchronously, so each
//! loudness level keeps its own buffer for the life of the program.
use std::{collections::HashMap, sync::Mutex};
use windows::{
    Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT},
    core::PCWSTR,
};

static BUFFERS: Mutex<Option<HashMap<u8, &'static [u8]>>> = Mutex::new(None);

/// The 16-bit PCM WAV `wav` scaled to `level` (0..20 of 20).
fn scaled(wav: &[u8], level: u8) -> Vec<u8> {
    let mut out = wav.to_vec();
    // Find the "data" chunk.
    let mut at = 12;
    while at + 8 <= out.len() {
        let size =
            u32::from_le_bytes([out[at + 4], out[at + 5], out[at + 6], out[at + 7]]) as usize;
        if &out[at..at + 4] == b"data" {
            let end = (at + 8 + size).min(out.len());
            let gain = f32::from(level) / 20.0;
            for sample in out[at + 8..end].chunks_exact_mut(2) {
                let v = i16::from_le_bytes([sample[0], sample[1]]);
                let s = (f32::from(v) * gain).round() as i16;
                sample.copy_from_slice(&s.to_le_bytes());
            }
            break;
        }
        at += 8 + size + (size & 1);
    }
    out
}

/// Plays `wav` (16-bit PCM) at `volume` (0..=1, at least 0.1).
pub fn play(wav: &'static [u8], volume: f32) {
    let level = (volume.clamp(0.1, 1.0) * 20.0).round() as u8;
    let buffer = {
        let Ok(mut buffers) = BUFFERS.lock() else {
            return;
        };
        let map = buffers.get_or_insert_with(HashMap::new);
        *map.entry(level)
            .or_insert_with(|| Box::leak(scaled(wav, level).into_boxed_slice()))
    };
    // SAFETY: `buffer` is a leaked, immutable WAV image that lives for the
    // rest of the program, as asynchronous memory playback requires.
    unsafe {
        let _ = PlaySoundW(
            PCWSTR(buffer.as_ptr().cast()),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}
