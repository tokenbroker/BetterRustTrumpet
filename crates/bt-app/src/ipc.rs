//! The CLI pipe (`bt`): the pipe thread hands each command to the UI thread
//! (a queue plus an `Event::App` post to the hidden window) and waits for the
//! answer, like the original's DispatchToUI. Ping is answered on the pipe
//! thread, as in the original.
use crate::{core::Core, rules};
use bt_app::win32::{
    clock,
    pipe::{self, PipeServer},
    wnd::Poster,
};
use bt_core::{
    cli_dispatch::{self, App, Device, Provider},
    settings::{Key, Value},
};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Mutex, mpsc},
    time::Duration,
};

/// `Event::App` code for "pipe commands are waiting".
pub const APP_IPC: usize = 100;

/// The longest a pipe client waits for the UI thread.
const UI_TIMEOUT: Duration = Duration::from_secs(30);

const VERSION: &str = env!("CARGO_PKG_VERSION");

struct Job {
    line: String,
    reply: mpsc::Sender<String>,
}

static JOBS: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());

/// Starts the pipe server; commands reach the UI thread through `hidden`.
pub fn start(hidden: Poster) -> Option<PipeServer> {
    let server = PipeServer::start(&pipe::session_pipe_name(), move |line| {
        if let Some(pong) = cli_dispatch::ping(line, Some(VERSION)) {
            return pong;
        }
        let (reply, answer) = mpsc::channel();
        if let Ok(mut jobs) = JOBS.lock() {
            jobs.push_back(Job {
                line: line.to_owned(),
                reply,
            });
        }
        hidden.post(APP_IPC);
        answer
            .recv_timeout(UI_TIMEOUT)
            .unwrap_or_else(|_| cli_dispatch::error("BetterTrumpet did not respond"))
    });
    match server {
        Ok(server) => Some(server),
        Err(e) => {
            eprintln!("BetterTrumpet: the CLI pipe failed to start: {e}");
            None
        }
    }
}

/// Runs the waiting pipe commands (UI thread).
pub fn drain(core: &mut Core) {
    loop {
        let Some(job) = JOBS.lock().ok().and_then(|mut jobs| jobs.pop_front()) else {
            return;
        };
        let response = catch_unwind(AssertUnwindSafe(|| {
            cli_dispatch::process(&mut Live { core: &mut *core }, &job.line)
        }))
        .unwrap_or_else(|_| cli_dispatch::error("internal error"));
        let _ = job.reply.send(response);
    }
}

/// `ToVolumeInt`: percent rounded half away from zero.
fn percent(volume: f32) -> i32 {
    (volume * 100.0).round() as i32
}

/// The CLI's view of the live audio state and the app's setters (which
/// record undo steps like the original's view models).
struct Live<'a> {
    core: &'a mut Core,
}

impl Provider for Live<'_> {
    fn version(&self) -> Option<String> {
        Some(VERSION.into())
    }

    fn devices(&mut self) -> Option<Vec<Device>> {
        let snapshot = self.core.audio.snapshot().ok()?;
        Some(
            snapshot
                .devices
                .into_iter()
                .map(|d| Device {
                    volume: percent(d.volume),
                    is_muted: d.muted,
                    is_default: d.is_default,
                    apps: d
                        .apps
                        .iter()
                        .map(|a| App {
                            app_id: rules::app_id(a),
                            exe_name: rules::exe_name(a),
                            key: a.key.clone(),
                            display_name: a.display_name.clone(),
                            process_id: a.process_ids.first().map_or(0, |&p| p as i32),
                            volume: percent(a.volume),
                            is_muted: a.muted,
                        })
                        .collect(),
                    id: d.id,
                    name: d.name,
                })
                .collect(),
        )
    }

    fn set_device_volume(&mut self, device: &Device, volume: i32) {
        self.core.set_device_volume(
            &device.id,
            &device.name,
            volume as f32 / 100.0,
            device.volume,
        );
    }

    fn set_device_mute(&mut self, device: &Device, muted: bool) {
        self.core.set_device_mute(&device.id, &device.name, muted);
    }

    fn set_app_volume(&mut self, device: &Device, app: &App, volume: i32) {
        self.core.set_app_volume(
            &device.id,
            &app.key,
            &app.display_name,
            volume as f32 / 100.0,
            app.volume,
        );
    }

    fn set_app_mute(&mut self, device: &Device, app: &App, muted: bool) {
        self.core
            .set_app_mute(&device.id, &app.key, &app.display_name, muted);
    }

    /// bt-audio sets the console and multimedia roles together; the result
    /// is verified against the default Windows then reports.
    fn set_default_device(&mut self, device: &Device) -> Result<(), String> {
        if self.core.audio.set_default_device(&device.id).is_err() {
            return Err(format!(
                "failed to set default device for eConsole: {}",
                device.name
            ));
        }
        let applied = self.core.audio.devices().ok().is_some_and(|s| {
            s.devices
                .iter()
                .any(|d| d.is_default && d.id.eq_ignore_ascii_case(&device.id))
        });
        if applied {
            Ok(())
        } else {
            Err(format!(
                "Windows did not apply the default device for eMultimedia: {}",
                device.name
            ))
        }
    }

    fn route_app(&mut self, app: &App, device: &Device) -> bool {
        self.core
            .audio
            .set_app_device(&app.key, Some(&device.id))
            .is_ok()
    }

    /// bt-audio routes by session, so a process without one cannot be routed.
    fn route_processes(&mut self, _name: &str, _device: &Device) -> usize {
        0
    }

    fn profiles_json(&self) -> Option<Option<String>> {
        Some(match self.core.store.get(Key::VolumeProfilesJson) {
            Value::Text(text) => text,
            _ => None,
        })
    }

    fn local_now_ticks(&self) -> i64 {
        clock::local_now_ms() * 10_000
    }
}
