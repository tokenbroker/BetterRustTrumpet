//! CliHandler.ProcessCommand over a live provider: the app supplies devices,
//! apps and setters; parsing, matching, error text and the JSON responses
//! come from the ported CLI modules. Runs on the app's UI thread, like the
//! original's DispatchToUI.
use crate::{
    cli,
    cli_responses::{
        self as responses, AppResponse, DefaultResponse, DeviceApp, DeviceResponse,
        ErrorResponse, GetVolumeResponse, HealthAudio, HealthSettings, MuteResponse,
        RouteResponse, SetDefaultResponse, SetVolumeResponse, Target, WatchApp, WatchDevice,
        WatchResponse,
    },
    cli_rules,
    cli_validation::{self, AppCandidate, CommandCulture, Volume},
    identity::filename_without_extension,
    newtonsoft::civil_from_days,
    numbers::IntegerFormat,
    text::{invariant_lower, is_blank, ordinal_contains, ordinal_ignore_case, ordinal_key},
};
use serde::Serialize;
use std::collections::HashSet;

/// One app on a device (IAppItemViewModel).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct App {
    /// AppId: the process image path, or the system sounds id.
    pub app_id: String,
    /// The provider's own handle for setters (session group key).
    pub key: String,
    pub exe_name: String,
    pub display_name: String,
    pub process_id: i32,
    pub volume: i32,
    pub is_muted: bool,
}

/// One output device (DeviceViewModel).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub volume: i32,
    pub is_muted: bool,
    pub is_default: bool,
    pub apps: Vec<App>,
}

/// The live side of the CLI. Setters are only called with a changed value
/// (the original view-model setters skip unchanged values and record undo).
pub trait Provider {
    /// App.PackageVersion; `None` prints "unknown".
    fn version(&self) -> Option<String>;
    /// Every device with its apps, or `None` when audio is not ready.
    fn devices(&mut self) -> Option<Vec<Device>>;
    fn set_device_volume(&mut self, device: &Device, volume: i32);
    fn set_device_mute(&mut self, device: &Device, muted: bool);
    fn set_app_volume(&mut self, device: &Device, app: &App, volume: i32);
    fn set_app_mute(&mut self, device: &Device, app: &App, muted: bool);
    /// Makes `device` the default for the console and multimedia roles and
    /// verifies it; the error is the response's message.
    fn set_default_device(&mut self, device: &Device) -> Result<(), String>;
    /// Routes `app` to `device` (SetDefaultEndPoint). Returns success.
    fn route_app(&mut self, app: &App, device: &Device) -> bool;
    /// SetDeviceForApp's fallback for apps without a session: routes every
    /// process named `name` and returns how many were routed.
    fn route_processes(&mut self, name: &str, device: &Device) -> usize;
    /// `None` when settings are unavailable, else VolumeProfilesJson.
    fn profiles_json(&self) -> Option<Option<String>>;
    /// `DateTime.Now` in .NET ticks.
    fn local_now_ticks(&self) -> i64;
    /// Commands whose feature lives outside this module (profiles, update
    /// check, settings export/import). `None` reports it as unavailable.
    fn extension(&mut self, command: &str, args: &[String]) -> Option<String> {
        let _ = (command, args);
        None
    }
}

/// Culture boundary (D29). The installed dispatcher uses invariant signs and
/// ordinal prefixes until a native current-culture adapter is accepted.
pub struct InvariantCulture;
impl CommandCulture for InvariantCulture {
    fn integer_format(&self) -> IntegerFormat<'_> {
        IntegerFormat::INVARIANT
    }
    fn starts_with(&self, value: &str, prefix: &str) -> bool {
        value.starts_with(prefix)
    }
}

/// Commands whose feature is not ported: they answer this error.
pub const UNAVAILABLE: [&str; 13] = [
    "list-profiles",
    "presets",
    "apply-profile",
    "apply",
    "save",
    "preset-next",
    "preset-prev",
    "preset-previous",
    "delete",
    "preset-create",
    "check-update",
    "export-settings",
    "import-settings",
];

pub fn error(message: impl Into<String>) -> String {
    json(&ErrorResponse::from(message.into()))
}

fn json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|e| error(e.to_string()))
}

fn unavailable(command: &str) -> String {
    error(format!("{command} is not available in BetterTrumpet Rust yet"))
}

/// The ping answer, or `None` when `line` is another command. The original
/// answers ping on the pipe thread without dispatching to the UI.
pub fn ping(line: &str, version: Option<&str>) -> Option<String> {
    let parts = cli::normalize(cli::tokenize(line));
    let first = parts.first()?;
    (invariant_lower(first) == "ping").then(|| json(&responses::ping(version)))
}

/// CliHandler.ProcessCommand.
pub fn process(provider: &mut impl Provider, line: &str) -> String {
    if is_blank(Some(line)) {
        return error("empty command");
    }
    let parts = cli::tokenize(line);
    if parts.is_empty() {
        return error("empty command");
    }
    let parts = cli::normalize(parts);
    let command = invariant_lower(&parts[0]);
    let args = &parts[1..];
    match command.as_str() {
        "ping" => json(&responses::ping(provider.version().as_deref())),
        "doctor" => doctor(provider),
        "batch" => match responses::batch(args, |line| process(provider, line)) {
            Ok(response) => json(&response),
            Err(e) => json(&e),
        },
        "list-devices" => list_devices(provider),
        "list-apps" => list_apps(provider),
        "resolve-apps" => {
            let collection = provider.devices().map(|d| collection(&d));
            match cli_rules::resolve_command(collection.as_ref(), args) {
                Ok(response) => json(&response),
                Err(e) => error(e),
            }
        }
        "get-volume" => get_volume(provider, args),
        "set-volume" => set_volume(provider, args),
        "mute" => set_mute(provider, args, Some(true)),
        "unmute" => set_mute(provider, args, Some(false)),
        "toggle-mute" => set_mute(provider, args, None),
        "get-default" => get_default(provider),
        "set-default" => set_default(provider, args),
        "set-device" => set_device(provider, args),
        "rule-preview" => {
            let collection = provider.devices().map(|d| collection(&d));
            match cli_rules::preview_command(collection.as_ref(), args) {
                Ok(response) => json(&response),
                Err(e) => error(e),
            }
        }
        "rule-apply" => rule_apply(provider, args),
        "watch" => watch(provider),
        c if UNAVAILABLE.contains(&c) => provider
            .extension(c, args)
            .unwrap_or_else(|| unavailable(c)),
        // Anything else is a preset name: ApplyProfile(parts).
        _ => provider
            .extension("apply", &parts)
            .unwrap_or_else(|| unavailable("apply")),
    }
}

fn list_devices(provider: &mut impl Provider) -> String {
    let Some(devices) = provider.devices() else {
        return error("audio not ready");
    };
    json(
        &devices
            .iter()
            .map(|d| DeviceResponse {
                id: Some(d.id.clone()),
                name: Some(d.name.clone()),
                volume: d.volume,
                is_muted: d.is_muted,
                is_default: d.is_default,
                apps: d
                    .apps
                    .iter()
                    .map(|a| DeviceApp {
                        id: Some(a.app_id.clone()),
                        name: Some(a.display_name.clone()),
                        exe_name: Some(a.exe_name.clone()),
                        volume: a.volume,
                        is_muted: a.is_muted,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>(),
    )
}

fn list_apps(provider: &mut impl Provider) -> String {
    let Some(devices) = provider.devices() else {
        return error("audio not ready");
    };
    // Deduplicated by exe name (OrdinalIgnoreCase): apps can be on several devices.
    let mut seen = HashSet::new();
    let apps: Vec<_> = devices
        .iter()
        .flat_map(|d| d.apps.iter().map(move |a| (d, a)))
        .filter(|(_, a)| seen.insert(ordinal_key(&a.exe_name)))
        .map(|(d, a)| AppResponse {
            exe_name: Some(a.exe_name.clone()),
            display_name: Some(a.display_name.clone()),
            volume: a.volume,
            is_muted: a.is_muted,
            device: Some(d.name.clone()),
            process_id: a.process_id,
        })
        .collect();
    json(&apps)
}

/// ResolveDevice: the default, or `--device` by exact id, then by name part.
fn resolve_device(devices: &[Device], args: &[String]) -> Option<usize> {
    match cli::get_arg(args, "--device") {
        None => devices.iter().position(|d| d.is_default),
        Some(arg) => devices
            .iter()
            .position(|d| d.id == arg)
            .or_else(|| devices.iter().position(|d| ordinal_contains(&d.name, arg))),
    }
}

/// FindApp over every device's apps: exact matches before partial ones.
fn find_app(devices: &[Device], name: &str) -> Option<(usize, usize)> {
    let all: Vec<(usize, usize)> = devices
        .iter()
        .enumerate()
        .flat_map(|(i, d)| (0..d.apps.len()).map(move |j| (i, j)))
        .collect();
    let candidates: Vec<_> = all
        .iter()
        .map(|&(i, j)| AppCandidate {
            exe_name: Some(&devices[i].apps[j].exe_name),
            display_name: Some(&devices[i].apps[j].display_name),
        })
        .collect();
    cli_validation::find_app(&candidates, name).map(|k| all[k])
}

fn app_matches(app: &App, name: &str) -> bool {
    let candidate = AppCandidate {
        exe_name: Some(&app.exe_name),
        display_name: Some(&app.display_name),
    };
    cli_validation::find_app(&[candidate], name).is_some()
}

/// Device lookup by display name: exact (ignoring case), then contained.
fn device_named(devices: &[Device], name: &str) -> Option<usize> {
    devices
        .iter()
        .position(|d| ordinal_ignore_case(&d.name, name))
        .or_else(|| devices.iter().position(|d| ordinal_contains(&d.name, name)))
}

fn get_volume(provider: &mut impl Provider, args: &[String]) -> String {
    let devices = provider.devices().unwrap_or_default();
    let Some(i) = resolve_device(&devices, args) else {
        return error("device not found");
    };
    let d = &devices[i];
    json(&GetVolumeResponse {
        device: Some(d.name.clone()),
        volume: d.volume,
        is_muted: d.is_muted,
    })
}

fn set_volume(provider: &mut impl Provider, args: &[String]) -> String {
    let request = match cli_validation::set_volume(args, &InvariantCulture) {
        Ok(request) => request,
        Err(e) => return error(e),
    };
    let delta = match request.volume {
        Volume::Relative(delta) => Some(delta),
        Volume::Absolute(_) => None,
    };
    if let Some(name) = request.app {
        let Some(devices) = provider.devices() else {
            return error("audio not ready");
        };
        let Some((i, j)) = find_app(&devices, &name) else {
            return error(format!("app not found: {name}"));
        };
        let (device, app) = (&devices[i], &devices[i].apps[j]);
        let volume = request.volume.apply(app.volume);
        if volume != app.volume {
            provider.set_app_volume(device, app, volume);
        }
        return json(&SetVolumeResponse {
            ok: true,
            target: Target::App(Some(app.display_name.clone())),
            volume,
            delta,
        });
    }
    let devices = provider.devices().unwrap_or_default();
    let Some(i) = resolve_device(&devices, args) else {
        return error("device not found");
    };
    let device = &devices[i];
    let volume = request.volume.apply(device.volume);
    if volume != device.volume {
        provider.set_device_volume(device, volume);
    }
    json(&SetVolumeResponse {
        ok: true,
        target: Target::Device(Some(device.name.clone())),
        volume,
        delta,
    })
}

/// SetMute (`Some`) and ToggleMute (`None`).
fn set_mute(provider: &mut impl Provider, args: &[String], mute: Option<bool>) -> String {
    let devices = provider.devices().unwrap_or_default();
    if let Some(name) = cli::get_arg(args, "--app") {
        let Some((i, j)) = find_app(&devices, name) else {
            return error(format!("app not found: {name}"));
        };
        let (device, app) = (&devices[i], &devices[i].apps[j]);
        let muted = mute.unwrap_or(!app.is_muted);
        if muted != app.is_muted {
            provider.set_app_mute(device, app, muted);
        }
        return json(&MuteResponse {
            ok: true,
            target: Target::App(Some(app.display_name.clone())),
            is_muted: muted,
        });
    }
    let Some(i) = resolve_device(&devices, args) else {
        return error("device not found");
    };
    let device = &devices[i];
    let muted = mute.unwrap_or(!device.is_muted);
    if muted != device.is_muted {
        provider.set_device_mute(device, muted);
    }
    json(&MuteResponse {
        ok: true,
        target: Target::Device(Some(device.name.clone())),
        is_muted: muted,
    })
}

fn get_default(provider: &mut impl Provider) -> String {
    let Some(devices) = provider.devices() else {
        return error("audio not ready");
    };
    let Some(d) = devices.iter().find(|d| d.is_default) else {
        return error("no default device");
    };
    json(&DefaultResponse {
        id: Some(d.id.clone()),
        name: Some(d.name.clone()),
        volume: d.volume,
        is_muted: d.is_muted,
    })
}

fn set_default(provider: &mut impl Provider, args: &[String]) -> String {
    if args.is_empty() {
        return error("usage: set-default DEVICE_NAME");
    }
    let name = args.join(" ");
    let Some(devices) = provider.devices() else {
        return error("audio not ready");
    };
    let Some(i) = device_named(&devices, &name) else {
        return error(format!("device not found: {name}"));
    };
    let device = &devices[i];
    if let Err(e) = provider.set_default_device(device) {
        return error(e);
    }
    json(&SetDefaultResponse {
        ok: true,
        name: Some(device.name.clone()),
        id: Some(device.id.clone()),
    })
}

fn set_device(provider: &mut impl Provider, args: &[String]) -> String {
    if args.len() < 2 {
        return error("usage: set-device APP_EXE DEVICE_NAME");
    }
    let app_exe = &args[0];
    let device_name = args[1..].join(" ");
    let Some(devices) = provider.devices() else {
        return error("audio not ready");
    };
    let Some(target) = device_named(&devices, &device_name) else {
        return error(format!("device not found: {device_name}"));
    };
    let target = &devices[target];
    let mut routed = 0;
    let mut routed_apps: Vec<String> = vec![];
    for app in devices
        .iter()
        .flat_map(|d| &d.apps)
        .filter(|a| app_matches(a, app_exe))
    {
        if provider.route_app(app, target) {
            routed += 1;
            if !routed_apps.contains(&app.display_name) {
                routed_apps.push(app.display_name.clone());
            }
        }
    }
    // Apps that are running but not playing have no session yet.
    if routed == 0 {
        let name = filename_without_extension(app_exe);
        let count = provider.route_processes(&name, target);
        routed += count;
        routed_apps.extend(std::iter::repeat_n(name, count));
    }
    if routed == 0 {
        return error(format!("app not found or no processes running: {app_exe}"));
    }
    json(&RouteResponse {
        ok: true,
        app: Some(
            routed_apps
                .into_iter()
                .next()
                .unwrap_or_else(|| app_exe.clone()),
        ),
        device: Some(target.name.clone()),
        processes_routed: routed as i32,
    })
}

/// The rule planner's snapshot of the live devices.
pub fn collection(devices: &[Device]) -> cli_rules::Collection {
    cli_rules::Collection {
        all_devices: Some(
            devices
                .iter()
                .map(|d| {
                    Some(cli_rules::Device {
                        id: Some(d.id.clone()),
                        display_name: Some(d.name.clone()),
                        apps: Some(
                            d.apps
                                .iter()
                                .map(|a| {
                                    Some(cli_rules::App {
                                        app_id: Some(a.app_id.clone()),
                                        exe_name: Some(a.exe_name.clone()),
                                        display_name: Some(a.display_name.clone()),
                                        process_id: a.process_id,
                                        volume: a.volume,
                                        is_muted: a.is_muted,
                                    })
                                })
                                .collect(),
                        ),
                    })
                })
                .collect(),
        ),
    }
}

/// ApplyRule: the plan is applied to a snapshot by the ported planner, then
/// every app whose volume or mute ended up different is set live.
fn rule_apply(provider: &mut impl Provider, args: &[String]) -> String {
    let Some(devices) = provider.devices() else {
        return error("audio not ready");
    };
    let before = collection(&devices);
    let mut after = before.clone();
    let response = match cli_rules::apply_command(Some(&mut after), args) {
        Ok(response) => response,
        Err(e) => return error(e),
    };
    let rows = |c: &cli_rules::Collection| -> Vec<(i32, bool)> {
        c.all_devices
            .iter()
            .flatten()
            .flatten()
            .flat_map(|d| d.apps.iter().flatten().flatten())
            .map(|a| (a.volume, a.is_muted))
            .collect()
    };
    let changed = rows(&before).into_iter().zip(rows(&after));
    let apps = devices
        .iter()
        .flat_map(|d| d.apps.iter().map(move |a| (d, a)));
    for ((device, app), ((volume, muted), (new_volume, new_muted))) in apps.zip(changed) {
        if new_volume != volume {
            provider.set_app_volume(device, app, new_volume);
        }
        if new_muted != muted {
            provider.set_app_mute(device, app, new_muted);
        }
    }
    json(&response)
}

/// `DateTime.Now.ToString("yyyy-MM-ddTHH:mm:ss.fffZ")`: local time with a
/// literal Z (Q01).
pub fn watch_timestamp(local_ticks: i64) -> String {
    const TICKS_PER_DAY: i64 = 864_000_000_000;
    let (year, month, day) = civil_from_days(local_ticks / TICKS_PER_DAY);
    let ms = local_ticks % TICKS_PER_DAY / 10_000;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

fn watch(provider: &mut impl Provider) -> String {
    let Some(devices) = provider.devices() else {
        return error("audio not ready");
    };
    json(&WatchResponse {
        timestamp: watch_timestamp(provider.local_now_ticks()),
        devices: devices
            .iter()
            .map(|d| WatchDevice {
                name: Some(d.name.clone()),
                volume: d.volume,
                is_muted: d.is_muted,
                is_default: d.is_default,
                app_count: d.apps.len(),
                apps: d
                    .apps
                    .iter()
                    .map(|a| WatchApp {
                        exe_name: Some(a.exe_name.clone()),
                        name: Some(a.display_name.clone()),
                        volume: a.volume,
                        is_muted: a.is_muted,
                        process_id: a.process_id,
                    })
                    .collect(),
            })
            .collect(),
    })
}

fn doctor(provider: &mut impl Provider) -> String {
    let devices = provider.devices();
    let settings = provider.profiles_json();
    let counts: Option<Vec<Option<usize>>> = devices
        .as_ref()
        .map(|d| d.iter().map(|d| Some(d.apps.len())).collect());
    let default = devices
        .as_ref()
        .and_then(|d| d.iter().find(|d| d.is_default))
        .map(|d| d.name.as_str());
    // The Rust app exits at startup without its audio engine, so the device
    // manager is always present.
    json(&responses::doctor(
        provider.version().as_deref(),
        counts.as_ref().map(|counts| HealthAudio {
            default_device: default,
            device_app_counts: Some(counts),
        }),
        true,
        settings.as_ref().map(|json| HealthSettings {
            profiles_json: json.as_deref(),
        }),
    ))
}
