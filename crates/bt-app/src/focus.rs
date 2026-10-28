//! Focus-lost volume (FocusLostService): apps that lose the foreground are
//! muted or turned down to an absolute percentage, and get their saved
//! volume back when they return to the foreground or the feature turns off.
//!
//! The original polls the foreground window every 250 ms while the feature
//! is on. Here the same observation runs when the foreground changes (a
//! WinEvent hook, installed only while needed), when Windows reports a
//! session change (new apps, outside volume changes) and when the settings
//! change. Fades advance on a 40 ms timer that runs only while one is
//! pending, as in the original. Writes record no undo step.
use crate::{core::Core, rules, store::with_env};
use bt_app::win32::{
    foreground,
    wnd::{Poster, Window},
};
use bt_audio::Snapshot;
use bt_core::{
    focus::{self, Adjustment, Mode, Supervisor},
    models::{AppRuleEntry, VolumeRuleMode},
    settings::Key,
};
use std::{collections::HashMap, time::Instant};

/// `Event::App` code posted when the foreground window changes.
pub const APP_FOREGROUND: usize = 40;
/// Hidden-window timer that advances fades.
pub const FADE_TIMER: usize = 40;
const FADE_INTERVAL_MS: u32 = 40;

/// The settings the original caches (UseFocusLostVolume and FocusLost*).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub attenuate_percent: i32,
    pub fade_duration_ms: i32,
    pub selected_apps_only: bool,
}

impl Settings {
    pub fn mode(&self) -> Mode {
        focus::resolve_mode(self.enabled, self.attenuate_percent)
    }
}

/// Whether a session may be changed: never under hard mute or Lock, and
/// with "selected apps only" just the apps whose rule has focus lost on.
pub fn can_adjust(rule: Option<&AppRuleEntry>, selected_apps_only: bool) -> bool {
    let blocked = rule.is_some_and(|r| r.hard_muted || r.volume_mode == VolumeRuleMode::LOCK);
    let selected = rule.is_some_and(|r| r.focus_lost_enabled);
    !blocked && (!selected_apps_only || selected)
}

/// The original's session key: device id, U+001F, app id.
fn session_key(device: &str, app: &str) -> String {
    format!("{device}\u{1f}{app}")
}

/// One app on one device as the supervisor sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub device: String,
    pub app: String,
    pub process_id: i32,
    pub volume: i32,
    pub muted: bool,
    pub can_adjust: bool,
    pub exe_name: String,
}

impl Live {
    fn key(&self) -> String {
        session_key(&self.device, &self.app)
    }
}

/// Where the volume writes go (the audio engine, or a test double).
pub trait Mixer {
    fn set_volume(&mut self, device: &str, app: &str, volume: i32);
    fn set_mute(&mut self, device: &str, app: &str, muted: bool);
}

#[derive(Clone, Debug)]
struct Fade {
    device: String,
    app: String,
    target: Adjustment,
    start_volume: i32,
    end_volume: i32,
    duration_ms: i32,
    started: Instant,
}

/// The supervisor plus pending fades (FocusLostService without timers).
#[derive(Default)]
pub struct State {
    supervisor: Supervisor,
    fades: HashMap<String, Fade>,
}

impl State {
    pub fn has_saved_state(&self) -> bool {
        self.supervisor.has_saved_state()
    }
    pub fn fading(&self) -> bool {
        !self.fades.is_empty()
    }

    /// FocusLostService.Apply: one foreground observation.
    #[allow(clippy::too_many_arguments)]
    pub fn apply(
        &mut self,
        mode: Mode,
        foreground_pid: i32,
        foreground_exe: Option<&str>,
        attenuate_percent: i32,
        duration_ms: i32,
        own_pid: i32,
        live: &[Live],
        now: Instant,
        mixer: &mut impl Mixer,
    ) {
        let duration_ms = focus::clamp_fade_duration_ms(duration_ms);
        let mut by_key = HashMap::new();
        let sessions: Vec<focus::Session> = live
            .iter()
            .map(|l| {
                let key = l.key();
                by_key.insert(key.clone(), l);
                focus::Session {
                    key: Some(key),
                    process_id: l.process_id,
                    volume: l.volume,
                    is_muted: l.muted,
                    can_adjust: l.can_adjust,
                    executable_name: Some(l.exe_name.clone()),
                }
            })
            .collect();
        let adjustments = self.supervisor.on_foreground_changed(
            foreground_pid,
            &sessions,
            mode,
            attenuate_percent,
            own_pid,
            foreground_exe,
        );
        for adjustment in adjustments {
            if let Some(app) = by_key.get(&adjustment.key) {
                self.queue(adjustment, app, duration_ms, now, mixer);
            }
        }
        // Sessions can end mid-fade; a late tick must not touch a new
        // session that reuses the key.
        self.fades.retain(|key, _| by_key.contains_key(key));
    }

    fn queue(
        &mut self,
        adjustment: Adjustment,
        app: &Live,
        duration_ms: i32,
        now: Instant,
        mixer: &mut impl Mixer,
    ) {
        if self.fades.get(&adjustment.key).is_some_and(|f| {
            f.target.volume == adjustment.volume
                && f.target.is_muted == adjustment.is_muted
                && f.duration_ms == duration_ms
        }) {
            return;
        }
        self.fades.remove(&adjustment.key);
        if duration_ms <= 0 {
            write(
                mixer,
                &app.device,
                &app.app,
                adjustment.volume,
                adjustment.is_muted,
            );
            return;
        }
        let mut start_volume = app.volume;
        let mut end_volume = adjustment.volume;
        if adjustment.is_muted && !app.muted {
            // Keep the slider value after the fade, but reach silence first.
            end_volume = 0;
        } else if !adjustment.is_muted && app.muted {
            // Unmute at zero so the saved level never comes back in one jump.
            write(mixer, &app.device, &app.app, 0, false);
            start_volume = 0;
        }
        if start_volume == end_volume && app.muted == adjustment.is_muted {
            write(
                mixer,
                &app.device,
                &app.app,
                adjustment.volume,
                adjustment.is_muted,
            );
            return;
        }
        self.fades.insert(
            adjustment.key.clone(),
            Fade {
                device: app.device.clone(),
                app: app.app.clone(),
                target: adjustment,
                start_volume,
                end_volume,
                duration_ms,
                started: now,
            },
        );
    }

    /// FocusLostService.AdvanceFades: one 40 ms tick.
    pub fn tick(&mut self, now: Instant, mixer: &mut impl Mixer) {
        self.fades.retain(|_, fade| {
            let elapsed = now.saturating_duration_since(fade.started).as_secs_f64() * 1000.0;
            let progress = if fade.duration_ms <= 0 {
                1.0
            } else {
                elapsed / f64::from(fade.duration_ms)
            };
            let (volume, muted) = if progress >= 1.0 {
                // For mute, the slider value comes back only once silent.
                (fade.target.volume, fade.target.is_muted)
            } else {
                let volume =
                    focus::interpolate_volume(fade.start_volume, fade.end_volume, progress);
                let muted = fade.target.is_muted && fade.end_volume == fade.target.volume;
                (volume, muted)
            };
            write(mixer, &fade.device, &fade.app, volume, muted);
            progress < 1.0
        });
    }
}

fn write(mixer: &mut impl Mixer, device: &str, app: &str, volume: i32, muted: bool) {
    mixer.set_volume(device, app, volume);
    mixer.set_mute(device, app, muted);
}

/// The original's ExeName for a process (Process.ProcessName).
fn process_name(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match file.rfind('.') {
        Some(dot) if dot > 0 => file[..dot].to_owned(),
        _ => file.to_owned(),
    }
}

/// The apps of a snapshot as supervisor sessions. A grouped app reports the
/// foreground process when it is one of its processes, else its first.
pub fn live_sessions(
    snapshot: &Snapshot,
    foreground_pid: i32,
    mut rule: impl FnMut(&str) -> Option<AppRuleEntry>,
    selected_apps_only: bool,
) -> Vec<Live> {
    let mut live = vec![];
    for device in &snapshot.devices {
        for app in &device.apps {
            let exe_name = rules::exe_name(app);
            let pid = if app
                .process_ids
                .iter()
                .any(|p| *p as i32 == foreground_pid && foreground_pid > 0)
            {
                foreground_pid
            } else {
                app.process_ids.first().map_or(0, |p| *p as i32)
            };
            live.push(Live {
                device: device.id.clone(),
                app: app.key.clone(),
                process_id: pid,
                volume: (app.volume * 100.0).round() as i32,
                muted: app.muted,
                can_adjust: can_adjust(rule(&exe_name).as_ref(), selected_apps_only),
                exe_name,
            });
        }
    }
    live
}

struct Audio<'a>(&'a bt_audio::AudioEngine);
impl Mixer for Audio<'_> {
    fn set_volume(&mut self, device: &str, app: &str, volume: i32) {
        let _ = self.0.set_app_volume(device, app, volume as f32 / 100.0);
    }
    fn set_mute(&mut self, device: &str, app: &str, muted: bool) {
        let _ = self.0.set_app_mute(device, app, muted);
    }
}

/// The running service: settings, hook and fade timer around [`State`].
pub struct FocusLost {
    timer: Window,
    settings: Settings,
    state: State,
    started: bool,
    last_pid: i32,
    last_exe: Option<String>,
}

impl FocusLost {
    /// `timer` is the hidden window; foreground changes post
    /// [`APP_FOREGROUND`] through `poster`.
    pub fn new(timer: Window, poster: Poster) -> Self {
        foreground::init(move || poster.post(APP_FOREGROUND));
        Self {
            timer,
            settings: Settings::default(),
            state: State::default(),
            started: false,
            last_pid: 0,
            last_exe: None,
        }
    }

    fn read_settings(&mut self, core: &Core) {
        let store = &core.store;
        self.settings = Settings {
            enabled: store.bool(Key::UseFocusLostVolume),
            attenuate_percent: store.int(Key::FocusLostAttenuatePercent),
            fade_duration_ms: store.int(Key::FocusLostFadeDurationMs),
            selected_apps_only: store.bool(Key::FocusLostSelectedAppsOnly),
        };
    }

    /// Whether foreground changes are being watched.
    fn active(&self) -> bool {
        self.started && (self.settings.enabled || self.state.has_saved_state())
    }

    pub fn start(&mut self, core: &mut Core) {
        self.started = true;
        self.read_settings(core);
        if self.settings.enabled {
            self.observe(core);
        }
    }

    /// The settings changed: re-read them and observe once (enabling starts
    /// tracking; disabling restores the saved volumes).
    pub fn settings_changed(&mut self, core: &mut Core) {
        if !self.started {
            return;
        }
        let before = self.settings;
        self.read_settings(core);
        if self.settings != before || self.active() {
            self.observe(core);
        }
    }

    /// The foreground window changed.
    pub fn foreground_changed(&mut self, core: &mut Core) {
        if self.active() {
            self.observe(core);
        }
    }

    /// Windows reported a session change (an app appeared, or a volume was
    /// changed elsewhere): the original's next poll would see it.
    pub fn audio_changed(&mut self, core: &mut Core) {
        if self.active() {
            self.observe(core);
        }
    }

    pub fn timer(&mut self, core: &mut Core) {
        self.state.tick(Instant::now(), &mut Audio(&core.audio));
        self.sync();
    }

    /// Shutdown never leaves an app muted or turned down.
    pub fn stop(&mut self, core: &mut Core) {
        self.started = false;
        if self.state.has_saved_state() || self.state.fading() {
            self.state.fades.clear();
            self.apply(core, Mode::OFF, 0, 0, 0);
        }
        self.sync();
    }

    fn observe(&mut self, core: &mut Core) {
        let pid = foreground::foreground_pid() as i32;
        let settings = self.settings;
        self.apply(
            core,
            settings.mode(),
            pid,
            settings.attenuate_percent,
            settings.fade_duration_ms,
        );
        self.sync();
    }

    fn apply(&mut self, core: &mut Core, mode: Mode, pid: i32, percent: i32, duration: i32) {
        let Ok(snapshot) = core.audio.snapshot() else {
            return;
        };
        // Resolving the name is the costly part; only on a new process.
        if pid != self.last_pid {
            self.last_pid = pid;
            self.last_exe = (pid > 0)
                .then(|| foreground::process_path(pid as u32))
                .flatten()
                .map(|p| process_name(&p));
        }
        let store = &mut core.store;
        let live = with_env(|env| {
            live_sessions(
                &snapshot,
                pid,
                |exe| store.rules.get(Some(exe), env),
                self.settings.selected_apps_only,
            )
        });
        core.store.flush();
        self.state.apply(
            mode,
            pid,
            self.last_exe.as_deref(),
            percent,
            duration,
            std::process::id() as i32,
            &live,
            Instant::now(),
            &mut Audio(&core.audio),
        );
    }

    /// Installs the hook only while needed and runs the fade timer only
    /// while a fade is pending, so an unused feature costs nothing.
    fn sync(&mut self) {
        if self.active() {
            foreground::install();
        } else {
            foreground::uninstall();
        }
        if self.state.fading() {
            self.timer.set_timer(FADE_TIMER, FADE_INTERVAL_MS);
        } else {
            self.timer.kill_timer(FADE_TIMER);
        }
    }
}

impl Drop for FocusLost {
    fn drop(&mut self) {
        foreground::uninstall();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Default)]
    struct Recorder {
        writes: Vec<(String, i32, bool)>,
        current: HashMap<String, (i32, bool)>,
    }
    impl Mixer for Recorder {
        fn set_volume(&mut self, _device: &str, app: &str, volume: i32) {
            self.current.entry(app.into()).or_default().0 = volume;
        }
        fn set_mute(&mut self, _device: &str, app: &str, muted: bool) {
            let entry = self.current.entry(app.into()).or_default();
            entry.1 = muted;
            self.writes.push((app.into(), entry.0, muted));
        }
    }

    fn app(name: &str, pid: i32, volume: i32, muted: bool) -> Live {
        Live {
            device: "dev".into(),
            app: name.into(),
            process_id: pid,
            volume,
            muted,
            can_adjust: true,
            exe_name: name.into(),
        }
    }

    const OWN: i32 = 999;

    fn observe(
        state: &mut State,
        mode: Mode,
        pid: i32,
        duration: i32,
        live: &[Live],
        now: Instant,
        mixer: &mut Recorder,
    ) {
        state.apply(mode, pid, None, 20, duration, OWN, live, now, mixer);
    }

    #[test]
    fn eligibility_follows_the_original() {
        let rule = |hard_muted, mode, focus_lost_enabled| AppRuleEntry {
            hard_muted,
            volume_mode: mode,
            focus_lost_enabled,
            ..AppRuleEntry::default()
        };
        assert!(can_adjust(None, false));
        assert!(!can_adjust(None, true));
        assert!(!can_adjust(
            Some(&rule(true, VolumeRuleMode::NONE, true)),
            false
        ));
        assert!(!can_adjust(
            Some(&rule(false, VolumeRuleMode::LOCK, true)),
            false
        ));
        assert!(can_adjust(
            Some(&rule(false, VolumeRuleMode::LAUNCH, false)),
            false
        ));
        assert!(can_adjust(
            Some(&rule(false, VolumeRuleMode::NONE, true)),
            true
        ));
        assert!(!can_adjust(
            Some(&rule(false, VolumeRuleMode::NONE, false)),
            true
        ));
    }

    #[test]
    fn attenuates_background_apps_and_restores_them() {
        let (mut state, mut mixer, now) = (State::default(), Recorder::default(), Instant::now());
        let live = [app("a", 1, 80, false), app("b", 2, 60, false)];
        // The first observation only records the foreground.
        observe(&mut state, Mode::ATTENUATE, 1, 0, &live, now, &mut mixer);
        assert!(mixer.writes.is_empty());
        observe(&mut state, Mode::ATTENUATE, 2, 0, &live, now, &mut mixer);
        assert_eq!(mixer.writes, [("a".into(), 20, false)]);
        let live = [app("a", 1, 20, false), app("b", 2, 60, false)];
        observe(&mut state, Mode::ATTENUATE, 1, 0, &live, now, &mut mixer);
        assert_eq!(
            mixer.writes[1..],
            [("a".into(), 80, false), ("b".into(), 20, false)]
        );
        // Our own windows keep everything as it is.
        mixer.writes.clear();
        observe(&mut state, Mode::ATTENUATE, OWN, 0, &live, now, &mut mixer);
        assert!(mixer.writes.is_empty());
        // Turning the feature off restores the saved volume.
        let live = [app("a", 1, 80, false), app("b", 2, 20, false)];
        observe(&mut state, Mode::OFF, 0, 0, &live, now, &mut mixer);
        assert_eq!(mixer.writes, [("b".into(), 60, false)]);
        assert!(!state.has_saved_state());
    }

    #[test]
    fn mute_fades_to_silence_then_mutes_at_the_saved_level() {
        let (mut state, mut mixer, now) = (State::default(), Recorder::default(), Instant::now());
        let live = [app("a", 1, 80, false), app("b", 2, 50, false)];
        observe(&mut state, Mode::MUTE, 1, 1000, &live, now, &mut mixer);
        observe(&mut state, Mode::MUTE, 2, 1000, &live, now, &mut mixer);
        assert!(state.fading());
        state.tick(now + Duration::from_millis(500), &mut mixer);
        assert_eq!(mixer.writes.last().unwrap(), &("a".into(), 40, false));
        // Re-observing with the same target leaves the fade running.
        observe(&mut state, Mode::MUTE, 2, 1000, &live, now, &mut mixer);
        assert!(state.fading());
        state.tick(now + Duration::from_millis(1000), &mut mixer);
        assert_eq!(mixer.writes.last().unwrap(), &("a".into(), 80, true));
        assert!(!state.fading());
    }

    #[test]
    fn unmute_starts_from_zero_and_fades_up() {
        let (mut state, mut mixer, now) = (State::default(), Recorder::default(), Instant::now());
        let live = [app("a", 1, 80, false), app("b", 2, 50, false)];
        observe(&mut state, Mode::MUTE, 1, 0, &live, now, &mut mixer);
        observe(&mut state, Mode::MUTE, 2, 0, &live, now, &mut mixer);
        assert_eq!(mixer.writes, [("a".into(), 80, true)]);
        mixer.writes.clear();
        let live = [app("a", 1, 80, true), app("b", 2, 50, false)];
        observe(&mut state, Mode::MUTE, 1, 400, &live, now, &mut mixer);
        // Unmuted at zero first, then b starts fading down.
        assert_eq!(mixer.writes, [("a".into(), 0, false)]);
        state.tick(now + Duration::from_millis(100), &mut mixer);
        // a: 0 -> 80 at 25% = 20 (away from zero); b: 50 -> 0 at 25%.
        assert!(mixer.writes.contains(&("a".into(), 20, false)));
        assert!(mixer.writes.contains(&("b".into(), 38, false)));
        state.tick(now + Duration::from_millis(400), &mut mixer);
        assert!(mixer.writes.contains(&("a".into(), 80, false)));
        assert!(mixer.writes.contains(&("b".into(), 50, true)));
    }

    #[test]
    fn fades_for_ended_sessions_are_dropped() {
        let (mut state, mut mixer, now) = (State::default(), Recorder::default(), Instant::now());
        let live = [app("a", 1, 80, false), app("b", 2, 50, false)];
        observe(&mut state, Mode::ATTENUATE, 1, 1000, &live, now, &mut mixer);
        observe(&mut state, Mode::ATTENUATE, 2, 1000, &live, now, &mut mixer);
        assert!(state.fading());
        observe(
            &mut state,
            Mode::ATTENUATE,
            2,
            1000,
            &live[1..],
            now,
            &mut mixer,
        );
        assert!(!state.fading());
    }

    #[test]
    fn process_names_drop_the_extension() {
        assert_eq!(process_name(r"C:\Games\game.exe"), "game");
        assert_eq!(process_name(r"C:\x\.hidden"), ".hidden");
        assert_eq!(process_name("plain"), "plain");
    }
}
