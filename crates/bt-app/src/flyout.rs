//! Flyout commands backed by the settings: appearance, the expanded state,
//! hidden entries, the app popup's rules and undo/redo.
use crate::{
    App, error, rules,
    store::{self, Store, with_env},
};
use bt_app::win32::{clock, theme};
use bt_core::{
    app_rules::{Metadata, Mutation},
    colors::{self, Color, NumberCulture},
    hidden::AppKey,
    models::VolumeRuleMode,
    settings::{Key, Value},
    undo::{Change, Target},
};
use tauri::{AppHandle, Emitter, Manager, State};

/// A colour setting as CSS, or `None` for the original's "Transparent =
/// use the default".
fn css_color(store: &Store, key: &str) -> Option<String> {
    let c = colors::parse_setting(store.text(key).as_deref(), &NumberCulture::INVARIANT);
    let t = Color::TRANSPARENT;
    if (c.a, c.r, c.g, c.b) == (t.a, t.r, t.g, t.b) {
        return None;
    }
    Some(format!(
        "rgba({}, {}, {}, {:.3})",
        c.r,
        c.g,
        c.b,
        f64::from(c.a) / 255.0
    ))
}

/// Everything the flyout styles itself with: the Windows colours the
/// original resolves, and the user's appearance and behaviour settings.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlyoutConfig {
    accent: String,
    accent_dark1: String,
    transparency: bool,
    light: bool,
    accent_on_taskbar: bool,
    peak_meter_fps: i32,
    peak_meter_style: i32,
    use_volume_tick_sound: bool,
    use_smooth_volume_animation: bool,
    volume_animation_speed: f64,
    show_app_tooltips: bool,
    is_expanded: bool,
    /// UseCustomSliderColors; each colour `None` means its default.
    custom_colors: Option<CustomColors>,
}

#[derive(serde::Serialize)]
pub struct CustomColors {
    thumb: Option<String>,
    fill: Option<String>,
    track: Option<String>,
    peak: Option<String>,
}

#[tauri::command]
pub fn flyout_config(app: State<'_, App>) -> Result<FlyoutConfig, String> {
    let store = app.store.lock().map_err(error)?;
    let custom_colors = store
        .bool(Key::UseCustomSliderColors)
        .then(|| CustomColors {
            thumb: css_color(&store, "SliderThumbColor"),
            fill: css_color(&store, "SliderTrackFillColor"),
            track: css_color(&store, "SliderTrackBackgroundColor"),
            peak: css_color(&store, "PeakMeterColor"),
        });
    Ok(FlyoutConfig {
        accent: theme::accent(),
        accent_dark1: theme::accent_dark1(),
        transparency: theme::transparency(),
        light: theme::system_light(),
        accent_on_taskbar: theme::accent_on_taskbar(),
        peak_meter_fps: store.settings.effective_peak_fps(store::context()),
        peak_meter_style: store.int(Key::PeakMeterStyle),
        use_volume_tick_sound: store.bool(Key::UseVolumeTickSound),
        use_smooth_volume_animation: store.bool(Key::UseSmoothVolumeAnimation),
        volume_animation_speed: store.float(Key::VolumeAnimationSpeed),
        show_app_tooltips: store.bool(Key::ShowAppTooltips),
        is_expanded: store.bool(Key::IsExpanded),
        custom_colors,
    })
}

/// The header's expand arrow, remembered like the original's IsExpanded.
#[tauri::command]
pub fn set_expanded(app: State<'_, App>, expanded: bool) -> Result<(), String> {
    app.store
        .lock()
        .map_err(error)?
        .set(Key::IsExpanded, Value::Bool(expanded));
    Ok(())
}

/// An app entry's identity as the flyout sends it.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEntry {
    device_id: String,
    app_id: String,
    exe_name: String,
    #[serde(default)]
    display_name: String,
}

impl AppEntry {
    fn key(&self) -> AppKey<'_> {
        AppKey {
            device_id: Some(&self.device_id),
            app_id: Some(&self.app_id),
            exe_name: Some(&self.exe_name),
        }
    }
}

#[tauri::command]
pub fn hide_app(app: State<'_, App>, entry: AppEntry) -> Result<(), String> {
    let mut store = app.store.lock().map_err(error)?;
    with_env(|env| {
        store
            .hidden_apps
            .hide(entry.key(), Some(&entry.display_name), env)
    });
    store.flush();
    Ok(())
}

#[tauri::command]
pub fn unhide_app(app: State<'_, App>, entry: AppEntry) -> Result<(), String> {
    let mut store = app.store.lock().map_err(error)?;
    with_env(|env| store.hidden_apps.unhide(entry.key(), env));
    store.flush();
    Ok(())
}

/// "Restore all on this device".
#[tauri::command]
pub fn unhide_device_apps(app: State<'_, App>, device_id: String) -> Result<(), String> {
    let mut store = app.store.lock().map_err(error)?;
    with_env(|env| store.hidden_apps.unhide_device(Some(&device_id), env));
    store.flush();
    Ok(())
}

#[tauri::command]
pub fn hide_device(app: State<'_, App>, device_id: String, name: String) -> Result<(), String> {
    let mut store = app.store.lock().map_err(error)?;
    with_env(|env| {
        store
            .hidden_devices
            .hide(Some(&device_id), Some(&name), env)
    });
    store.flush();
    Ok(())
}

/// One of the app popup's rule buttons (FocusedAppItemViewModel). `kind` is
/// "hardMute" or "focusLost" (toggles), "lock" or "launch" (at `value`
/// percent), or "none" (clears the volume rule).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleChange {
    device_id: String,
    app_key: String,
    exe_name: String,
    display_name: String,
    icon_path: Option<String>,
    kind: String,
    #[serde(default)]
    value: i32,
}

#[tauri::command]
pub async fn app_rule(handle: AppHandle, change: RuleChange) -> Result<(), String> {
    let app = handle.state::<App>();
    let snapshot = app.audio.snapshot().map_err(error)?;
    let session = snapshot
        .devices
        .iter()
        .find(|d| d.id == change.device_id)
        .and_then(|d| d.apps.iter().find(|a| a.key == change.app_key));
    let mut store = app.store.lock().map_err(error)?;
    let exe = Some(change.exe_name.as_str());
    let current = with_env(|env| store.rules.get(exe, env));
    let was_hard_muted = current.as_ref().is_some_and(|r| r.hard_muted);
    let mutation = match change.kind.as_str() {
        "hardMute" => Mutation::HardMute(!was_hard_muted),
        "focusLost" => Mutation::FocusLost(!current.is_some_and(|r| r.focus_lost_enabled)),
        "lock" => Mutation::Volume(VolumeRuleMode::LOCK, change.value),
        "launch" => Mutation::Volume(VolumeRuleMode::LAUNCH, change.value),
        "none" => Mutation::Volume(VolumeRuleMode::NONE, 0),
        other => return Err(format!("unknown rule {other}")),
    };
    with_env(|env| {
        store.rules.change(
            Metadata {
                exe_name: exe,
                display_name: Some(&change.display_name),
                icon_path: change.icon_path.as_deref(),
                is_desktop_app: Some(true),
            },
            mutation,
            env,
        )
    });
    store.flush();
    // Turning hard mute off unmutes the app the user is acting on.
    if was_hard_muted && change.kind == "hardMute" && session.is_some_and(|s| s.muted) {
        let _ = app
            .audio
            .set_app_mute(&change.device_id, &change.app_key, false);
    }
    let mut tracker = app.launch.lock().map_err(error)?;
    // A new volume rule applies right away, Launch included.
    if let Some(session) = session.filter(|_| matches!(change.kind.as_str(), "lock" | "launch")) {
        tracker.release(&session.process_ids);
    }
    rules::enforce(&app.audio, &mut store, &mut tracker, &snapshot);
    drop((store, tracker));
    let _ = handle.emit("volume-changed", ());
    Ok(())
}

pub fn undo_target(id: &str, name: &str, is_device: bool) -> Target {
    Target {
        id: id.into(),
        display_name: name.into(),
        is_device,
    }
}

pub fn record_volume(app: &App, target: Target, old: i32, new: i32) {
    if let Ok(mut undo) = app.undo.lock() {
        undo.record_volume_change(target, old, new, clock::local_now_ms());
    }
}

fn record_mute(app: &App, target: Target, old: bool, new: bool) {
    if let Ok(mut undo) = app.undo.lock() {
        undo.record_mute_change(target, old, new, clock::local_now_ms());
    }
}

fn percent(volume: f32) -> i32 {
    (volume * 100.0).round() as i32
}

// Volume and mute changes from the flyout. `previous` (0..100) or `name`
// records an undo step, as the original's view models do for every change
// the user makes.
#[tauri::command]
pub async fn set_device_volume(
    app: State<'_, App>,
    device_id: String,
    volume: f32,
    previous: Option<i32>,
    name: Option<String>,
) -> Result<(), String> {
    if let Some(old) = previous {
        let target = undo_target(&device_id, name.as_deref().unwrap_or_default(), true);
        record_volume(&app, target, old, percent(volume));
    }
    app.audio
        .set_device_volume(&device_id, volume)
        .map_err(error)
}
#[tauri::command]
pub async fn set_device_mute(
    app: State<'_, App>,
    device_id: String,
    muted: bool,
    name: Option<String>,
) -> Result<(), String> {
    if let Some(name) = name {
        record_mute(&app, undo_target(&device_id, &name, true), !muted, muted);
    }
    app.audio.set_device_mute(&device_id, muted).map_err(error)
}
#[tauri::command]
pub async fn set_app_volume(
    app: State<'_, App>,
    device_id: String,
    app_key: String,
    volume: f32,
    previous: Option<i32>,
    name: Option<String>,
) -> Result<(), String> {
    if let Some(old) = previous {
        let target = undo_target(&app_key, name.as_deref().unwrap_or_default(), false);
        record_volume(&app, target, old, percent(volume));
    }
    app.audio
        .set_app_volume(&device_id, &app_key, volume)
        .map_err(error)
}
#[tauri::command]
pub async fn set_app_mute(
    app: State<'_, App>,
    device_id: String,
    app_key: String,
    muted: bool,
    name: Option<String>,
) -> Result<(), String> {
    if let Some(name) = name {
        record_mute(&app, undo_target(&app_key, &name, false), !muted, muted);
    }
    app.audio
        .set_app_mute(&device_id, &app_key, muted)
        .map_err(error)
}

/// Ctrl+Z / Ctrl+Y: re-applies the old or new value to the device, or to
/// the first device the app is on (UndoRedoHelper).
fn replay(handle: &AppHandle, is_undo: bool) -> Result<bool, String> {
    let app = handle.state::<App>();
    let action = {
        let mut undo = app.undo.lock().map_err(error)?;
        if is_undo { undo.undo() } else { undo.redo() }
    };
    let Some(action) = action else {
        return Ok(false);
    };
    let target = &action.target;
    let device_id = if target.is_device {
        Some(target.id.clone())
    } else {
        app.audio
            .snapshot()
            .map_err(error)?
            .devices
            .iter()
            .find(|d| d.apps.iter().any(|a| a.key == target.id))
            .map(|d| d.id.clone())
    };
    // The original skips targets that have gone away.
    let Some(device_id) = device_id else {
        return Ok(true);
    };
    let audio = &app.audio;
    let result = match (action.change, target.is_device) {
        (Change::Volume { old, new }, true) => {
            audio.set_device_volume(&device_id, pick(is_undo, old, new) as f32 / 100.0)
        }
        (Change::Volume { old, new }, false) => audio.set_app_volume(
            &device_id,
            &target.id,
            pick(is_undo, old, new) as f32 / 100.0,
        ),
        (Change::Mute { old, new }, true) => {
            audio.set_device_mute(&device_id, pick(is_undo, old, new))
        }
        (Change::Mute { old, new }, false) => {
            audio.set_app_mute(&device_id, &target.id, pick(is_undo, old, new))
        }
    };
    let _ = handle.emit("volume-changed", ());
    result.map(|()| true).map_err(error)
}

fn pick<T>(is_undo: bool, old: T, new: T) -> T {
    if is_undo { old } else { new }
}

#[tauri::command]
pub async fn undo(handle: AppHandle) -> Result<bool, String> {
    replay(&handle, true)
}
#[tauri::command]
pub async fn redo(handle: AppHandle) -> Result<bool, String> {
    replay(&handle, false)
}

/// One process's session (the app popup's rows). The original cannot undo
/// these: its undo targets whole apps.
#[tauri::command]
pub async fn set_process_volume(
    app: State<'_, App>,
    device_id: String,
    app_key: String,
    process_id: u32,
    volume: f32,
) -> Result<(), String> {
    app.audio
        .set_process_volume(&device_id, &app_key, process_id, volume)
        .map_err(error)
}
#[tauri::command]
pub async fn set_process_mute(
    app: State<'_, App>,
    device_id: String,
    app_key: String,
    process_id: u32,
    muted: bool,
) -> Result<(), String> {
    app.audio
        .set_process_mute(&device_id, &app_key, process_id, muted)
        .map_err(error)
}

/// The flyout window's DWM acrylic tint (AcrylicColor_Flyout): accent mode
/// SystemAccentDark1 at 0.8, light LightChromeMedium (#E6E6E6) at 0.8,
/// dark ApplicationBackgroundDarkTheme (#000) at 0.7. With transparency
/// effects off the page paints a solid colour instead.
pub fn apply_backdrop(window: &tauri::WebviewWindow) {
    use tauri::window::{Color, Effect, EffectsBuilder};
    if !theme::transparency() {
        let _ = window.set_effects(None);
        return;
    }
    let light = theme::system_light();
    let tint = if theme::accent_on_taskbar() && !light {
        let hex = theme::accent_dark1();
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
        Color(byte(1), byte(3), byte(5), 204)
    } else if light {
        Color(230, 230, 230, 204)
    } else {
        Color(0, 0, 0, 179)
    };
    let _ = window.set_effects(
        EffectsBuilder::new()
            .effect(Effect::Acrylic)
            .color(tint)
            .build(),
    );
}
