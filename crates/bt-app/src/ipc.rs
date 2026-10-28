//! The CLI pipe (`bt`): the pipe thread hands each command to the UI thread
//! (a queue plus an `Event::App` post to the hidden window) and waits for the
//! answer, like the original's DispatchToUI. Ping is answered on the pipe
//! thread, as in the original.
use crate::{core::Core, profiles, rules, settings_file};
use bt_app::win32::{
    clock,
    pipe::{self, PipeServer},
    wnd::Poster,
};
use bt_audio::Role;
use bt_core::{
    cli_dispatch::{self, App, Device, Provider, json},
    cli_presets,
    cli_responses::{ApplyProfileResponse, CreatePresetResponse, DeleteResponse},
    models::{AppVolumeEntry, CaptureScope, VolumeProfile},
    settings::{Key, Value},
    text::is_blank,
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

/// What the commands left for the UI to do.
#[derive(Default)]
pub struct Effects {
    /// Presets were saved or deleted: register their shortcuts again.
    pub hotkeys: bool,
    /// Routed preset apps to re-apply once their move has settled.
    pub settle: Vec<AppVolumeEntry>,
    /// Settings were imported: everything that caches them reloads.
    pub reload: bool,
}

/// Runs the waiting pipe commands (UI thread).
pub fn drain(core: &mut Core) -> Effects {
    let mut effects = Effects::default();
    loop {
        let Some(job) = JOBS.lock().ok().and_then(|mut jobs| jobs.pop_front()) else {
            return effects;
        };
        let response = catch_unwind(AssertUnwindSafe(|| {
            let mut live = Live {
                core: &mut *core,
                effects: &mut effects,
            };
            cli_dispatch::process(&mut live, &job.line)
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
    effects: &'a mut Effects,
}

/// CliHandler's QuickTrumpet commands over `profiles::Service`.
impl Live<'_> {
    fn audio_ready(&self) -> bool {
        self.core.audio.snapshot().is_ok()
    }

    /// ListProfiles.
    fn list_profiles(&mut self) -> String {
        let stored = self.core.store.text(Key::VolumeProfilesJson.storage_key());
        let stored = stored.as_deref().unwrap_or_default();
        if is_blank(Some(stored)) || stored == "[]" {
            return "[]".into();
        }
        let last = self
            .core
            .store
            .text(Key::QuickTrumpetLastAppliedId.storage_key());
        let presets = profiles::read(&self.core.store);
        json(&cli_presets::list(
            &presets,
            last.as_deref(),
            profiles::describe_profile,
        ))
    }

    /// ApplyProfile (also a bare preset name).
    fn apply_profile(&mut self, args: &[String]) -> String {
        if args.is_empty() {
            return cli_dispatch::error("usage: apply-profile NAME");
        }
        let name = args.join(" ");
        if !self.audio_ready() {
            return cli_dispatch::error("audio not ready");
        }
        let service = profiles::Service::open(self.core);
        let Some(index) = service.find(&name) else {
            return cli_dispatch::error(format!("QuickTrumpet preset not found: {name}"));
        };
        self.apply_index(index)
    }

    /// CycleProfile.
    fn cycle(&mut self, direction: i32) -> String {
        if !self.audio_ready() {
            return cli_dispatch::error("audio not ready");
        }
        let service = profiles::Service::open(self.core);
        let Some(index) = service.cycle_target(direction) else {
            return cli_dispatch::error("no QuickTrumpet presets saved");
        };
        self.apply_index(index)
    }

    /// ApplyProfile, then DescribeApplyResult.
    fn apply_index(&mut self, index: usize) -> String {
        let mut service = profiles::Service::open(self.core);
        let Some(profile) = service.profiles().get(index).cloned() else {
            return cli_dispatch::error("no QuickTrumpet presets saved");
        };
        let result = service.apply(index);
        self.effects.settle.extend(result.settle.iter().cloned());
        json(&ApplyProfileResponse {
            ok: true,
            preset: profile.name.clone(),
            slug: bt_core::profiles::get_slug(&profile),
            defaults_applied: result
                .defaults_applied
                .iter()
                .map(|(role, id, name)| {
                    cli_presets::applied_default(
                        profiles::role_of(*role),
                        name.clone(),
                        Some(id.clone()),
                    )
                })
                .collect(),
            devices_applied: result.devices_applied as i32,
            devices_missing: result.devices_missing.clone(),
            apps_applied: result.apps_applied as i32,
            apps_missing: result.apps_missing as i32,
            apps_routed: result.apps_routed as i32,
            summary: profiles::describe_result(&result),
            warnings: result.warnings.clone(),
        })
    }

    /// SaveProfile (Q29): the legacy capture, then the flags.
    fn save(&mut self, args: &[String]) -> String {
        let request = match cli_presets::parse_save(args) {
            Ok(request) => request,
            Err(e) => return cli_dispatch::error(e),
        };
        if !self.audio_ready() {
            return cli_dispatch::error("audio not ready");
        }
        let mut service = profiles::Service::open(self.core);
        let scope = if request.all_devices {
            CaptureScope::ALL_DEVICES
        } else {
            CaptureScope::CURRENT_DEVICE
        };
        let mut profile = service.capture_current_state(&request.name, scope);
        // ApplyAppsOnly's setter.
        profile.include_device_volumes = !request.apps_only;
        if request.no_apps {
            profile.include_app_volumes = false;
        }
        if request.no_route {
            profile.route_apps = false;
        }
        if request.defaults {
            for role in Role::ALL {
                service.capture_default(&mut profile, role);
            }
        }
        let saved = save_profile(&mut service, profile);
        self.effects.hotkeys = true;
        json(&cli_presets::saved(
            &saved,
            profiles::describe_profile(&saved),
        ))
    }

    /// DeleteProfile.
    fn delete(&mut self, args: &[String]) -> String {
        if args.is_empty() {
            return cli_dispatch::error("usage: delete NAME");
        }
        let name = args.join(" ");
        let mut service = profiles::Service::open(self.core);
        let Some(index) = service.find(&name) else {
            return cli_dispatch::error(format!("QuickTrumpet preset not found: {name}"));
        };
        let preset = service.profiles()[index].name.clone();
        service.delete(index);
        self.effects.hotkeys = true;
        json(&DeleteResponse { ok: true, preset })
    }

    /// CreatePresetFromRule: the rule is applied live, then captured.
    fn create(&mut self, args: &[String]) -> String {
        let (name, rule) = match cli_presets::split_create(args) {
            Ok(split) => split,
            Err(e) => return cli_dispatch::error(e),
        };
        let (spec, plan) = match cli_dispatch::apply_rule(self, rule) {
            Ok(applied) => applied,
            Err(e) => return cli_dispatch::error(e),
        };
        let mut service = profiles::Service::open(self.core);
        let scope = if spec.capture_all_devices {
            CaptureScope::ALL_DEVICES
        } else {
            CaptureScope::CURRENT_DEVICE
        };
        let mut profile = service.capture_current_state(name, scope);
        profile.include_device_volumes = !spec.apply_apps_only;
        let saved = save_profile(&mut service, profile);
        self.effects.hotkeys = true;
        let apps_captured = saved
            .devices
            .iter()
            .flatten()
            .flatten()
            .map(|d| d.apps.as_ref().map_or(0, Vec::len))
            .sum();
        json(&CreatePresetResponse {
            ok: true,
            preset: saved.name.clone(),
            slug: saved.slug.clone(),
            capture_scope: cli_presets::scope_name(saved.capture_scope),
            apply_apps_only: saved.apply_apps_only(),
            apps_changed: plan.len(),
            apps_captured,
            warnings: bt_core::cli_rules::warnings(&spec, &plan),
            changes: plan,
        })
    }
}

/// SaveProfile, answering the stored preset.
fn save_profile(service: &mut profiles::Service, profile: VolumeProfile) -> VolumeProfile {
    let fallback = profile.clone();
    let index = service.save(profile);
    service.profiles().get(index).cloned().unwrap_or(fallback)
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

    fn extension(&mut self, command: &str, args: &[String]) -> Option<String> {
        Some(match command {
            "list-profiles" | "presets" => self.list_profiles(),
            "apply-profile" | "apply" => self.apply_profile(args),
            "save" => self.save(args),
            "preset-next" => self.cycle(1),
            "preset-prev" | "preset-previous" => self.cycle(-1),
            "delete" => self.delete(args),
            "preset-create" => self.create(args),
            "export-settings" | "import-settings" => {
                let mut files = settings_file::Files {
                    core: &mut *self.core,
                    imported: false,
                };
                let response = if command == "export-settings" {
                    cli_dispatch::export_settings(&mut files, args)
                } else {
                    cli_dispatch::import_settings(&mut files, args)
                };
                self.effects.reload |= files.imported;
                response
            }
            _ => return None,
        })
    }
}
