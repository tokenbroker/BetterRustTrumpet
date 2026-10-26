//! FocusLostVolumePolicy, FadePolicy and Supervisor without Win32 polling.
use crate::{identity, text::is_blank};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Mode(pub i32);
impl Mode {
    pub const OFF: Self = Self(0);
    pub const MUTE: Self = Self(1);
    pub const ATTENUATE: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub volume: i32,
    pub is_muted: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Session {
    pub key: Option<String>,
    pub process_id: i32,
    pub volume: i32,
    pub is_muted: bool,
    pub can_adjust: bool,
    pub executable_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Adjustment {
    pub key: String,
    pub volume: i32,
    pub is_muted: bool,
}

pub fn resolve_mode(enabled: bool, percent: i32) -> Mode {
    if !enabled {
        Mode::OFF
    } else if percent <= 0 {
        Mode::MUTE
    } else {
        Mode::ATTENUATE
    }
}
pub fn clamp_attenuate_percent(percent: i32) -> i32 {
    percent.clamp(0, 100)
}
pub fn apply_background(current: Snapshot, mode: Mode, percent: i32) -> Snapshot {
    match mode {
        Mode::MUTE => Snapshot {
            is_muted: true,
            ..current
        },
        Mode::ATTENUATE => Snapshot {
            volume: clamp_attenuate_percent(percent),
            ..current
        },
        _ => current,
    }
}

pub const DEFAULT_FADE_DURATION_MS: i32 = 0;
pub const MAX_FADE_DURATION_MS: i32 = 5_000;
pub fn clamp_fade_duration_ms(duration: i32) -> i32 {
    duration.clamp(0, MAX_FADE_DURATION_MS)
}
pub fn interpolate_volume(start: i32, target: i32, progress: f64) -> i32 {
    let progress = progress.clamp(0.0, 1.0);
    let start = start.clamp(0, 100);
    let target = target.clamp(0, 100);
    let value = (f64::from(start) + f64::from(target - start) * progress).round();
    // .NET 8's unchecked NaN-to-int result differs from Rust's saturating cast.
    if value.is_nan() {
        i32::MIN
    } else {
        value as i32
    }
}

#[derive(Default, Debug)]
pub struct Supervisor {
    saved: HashMap<String, Snapshot>,
    foreground_pid: i32,
}
impl Supervisor {
    pub fn has_saved_state(&self) -> bool {
        !self.saved.is_empty()
    }
    pub fn on_foreground_changed(
        &mut self,
        pid: i32,
        sessions: &[Session],
        mode: Mode,
        percent: i32,
        ignored_pid: i32,
        foreground_exe: Option<&str>,
    ) -> Vec<Adjustment> {
        let mut adjustments = Vec::new();
        if mode == Mode::OFF {
            for session in sessions {
                if let Some(saved) = session
                    .key
                    .as_deref()
                    .filter(|k| !k.is_empty())
                    .and_then(|k| self.saved.get(k))
                {
                    queue(&mut adjustments, session, *saved);
                }
            }
            self.saved.clear();
            self.foreground_pid = 0;
            return adjustments;
        }
        if pid <= 0 || (ignored_pid != 0 && pid == ignored_pid) {
            return adjustments;
        }
        if self.foreground_pid == 0 {
            self.foreground_pid = pid;
            return adjustments;
        }
        self.foreground_pid = pid;
        let mut live = HashSet::new();
        for session in sessions {
            let Some(key) = session.key.as_deref().filter(|k| !k.is_empty()) else {
                continue;
            };
            if session.process_id <= 0 {
                continue;
            }
            if !session.can_adjust {
                if let Some(saved) = self.saved.remove(key) {
                    queue(&mut adjustments, session, saved);
                }
                continue;
            }
            live.insert(key);
            let foreground = session.process_id == pid
                || (!is_blank(session.executable_name.as_deref())
                    && !is_blank(foreground_exe)
                    && identity::matches_exact(
                        session.executable_name.as_deref(),
                        Some(""),
                        foreground_exe,
                    ));
            if foreground {
                if let Some(saved) = self.saved.remove(key) {
                    queue(&mut adjustments, session, saved);
                }
                continue;
            }
            let original = match self.saved.get(key).copied() {
                Some(saved) => saved,
                None => {
                    let saved = Snapshot {
                        volume: session.volume,
                        is_muted: session.is_muted,
                    };
                    self.saved.insert(key.into(), saved);
                    saved
                }
            };
            queue(
                &mut adjustments,
                session,
                apply_background(original, mode, percent),
            );
        }
        self.saved.retain(|key, _| live.contains(key.as_str()));
        adjustments
    }
}

fn queue(adjustments: &mut Vec<Adjustment>, session: &Session, state: Snapshot) {
    if session.volume != state.volume || session.is_muted != state.is_muted {
        adjustments.push(Adjustment {
            key: session.key.clone().unwrap_or_default(),
            volume: state.volume,
            is_muted: state.is_muted,
        });
    }
}
