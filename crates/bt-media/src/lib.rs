//! Now playing via Windows Global System Media Transport Controls (GSMTC):
//! the current media session's title/artist/art, playback state, timeline
//! and transport controls. All WinRT calls run on one worker thread.
use serde::Serialize;
use thiserror::Error;

#[cfg(windows)]
mod windows_impl;

#[derive(Clone, Debug, Error)]
pub enum MediaError {
    #[error("media session call failed: {0}")]
    WinRt(String),
    #[error("no media session")]
    NoSession,
    #[error("media engine stopped")]
    Stopped,
    #[error("media sessions are only available on Windows")]
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Repeat {
    None,
    Track,
    List,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    /// AppUserModelID of the source app (e.g. `Spotify.exe`).
    pub source_app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub can_play_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    /// The app supports shuffle / repeat (the original hides the buttons
    /// otherwise).
    pub can_shuffle: bool,
    pub can_repeat: bool,
    pub shuffle: Option<bool>,
    pub repeat: Option<Repeat>,
    /// Current position: the app's last report advanced by the time since,
    /// while playing (apps update the timeline only occasionally).
    pub position_ms: i64,
    pub duration_ms: i64,
    /// `data:` URL of the cover art, when the app provides one.
    pub art: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
    Seek(i64),
    ToggleShuffle,
    CycleRepeat,
}

/// Handle to the media worker thread. Cheap to clone and `Send + Sync`.
#[derive(Clone)]
pub struct MediaEngine {
    #[cfg(windows)]
    inner: windows_impl::Engine,
}

impl MediaEngine {
    pub fn start() -> Result<Self, MediaError> {
        #[cfg(windows)]
        {
            Ok(Self {
                inner: windows_impl::Engine::start()?,
            })
        }
        #[cfg(not(windows))]
        Err(MediaError::Unsupported)
    }
    /// The current session, or `None` when nothing is playing or paused.
    pub fn now_playing(&self) -> Result<Option<NowPlaying>, MediaError> {
        #[cfg(windows)]
        return self.inner.now_playing();
        #[cfg(not(windows))]
        Err(MediaError::Unsupported)
    }
    pub fn send(&self, command: Command) -> Result<(), MediaError> {
        #[cfg(windows)]
        return self.inner.send(command);
        #[cfg(not(windows))]
        {
            let _ = command;
            Err(MediaError::Unsupported)
        }
    }
}
