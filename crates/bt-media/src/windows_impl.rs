//! WinRT side of the media engine. Everything here runs on the worker thread.
use crate::{Command, MediaError, NowPlaying, Repeat};
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
use windows::{
    Foundation::TypedEventHandler,
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
    pub fn watch(&self, listener: Arc<dyn Fn() + Send + Sync>) -> Result<(), MediaError> {
        self.call(move |b| b.watch(listener))
    }
    pub fn any_playing(&self) -> Result<bool, MediaError> {
        self.call(|b| Ok(b.any_playing()))
    }
}

/// The change subscriptions: the manager's two events, and
/// PlaybackInfoChanged and MediaPropertiesChanged (a new track) on every
/// session (re-made when the sessions change, like the original's
/// SubscribeToAllSessions).
struct Watch {
    listener: Arc<dyn Fn() + Send + Sync>,
    /// Set by the manager's handlers; the next call on the worker thread
    /// re-subscribes the sessions. Handlers never hold the job sender, so
    /// the worker still ends when the engine is dropped.
    stale: Arc<AtomicBool>,
    manager_tokens: [i64; 2],
    sessions: Vec<(Session, i64, i64)>,
}

/// Cover art is read once per track rather than on every poll.
struct Art {
    key: String,
    bytes: Option<Arc<Vec<u8>>>,
}

struct Backend {
    manager: Manager,
    art: RefCell<Option<Art>>,
    watch: RefCell<Option<Watch>>,
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(watch) = self.watch.get_mut().take() {
            let _ = self
                .manager
                .RemoveCurrentSessionChanged(watch.manager_tokens[0]);
            let _ = self.manager.RemoveSessionsChanged(watch.manager_tokens[1]);
            for (session, playback, properties) in watch.sessions {
                let _ = session.RemovePlaybackInfoChanged(playback);
                let _ = session.RemoveMediaPropertiesChanged(properties);
            }
        }
    }
}

const TICKS_PER_MS: i64 = 10_000;

impl Backend {
    fn new() -> Result<Self, MediaError> {
        Ok(Self {
            manager: Manager::RequestAsync()?.join()?,
            art: RefCell::new(None),
            watch: RefCell::new(None),
        })
    }
    /// Subscribes once; a second call only replaces the listener.
    fn watch(&self, listener: Arc<dyn Fn() + Send + Sync>) -> Result<(), MediaError> {
        if let Some(watch) = self.watch.borrow_mut().as_mut() {
            watch.listener = listener;
            return Ok(());
        }
        let stale = Arc::new(AtomicBool::new(false));
        let handler = |listener: &Arc<dyn Fn() + Send + Sync>, stale: &Arc<AtomicBool>| {
            let (listener, stale) = (listener.clone(), stale.clone());
            move || {
                stale.store(true, Ordering::Release);
                listener();
            }
        };
        let current = handler(&listener, &stale);
        let sessions = handler(&listener, &stale);
        let manager_tokens = [
            self.manager
                .CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                    current();
                    Ok(())
                }))?,
            self.manager
                .SessionsChanged(&TypedEventHandler::new(move |_, _| {
                    sessions();
                    Ok(())
                }))?,
        ];
        *self.watch.borrow_mut() = Some(Watch {
            listener,
            stale,
            manager_tokens,
            sessions: vec![],
        });
        self.resubscribe();
        Ok(())
    }
    /// Re-subscribes the session events on the current list of sessions.
    fn resubscribe(&self) {
        let mut watch = self.watch.borrow_mut();
        let Some(watch) = watch.as_mut() else {
            return;
        };
        watch.stale.store(false, Ordering::Release);
        for (session, playback, properties) in watch.sessions.drain(..) {
            let _ = session.RemovePlaybackInfoChanged(playback);
            let _ = session.RemoveMediaPropertiesChanged(properties);
        }
        let Ok(list) = self.manager.GetSessions() else {
            return;
        };
        for session in list {
            let listener = watch.listener.clone();
            let playback = session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                listener();
                Ok(())
            }));
            let listener = watch.listener.clone();
            let properties =
                session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                    listener();
                    Ok(())
                }));
            match (playback, properties) {
                (Ok(playback), Ok(properties)) => {
                    watch.sessions.push((session, playback, properties));
                }
                (Ok(playback), Err(_)) => {
                    let _ = session.RemovePlaybackInfoChanged(playback);
                }
                (Err(_), Ok(properties)) => {
                    let _ = session.RemoveMediaPropertiesChanged(properties);
                }
                (Err(_), Err(_)) => {}
            }
        }
    }
    /// Brings the session subscriptions up to date after a manager event.
    fn refresh_watch(&self) {
        let stale = self
            .watch
            .borrow()
            .as_ref()
            .is_some_and(|w| w.stale.load(Ordering::Acquire));
        if stale {
            self.resubscribe();
        }
    }
    /// MediaSessionService.CheckIfAnyMediaPlaying: any session (not only
    /// the current one) is playing.
    fn any_playing(&self) -> bool {
        self.refresh_watch();
        let Ok(list) = self.manager.GetSessions() else {
            return false;
        };
        list.into_iter().any(|session| {
            session
                .GetPlaybackInfo()
                .and_then(|info| info.PlaybackStatus())
                .is_ok_and(|status| status == Status::Playing)
        })
    }
    fn session(&self) -> Result<Session, MediaError> {
        self.manager
            .GetCurrentSession()
            .map_err(|_| MediaError::NoSession)
    }
    fn read_art(session: &Session) -> windows::core::Result<Option<Arc<Vec<u8>>>> {
        let properties = session.TryGetMediaPropertiesAsync()?.join()?;
        let Ok(thumbnail) = properties.Thumbnail() else {
            return Ok(None);
        };
        let stream = thumbnail.OpenReadAsync()?.join()?;
        let size = stream.Size()? as u32;
        if size == 0 || size > 8 * 1024 * 1024 {
            return Ok(None);
        }
        let reader = DataReader::CreateDataReader(&stream)?;
        reader.LoadAsync(size)?.join()?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes)?;
        Ok(Some(Arc::new(bytes)))
    }
    fn now_playing(&self) -> Result<Option<NowPlaying>, MediaError> {
        self.refresh_watch();
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
            if let Ok(updated) = timeline.LastUpdatedTime()
                && updated.UniversalTime > 0
                && now > updated.UniversalTime
            {
                position += now - updated.UniversalTime;
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
            .map(|a| a.bytes.clone());
        let art = match cached {
            Some(bytes) => bytes,
            None => {
                let bytes = Self::read_art(&session).ok().flatten();
                *self.art.borrow_mut() = Some(Art {
                    key,
                    bytes: bytes.clone(),
                });
                bytes
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
