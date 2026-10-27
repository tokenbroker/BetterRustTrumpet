//! WinRT side of the media engine. Everything here runs on the worker thread.
use crate::{Command, MediaError, NowPlaying, Repeat};
use std::{cell::RefCell, sync::mpsc, thread};
use windows::{
    Media::{
        Control::{
            GlobalSystemMediaTransportControlsSession as Session,
            GlobalSystemMediaTransportControlsSessionManager as Manager,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
        },
        MediaPlaybackAutoRepeatMode,
    },
    Storage::Streams::DataReader,
    Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
};

impl From<windows::core::Error> for MediaError {
    fn from(error: windows::core::Error) -> Self {
        MediaError::WinRt(format!("{} ({:?})", error.message(), error.code()))
    }
}

type Job = Box<dyn FnOnce(&Backend) + Send>;

#[derive(Clone)]
pub struct Engine {
    jobs: mpsc::Sender<Job>,
}

impl Engine {
    pub fn start() -> Result<Self, MediaError> {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (ready, started) = mpsc::channel();
        thread::Builder::new()
            .name("bt-media-winrt".into())
            .spawn(move || {
                // SAFETY: once per dedicated thread, balanced below after every
                // WinRT object (the Backend) has been dropped.
                if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
                    let _ = ready.send(Err(MediaError::from(e)));
                    return;
                }
                match Backend::new() {
                    Ok(backend) => {
                        let _ = ready.send(Ok(()));
                        for job in inbox {
                            job(&backend);
                        }
                    }
                    Err(e) => {
                        let _ = ready.send(Err(e));
                    }
                }
                // SAFETY: matches the CoInitializeEx above on this thread.
                unsafe { CoUninitialize() };
            })
            .map_err(|e| MediaError::WinRt(e.to_string()))?;
        started.recv().map_err(|_| MediaError::Stopped)??;
        Ok(Self { jobs })
    }
    fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Backend) -> Result<T, MediaError> + Send + 'static,
    ) -> Result<T, MediaError> {
        let (reply, answer) = mpsc::channel();
        self.jobs
            .send(Box::new(move |backend| {
                let _ = reply.send(work(backend));
            }))
            .map_err(|_| MediaError::Stopped)?;
        answer.recv().map_err(|_| MediaError::Stopped)?
    }
    pub fn now_playing(&self) -> Result<Option<NowPlaying>, MediaError> {
        self.call(Backend::now_playing)
    }
    pub fn send(&self, command: Command) -> Result<(), MediaError> {
        self.call(move |b| b.send(command))
    }
}

/// Cover art is read once per track rather than on every poll.
struct Art {
    key: String,
    url: Option<String>,
}

struct Backend {
    manager: Manager,
    art: RefCell<Option<Art>>,
}

const TICKS_PER_MS: i64 = 10_000;

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

impl Backend {
    fn new() -> Result<Self, MediaError> {
        Ok(Self {
            manager: Manager::RequestAsync()?.join()?,
            art: RefCell::new(None),
        })
    }
    fn session(&self) -> Result<Session, MediaError> {
        self.manager
            .GetCurrentSession()
            .map_err(|_| MediaError::NoSession)
    }
    fn read_art(session: &Session) -> windows::core::Result<Option<String>> {
        let properties = session.TryGetMediaPropertiesAsync()?.join()?;
        let Ok(thumbnail) = properties.Thumbnail() else {
            return Ok(None);
        };
        let stream = thumbnail.OpenReadAsync()?.join()?;
        let size = stream.Size()? as u32;
        if size == 0 || size > 8 * 1024 * 1024 {
            return Ok(None);
        }
        let content_type = stream.ContentType()?.to_string();
        let reader = DataReader::CreateDataReader(&stream)?;
        reader.LoadAsync(size)?.join()?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes)?;
        let mime = if content_type.starts_with("image/") {
            content_type
        } else {
            "image/png".into()
        };
        Ok(Some(format!("data:{mime};base64,{}", base64(&bytes))))
    }
    fn now_playing(&self) -> Result<Option<NowPlaying>, MediaError> {
        let Ok(session) = self.session() else {
            return Ok(None);
        };
        let properties = session.TryGetMediaPropertiesAsync()?.join()?;
        let title = properties.Title()?.to_string();
        let artist = properties.Artist()?.to_string();
        let album = properties.AlbumTitle()?.to_string();
        let source_app = session.SourceAppUserModelId()?.to_string();
        let info = session.GetPlaybackInfo()?;
        let controls = info.Controls()?;
        let timeline = session.GetTimelineProperties()?;
        let start = timeline.StartTime()?.Duration;
        let end = timeline.EndTime()?.Duration;
        let mut position = timeline.Position()?.Duration;
        let playing = info.PlaybackStatus()? == Status::Playing;
        if playing {
            // DateTime ticks are 100 ns since 1601-01-01 UTC.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as i64 / 100)
                + 116_444_736_000_000_000;
            if let Ok(updated) = timeline.LastUpdatedTime() {
                if updated.UniversalTime > 0 && now > updated.UniversalTime {
                    position += now - updated.UniversalTime;
                }
            }
            position = position.min(end);
        }
        let shuffle = info.IsShuffleActive().ok().and_then(|v| v.Value().ok());
        let repeat =
            info.AutoRepeatMode()
                .ok()
                .and_then(|v| v.Value().ok())
                .map(|mode| match mode {
                    MediaPlaybackAutoRepeatMode::Track => Repeat::Track,
                    MediaPlaybackAutoRepeatMode::List => Repeat::List,
                    _ => Repeat::None,
                });
        let key = format!("{source_app}\u{1}{title}\u{1}{artist}\u{1}{album}");
        let cached = self
            .art
            .borrow()
            .as_ref()
            .filter(|a| a.key == key)
            .map(|a| a.url.clone());
        let art = match cached {
            Some(url) => url,
            None => {
                let url = Self::read_art(&session).ok().flatten();
                *self.art.borrow_mut() = Some(Art {
                    key,
                    url: url.clone(),
                });
                url
            }
        };
        Ok(Some(NowPlaying {
            source_app,
            title,
            artist,
            album,
            playing,
            can_play_pause: controls.IsPlayPauseToggleEnabled()?,
            can_next: controls.IsNextEnabled()?,
            can_previous: controls.IsPreviousEnabled()?,
            can_seek: controls.IsPlaybackPositionEnabled()?,
            can_shuffle: controls.IsShuffleEnabled().unwrap_or(false),
            can_repeat: controls.IsRepeatEnabled().unwrap_or(false),
            shuffle,
            repeat,
            position_ms: (position - start).max(0) / TICKS_PER_MS,
            duration_ms: (end - start).max(0) / TICKS_PER_MS,
            art,
        }))
    }
    fn send(&self, command: Command) -> Result<(), MediaError> {
        let session = self.session()?;
        match command {
            Command::PlayPause => {
                session.TryTogglePlayPauseAsync()?.join()?;
            }
            Command::Next => {
                session.TrySkipNextAsync()?.join()?;
            }
            Command::Previous => {
                session.TrySkipPreviousAsync()?.join()?;
            }
            Command::Seek(ms) => {
                let start = session.GetTimelineProperties()?.StartTime()?.Duration;
                session
                    .TryChangePlaybackPositionAsync(start + ms.max(0) * TICKS_PER_MS)?
                    .join()?;
            }
            Command::ToggleShuffle => {
                let info = session.GetPlaybackInfo()?;
                let current = info
                    .IsShuffleActive()
                    .ok()
                    .and_then(|v| v.Value().ok())
                    .unwrap_or(false);
                session.TryChangeShuffleActiveAsync(!current)?.join()?;
            }
            Command::CycleRepeat => {
                let info = session.GetPlaybackInfo()?;
                let next = match info.AutoRepeatMode().ok().and_then(|v| v.Value().ok()) {
                    Some(MediaPlaybackAutoRepeatMode::None) | None => {
                        MediaPlaybackAutoRepeatMode::List
                    }
                    Some(MediaPlaybackAutoRepeatMode::List) => MediaPlaybackAutoRepeatMode::Track,
                    _ => MediaPlaybackAutoRepeatMode::None,
                };
                session.TryChangeAutoRepeatModeAsync(next)?.join()?;
            }
        }
        Ok(())
    }
}
