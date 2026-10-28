//! LegacyMediaPlayerService: players that do not publish a Windows media
//! session (VLC, foobar2000, MPC-HC...) found among the app sessions that
//! are playing sound, as the fallback when GSMTC has no session.
//!
//! The original polls the audio sessions every second while such a player
//! plays (every 10 s otherwise, stopping after five empty polls). This port
//! re-reads them only when Windows reports a session change (the session
//! state events bt-audio already watches) and when the media popup refreshes
//! while visible, so it costs nothing when idle.
//!
//! Q15: the player cannot be controlled. Transport and seek do nothing (the
//! commands go to GSMTC, which has no session) and the timeline is zero; the
//! popup gets no shuffle or repeat buttons.
use crate::core::Core;
use bt_app::win32::{icons, mainwindow};
use bt_audio::ActiveSession;
use bt_media::NowPlaying;
use std::{cell::RefCell, collections::HashMap, sync::Arc};

/// KnownLegacyPlayers: process names (without `.exe`, any case).
const KNOWN_PLAYERS: [&str; 21] = [
    "vlc",
    "wmplayer",
    "mpc-hc",
    "mpc-hc64",
    "mpc-be",
    "mpc-be64",
    "potplayer",
    "potplayer64",
    "potplayermini",
    "potplayermini64",
    "foobar2000",
    "winamp",
    "aimp",
    "musicbee",
    "mediamonkey",
    "kmplayer",
    "gom",
    "smplayer",
    "mpv",
    "clementine",
    "audacity",
];

/// GetPlayerExecutablePath's known install locations: the Program Files
/// variable and the path below it.
const KNOWN_PATHS: [(&str, &str); 10] = [
    ("ProgramFiles", r"VideoLAN\VLC\vlc.exe"),
    ("ProgramFiles(x86)", r"VideoLAN\VLC\vlc.exe"),
    ("ProgramFiles", r"Windows Media Player\wmplayer.exe"),
    ("ProgramFiles(x86)", r"Windows Media Player\wmplayer.exe"),
    ("ProgramFiles", r"MPC-HC\mpc-hc64.exe"),
    ("ProgramFiles(x86)", r"MPC-HC\mpc-hc.exe"),
    ("ProgramFiles", r"MPC-BE x64\mpc-be64.exe"),
    ("ProgramFiles(x86)", r"MPC-BE\mpc-be.exe"),
    ("ProgramFiles(x86)", r"foobar2000\foobar2000.exe"),
    ("ProgramFiles", r"DAUM\PotPlayer\PotPlayerMini64.exe"),
];

/// The suffixes GetCurrentMediaInfo strips from a player's window title.
const TITLE_SUFFIXES: [&str; 6] = [
    " - VLC media player",
    " - Windows Media Player",
    " - foobar2000",
    " - MPC-HC",
    " - MPC-BE",
    " [foobar2000]",
];

/// Cover-art size for a player's icon.
const ICON_SIZE: u32 = 256;

/// A known player that is playing sound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    /// Full image path.
    pub executable: String,
    /// The session's display name.
    pub display_name: String,
}

impl Player {
    /// The executable's file name (the original's `ExeName`).
    pub fn exe_name(&self) -> &str {
        file_name(&self.executable)
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Lower-case file name without `.exe`.
fn clean_name(path: &str) -> String {
    let name = file_name(path).to_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

/// FindActiveLegacyPlayer: the first active session of a known player.
pub fn find(sessions: &[ActiveSession]) -> Option<Player> {
    sessions
        .iter()
        .find(|s| KNOWN_PLAYERS.contains(&clean_name(&s.executable).as_str()))
        .map(|s| Player {
            executable: s.executable.clone(),
            display_name: s.display_name.clone(),
        })
}

/// The playing legacy player, if any.
pub fn current(core: &Core) -> Option<Player> {
    find(&core.audio.active_sessions().ok()?)
}

/// GetCurrentMediaInfo: the main window's title without the player's
/// suffix, else the session's display name, else the executable name.
pub fn clean_title(title: &str) -> String {
    let mut title = title.to_owned();
    for suffix in TITLE_SUFFIXES {
        title = title.replace(suffix, "");
    }
    title.trim().to_owned()
}

fn title(player: &Player) -> String {
    mainwindow::title(&player.executable)
        .map(|t| clean_title(&t))
        .filter(|t| !t.trim().is_empty())
        .or_else(|| Some(player.display_name.clone()).filter(|n| !n.is_empty()))
        .unwrap_or_else(|| player.exe_name().to_owned())
}

/// GetPlayerExecutablePath's name cleanup of a session's source app id
/// ("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic" -> "zunemusic",
/// "Spotify.exe" -> "spotify").
pub fn app_id_name(source_app: &str) -> String {
    let mut name = source_app.to_lowercase();
    if let Some(stripped) = name.strip_suffix(".exe") {
        name = stripped.to_owned();
    }
    if let Some((_, after)) = name.rsplit_once('!') {
        name = after.to_owned();
    }
    if name.contains('.') && !name.contains('\\') {
        name = name.rsplit('.').next().unwrap_or_default().to_owned();
    }
    name
}

/// GetPlayerExecutablePath: a playing session of that program, then the
/// known install locations. (The original looks for any running process
/// by name; this port only looks at processes with an audio session.)
fn executable_for(core: &Core, source_app: &str) -> Option<String> {
    let name = app_id_name(source_app);
    if name.is_empty() {
        return None;
    }
    let sessions = core.audio.active_sessions().unwrap_or_default();
    if let Some(session) = sessions.iter().find(|s| clean_name(&s.executable) == name) {
        return Some(session.executable.clone());
    }
    KNOWN_PATHS.iter().find_map(|(variable, below)| {
        let path = format!("{}\\{below}", std::env::var(variable).ok()?);
        let file = clean_name(&path);
        (std::path::Path::new(&path).exists() && (name.contains(&file) || file.contains(&name)))
            .then_some(path)
    })
}

thread_local! {
    /// Player icons by executable path: the same `Arc` each time, so the
    /// popup does not decode it again.
    static ICONS: RefCell<HashMap<String, Option<Arc<Vec<u8>>>>> = RefCell::new(HashMap::new());
}

/// ExtractIconFromPath, as encoded cover art.
fn icon(executable: &str) -> Option<Arc<Vec<u8>>> {
    ICONS.with(|icons| {
        icons
            .borrow_mut()
            .entry(executable.to_lowercase())
            .or_insert_with(|| icons::png_file_icon(executable, ICON_SIZE).map(Arc::new))
            .clone()
    })
}

/// What the popup shows for a legacy player: its title, playing, no
/// controls and a zero timeline (Q15).
fn legacy_now_playing(player: &Player) -> NowPlaying {
    NowPlaying {
        source_app: player.exe_name().to_owned(),
        title: title(player),
        artist: String::new(),
        album: String::new(),
        playing: true,
        can_play_pause: false,
        can_next: false,
        can_previous: false,
        can_seek: false,
        can_shuffle: false,
        can_repeat: false,
        shuffle: None,
        repeat: None,
        position_ms: 0,
        duration_ms: 0,
        art: icon(&player.executable),
    }
}

/// MediaSessionService's view: the Windows media session, its player's
/// icon standing in for missing cover art (GetLegacyPlayerIcon), else a
/// playing legacy player.
pub fn now_playing(core: &Core) -> Option<NowPlaying> {
    let session = core
        .media
        .as_ref()
        .and_then(|m| m.now_playing().ok().flatten());
    match session {
        Some(mut now) => {
            if now.art.is_none() {
                now.art = executable_for(core, &now.source_app).and_then(|path| icon(&path));
            }
            Some(now)
        }
        None => current(core).map(|player| legacy_now_playing(&player)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(path: &str) -> ActiveSession {
        ActiveSession {
            executable: path.into(),
            display_name: String::new(),
            process_id: 1,
        }
    }

    #[test]
    fn finds_the_first_known_player() {
        let sessions = [
            session(r"C:\Program Files\Mozilla Firefox\firefox.exe"),
            session(r"C:\Program Files\VideoLAN\VLC\VLC.EXE"),
            session(r"C:\foobar2000\foobar2000.exe"),
        ];
        let player = find(&sessions).unwrap();
        assert_eq!(player.exe_name(), "VLC.EXE");
        assert!(find(&sessions[..1]).is_none());
    }

    #[test]
    fn strips_player_suffixes_from_titles() {
        assert_eq!(clean_title("Song.mp3 - VLC media player"), "Song.mp3");
        assert_eq!(
            clean_title("  Artist - Track [foobar2000] "),
            "Artist - Track"
        );
        assert_eq!(clean_title("VLC media player"), "VLC media player");
    }

    #[test]
    fn cleans_source_app_ids() {
        assert_eq!(app_id_name("Spotify.exe"), "spotify");
        assert_eq!(
            app_id_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "zunemusic"
        );
        assert_eq!(app_id_name("vlc"), "vlc");
    }
}
