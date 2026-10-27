//! BetterTrumpet Rust port — preview tray app.
//!
//! Daily-use features first: a tray flyout with every output device and its
//! apps (volume, mute, peaks, names and icons), default-device switching from
//! the flyout and the tray menu, a live tooltip, single instance and
//! "Start with Windows". Everything else in PARITY.md is still to come.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hotkeys;

use bt_app::win32::{icons, startup, webview, wheel};
use bt_audio::{AudioEngine, Snapshot};
use bt_media::{Command as MediaCommand, MediaEngine, NowPlaying};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalRect, Rect, State, WebviewWindow,
    WindowEvent,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const FLYOUT: &str = "flyout";
const MEDIA: &str = "media";
const TIP: &str = "tip";
/// The original's default MediaPopupHoverDelay (seconds).
const MEDIA_HOVER_DELAY: Duration = Duration::from_millis(2000);
/// The original's popup hide timer after the pointer leaves.
const MEDIA_HIDE_GRACE: Duration = Duration::from_millis(800);
/// Logical flyout width (tauri.conf.json uses the same value).
const FLYOUT_WIDTH: f64 = 380.0;
/// The original's media popup window: 300 wide, 185 collapsed and 405
/// expanded, including the card's 8 DIP shadow margin on each side.
const MEDIA_WIDTH: f64 = 300.0;
const MEDIA_COLLAPSED: f64 = 185.0;
const MEDIA_EXPANDED: f64 = 405.0;
/// Tooltip window shown above the popup; the tip is drawn bottom-centred in
/// it and the window ignores the pointer (tauri.conf.json matches).
const TIP_SIZE: (f64, f64) = (480.0, 40.0);
/// The original's SlideOut storyboard length, plus a frame.
const MEDIA_FADE_OUT: Duration = Duration::from_millis(165);

struct App {
    audio: AudioEngine,
    /// When focus loss last hid the flyout; a tray click right after it is the
    /// same gesture and must not reopen the window.
    hidden_at: Mutex<Option<Instant>>,
    /// Pinned flyouts stay open when they lose focus (Ctrl+P).
    pinned: AtomicBool,
    /// Icon data URLs per executable path.
    icons: Mutex<HashMap<String, Option<String>>>,
    /// `None` when Windows media sessions are unavailable.
    media: Option<MediaEngine>,
    tray_hover: AtomicBool,
    popup_hover: AtomicBool,
    /// Bumped on every tray enter so stale hover timers do nothing.
    hover_generation: AtomicU64,
    /// Bumped on every popup show so a pending fade-out does not hide it.
    media_generation: AtomicU64,
    /// The popup is fading out.
    media_hiding: AtomicBool,
    /// Expanded (cover art shown); kept for the session.
    media_expanded: AtomicBool,
    /// Tray icon bounds the popup was last placed against.
    media_tray: Mutex<Option<Rect>>,
    /// Registered shortcut labels for the tray menu ("action: keys").
    shortcuts: Mutex<Vec<String>>,
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// Audio commands are async so they run off the UI thread.
#[tauri::command]
async fn snapshot(app: State<'_, App>) -> Result<Snapshot, String> {
    app.audio.snapshot().map_err(error)
}
#[tauri::command]
async fn set_default_device(handle: AppHandle, device_id: String) -> Result<(), String> {
    let result = handle
        .state::<App>()
        .audio
        .set_default_device(&device_id)
        .map_err(error);
    refresh_tray(&handle);
    result
}
#[tauri::command]
async fn now_playing(app: State<'_, App>) -> Result<Option<NowPlaying>, String> {
    match &app.media {
        Some(media) => media.now_playing().map_err(error),
        None => Ok(None),
    }
}
#[tauri::command]
async fn media_command(
    app: State<'_, App>,
    command: String,
    position_ms: Option<i64>,
) -> Result<(), String> {
    let command = match command.as_str() {
        "playPause" => MediaCommand::PlayPause,
        "next" => MediaCommand::Next,
        "previous" => MediaCommand::Previous,
        "shuffle" => MediaCommand::ToggleShuffle,
        "repeat" => MediaCommand::CycleRepeat,
        "seek" => MediaCommand::Seek(position_ms.unwrap_or(0)),
        other => return Err(format!("unknown media command {other}")),
    };
    match &app.media {
        Some(media) => media.send(command).map_err(error),
        None => Err("media sessions are unavailable".into()),
    }
}
/// The media popup reports pointer enter/leave so it stays while hovered.
#[tauri::command]
fn media_hover(handle: AppHandle, hovering: bool) {
    handle
        .state::<App>()
        .popup_hover
        .store(hovering, Ordering::Relaxed);
    if !hovering {
        schedule_media_hide(handle);
    }
}

/// Routes an app to an output; `None` follows the default device again.
#[tauri::command]
async fn set_app_device(
    app: State<'_, App>,
    app_key: String,
    device_id: Option<String>,
) -> Result<(), String> {
    app.audio
        .set_app_device(&app_key, device_id.as_deref())
        .map_err(error)
}

/// PNG data URL of an executable's icon (cached; `None` when it has none).
#[tauri::command]
async fn app_icon(app: State<'_, App>, path: String) -> Result<Option<String>, String> {
    if let Some(cached) = app.icons.lock().map_err(error)?.get(&path) {
        return Ok(cached.clone());
    }
    let url = icons::data_url(&path, 32);
    app.icons.lock().map_err(error)?.insert(path, url.clone());
    Ok(url)
}
#[tauri::command]
async fn set_device_volume(
    app: State<'_, App>,
    device_id: String,
    volume: f32,
) -> Result<(), String> {
    app.audio
        .set_device_volume(&device_id, volume)
        .map_err(error)
}
#[tauri::command]
async fn set_device_mute(
    app: State<'_, App>,
    device_id: String,
    muted: bool,
) -> Result<(), String> {
    app.audio.set_device_mute(&device_id, muted).map_err(error)
}
#[tauri::command]
async fn set_app_volume(
    app: State<'_, App>,
    device_id: String,
    app_key: String,
    volume: f32,
) -> Result<(), String> {
    app.audio
        .set_app_volume(&device_id, &app_key, volume)
        .map_err(error)
}
#[tauri::command]
async fn set_app_mute(
    app: State<'_, App>,
    device_id: String,
    app_key: String,
    muted: bool,
) -> Result<(), String> {
    app.audio
        .set_app_mute(&device_id, &app_key, muted)
        .map_err(error)
}
/// Toggles pinning and returns the new state.
#[tauri::command]
fn toggle_pin(app: State<'_, App>) -> bool {
    !app.pinned.fetch_xor(true, Ordering::Relaxed)
}
#[tauri::command]
fn is_pinned(app: State<'_, App>) -> bool {
    app.pinned.load(Ordering::Relaxed)
}
#[tauri::command]
fn hide_flyout(window: WebviewWindow) {
    hide(&window);
}
/// The flyout reports its content height so the window hugs it.
#[tauri::command]
fn fit_flyout(window: WebviewWindow, height: f64) -> Result<(), String> {
    // Work in inner (client) pixels throughout: set_size sets the inner size,
    // so comparing against or reusing the outer size (frame + shadow) grows
    // the window a little on every refresh.
    let scale = window.scale_factor().map_err(error)?;
    let current = window.inner_size().map_err(error)?;
    let max = window
        .current_monitor()
        .ok()
        .flatten()
        .map_or(900.0, |m| m.work_area().size.height as f64 * 0.8);
    let width = (FLYOUT_WIDTH * scale).round() as u32;
    let height = (height * scale).clamp(120.0 * scale, max).round() as u32;
    if current.width.abs_diff(width) > 1 || current.height.abs_diff(height) > 1 {
        window
            .set_size(tauri::PhysicalSize::new(width, height))
            .map_err(error)?;
        place_near_taskbar(&window, None);
    }
    Ok(())
}

fn hide(window: &WebviewWindow) {
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        let _ = window.emit("flyout-hidden", ());
        set_active(window, false);
    }
}

/// Resumes (`true`) or suspends (`false`) a window's page; hidden pages run
/// no scripts or timers and trim their memory.
fn set_active(window: &WebviewWindow, active: bool) {
    let _ = window.with_webview(move |webview| webview::set_active(webview, active));
}

/// Bottom-right of the work area, horizontally centred on the tray icon when
/// its bounds are known (taskbar at the bottom; other edges come later).
fn place_near_taskbar(window: &WebviewWindow, tray: Option<Rect>) {
    let Ok(Some(monitor)) = window
        .current_monitor()
        .or_else(|_| window.primary_monitor())
    else {
        return;
    };
    let work: PhysicalRect<i32, u32> = *monitor.work_area();
    let Ok(size) = window.outer_size() else {
        return;
    };
    let margin = (12.0 * monitor.scale_factor()) as i32;
    let right = work.position.x + work.size.width as i32;
    let bottom = work.position.y + work.size.height as i32;
    let mut x = right - size.width as i32 - margin;
    if let Some(rect) = tray {
        let scale = monitor.scale_factor();
        let position = rect.position.to_physical::<i32>(scale);
        let width = rect.size.to_physical::<i32>(scale).width;
        x = (position.x + width / 2 - size.width as i32 / 2)
            .clamp(work.position.x + margin, right - size.width as i32 - margin);
    }
    let y = bottom - size.height as i32 - margin;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Plays the popup's fade-out, then hides it, unless it is shown again
/// before the fade ends.
fn hide_media(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MEDIA) else {
        return;
    };
    let state = app.state::<App>();
    if !window.is_visible().unwrap_or(false) || state.media_hiding.swap(true, Ordering::Relaxed) {
        return;
    }
    let generation = state.media_generation.load(Ordering::Relaxed);
    let _ = window.emit("media-hiding", ());
    present_tooltip(app);
    std::thread::spawn(move || {
        std::thread::sleep(MEDIA_FADE_OUT);
        let state = window.state::<App>();
        if state.media_generation.load(Ordering::Relaxed) == generation {
            let _ = window.hide();
            let _ = window.emit("media-hidden", ());
            set_active(&window, false);
            state.media_hiding.store(false, Ordering::Relaxed);
        }
    });
}

/// Shows the popup with its slide-in, centred 5 DIP above the tray icon like
/// the original. Does nothing if it is already showing.
fn show_media(app: &AppHandle, tray: Rect) {
    let Some(window) = app.get_webview_window(MEDIA) else {
        return;
    };
    let state = app.state::<App>();
    let hiding = state.media_hiding.swap(false, Ordering::Relaxed);
    if window.is_visible().unwrap_or(false) && !hiding {
        return;
    }
    state.media_generation.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut last) = state.media_tray.lock() {
        *last = Some(tray);
    }
    place_media(app, &window);
    set_active(&window, true);
    let _ = window.show();
    // Showing can move the window to the taskbar's monitor and change its
    // DPI, so place it again once it is there.
    place_media(app, &window);
    let expanded = state.media_expanded.load(Ordering::Relaxed);
    let _ = window.emit("media-shown", expanded);
    present_tooltip(app);
}

/// Whether the popup is up (shown and not fading out).
fn media_showing(app: &AppHandle) -> bool {
    app.get_webview_window(MEDIA)
        .is_some_and(|w| w.is_visible().unwrap_or(false))
        && !app.state::<App>().media_hiding.load(Ordering::Relaxed)
}

/// Where the tray icon is, in the physical pixels of its monitor.
struct TrayPlace {
    scale: f64,
    /// Horizontal centre of the icon.
    center_x: i32,
    /// Top of the icon once an auto-hidden taskbar has slid in.
    top: i32,
    work: PhysicalRect<i32, u32>,
}

fn tray_place(app: &AppHandle, tray: Rect) -> Option<TrayPlace> {
    let monitors = app.available_monitors().ok()?;
    // Tray rects are physical. The monitor nearest the icon's centre holds
    // it: an auto-hidden taskbar reports the icon partly below the screen.
    let probe = tray.position.to_physical::<i32>(1.0);
    let probe_size = tray.size.to_physical::<i32>(1.0);
    let (cx, cy) = (
        probe.x + probe_size.width / 2,
        probe.y + probe_size.height / 2,
    );
    let monitor = monitors.iter().min_by_key(|m| {
        let (p, s) = (m.position(), m.size());
        let dx = i64::from((p.x - cx).max(cx - (p.x + s.width as i32 - 1)).max(0));
        let dy = i64::from((p.y - cy).max(cy - (p.y + s.height as i32 - 1)).max(0));
        dx * dx + dy * dy
    })?;
    let scale = monitor.scale_factor();
    let icon = tray.position.to_physical::<i32>(scale);
    let icon_size = tray.size.to_physical::<i32>(scale);
    let screen_bottom = monitor.position().y + monitor.size().height as i32;
    Some(TrayPlace {
        scale,
        center_x: icon.x + icon_size.width / 2,
        top: icon.y.min(screen_bottom - icon_size.height),
        work: *monitor.work_area(),
    })
}

/// Popup window height in DIPs for the current expanded state.
fn media_height(app: &AppHandle) -> f64 {
    if app.state::<App>().media_expanded.load(Ordering::Relaxed) {
        MEDIA_EXPANDED
    } else {
        MEDIA_COLLAPSED
    }
}

/// Window = card + 8 DIP shadow margin; its bottom sits 5 DIP above the icon,
/// horizontally centred on it and kept 10 DIP inside the work area, as the
/// original. Size and position change together so expanding (which grows
/// the window upwards) does not flicker.
fn place_media(app: &AppHandle, window: &WebviewWindow) {
    let Some(tray) = app.state::<App>().media_tray.lock().ok().and_then(|t| *t) else {
        return;
    };
    let Some(place) = tray_place(app, tray) else {
        return;
    };
    let scale = place.scale;
    let width = (MEDIA_WIDTH * scale).round() as i32;
    let height = (media_height(app) * scale).round() as i32;
    let inset = (10.0 * scale) as i32;
    let work = place.work;
    let left = work.position.x + inset;
    let right = work.position.x + work.size.width as i32 - width - inset;
    let x = (place.center_x - width / 2).clamp(left, right.max(left));
    let y = (place.top - height - (5.0 * scale) as i32).max(work.position.y + inset);
    set_bounds(window, x, y, width, height);
}

/// Moves and resizes a window in one step (physical pixels).
fn set_bounds(window: &WebviewWindow, x: i32, y: i32, width: i32, height: i32) {
    match window.hwnd() {
        Ok(hwnd) => bt_app::win32::window::set_bounds(hwnd.0, x, y, width, height),
        Err(_) => {
            let _ = window.set_size(tauri::PhysicalSize::new(width as u32, height as u32));
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
}

/// The original's tray tooltip: "BetterTrumpet: 94% - Headset Earphone (USB)"
/// from the device description and enumerator, under 64 characters.
fn tooltip(devices: &Snapshot) -> String {
    let Some(d) = devices.devices.iter().find(|d| d.is_default) else {
        return "BetterTrumpet: No playback devices".into();
    };
    let state = if d.muted {
        "Muted".to_string()
    } else {
        format!("{}%", (d.volume * 100.0).round() as i32)
    };
    let prefix = format!("BetterTrumpet: {state} - ");
    let mut name = if d.enumerator.trim().is_empty() {
        d.description.clone()
    } else {
        format!("{} ({})", d.description, d.enumerator)
    };
    if d.description.trim().is_empty() && d.enumerator.trim().is_empty() {
        name = d.name.clone();
    }
    let room = 63usize.saturating_sub(prefix.chars().count());
    prefix + &name.chars().take(room).collect::<String>()
}

/// Like the original's RefreshTrayTooltipPresentation: while the popup is up
/// the native tooltip is blanked and the text shows above the popup instead
/// (only while the pointer is on the icon); otherwise the native one is used.
fn present_tooltip(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let text = app
        .state::<App>()
        .audio
        .devices()
        .map(|d| tooltip(&d))
        .unwrap_or_default();
    let tip = app.get_webview_window(TIP);
    if media_showing(app) {
        let _ = tray.set_tooltip(Some(""));
        let hovering = app.state::<App>().tray_hover.load(Ordering::Relaxed);
        match tip {
            Some(tip) if hovering => show_tip(app, &tip, &text),
            Some(tip) => hide_tip(&tip),
            None => {}
        }
    } else {
        if let Some(tip) = tip {
            hide_tip(&tip);
        }
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Above the popup: bottom 12 DIP over the popup window's top, centred on
/// the tray icon and kept 4 DIP inside the work area.
fn show_tip(app: &AppHandle, tip: &WebviewWindow, text: &str) {
    let Some(tray) = app.state::<App>().media_tray.lock().ok().and_then(|t| *t) else {
        return;
    };
    let Some(place) = tray_place(app, tray) else {
        return;
    };
    let scale = place.scale;
    let width = (TIP_SIZE.0 * scale).round() as i32;
    let height = (TIP_SIZE.1 * scale).round() as i32;
    let inset = (4.0 * scale) as i32;
    let work = place.work;
    let left = work.position.x + inset;
    let right = work.position.x + work.size.width as i32 - width - inset;
    let x = (place.center_x - width / 2).clamp(left, right.max(left));
    // The tip sits 6 DIP above the window's bottom edge (room for its shadow).
    let bottom = place.top - ((media_height(app) + 12.0 - 6.0) * scale).round() as i32;
    let y = (bottom - height).max(work.position.y + inset);
    let _ = tip.emit("tip-text", text);
    if !tip.is_visible().unwrap_or(false) {
        set_bounds(tip, x, y, width, height);
        set_active(tip, true);
        let _ = tip.show();
        let _ = tip.emit("tip-shown", ());
    } else {
        set_bounds(tip, x, y, width, height);
    }
}

fn hide_tip(tip: &WebviewWindow) {
    if tip.is_visible().unwrap_or(false) {
        let _ = tip.hide();
        set_active(tip, false);
    }
}

/// The popup's expand arrow: grows the window upwards for the cover art
/// (bottom stays put), or shrinks it back once the collapse has played.
#[tauri::command]
fn media_expand(handle: AppHandle, expanded: bool) {
    handle
        .state::<App>()
        .media_expanded
        .store(expanded, Ordering::Relaxed);
    if let Some(window) = handle.get_webview_window(MEDIA) {
        place_media(&handle, &window);
    }
    if handle.state::<App>().tray_hover.load(Ordering::Relaxed) {
        present_tooltip(&handle);
    }
}

/// The audio session of the app that owns the current media session, for
/// the popup's volume slider (the original's FindCurrentMediaApp).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaApp {
    device_id: String,
    app_key: String,
    volume: f32,
    muted: bool,
}

#[tauri::command]
async fn media_app(app: State<'_, App>, source_app: String) -> Result<Option<MediaApp>, String> {
    if source_app.trim().is_empty() {
        return Ok(None);
    }
    let snapshot = app.audio.snapshot().map_err(error)?;
    let source = source_app.to_lowercase();
    let stem = |s: &str| {
        let name = s.rsplit(['\\', '/']).next().unwrap_or(s);
        name.strip_suffix(".exe").unwrap_or(name).to_string()
    };
    let source_stem = stem(&source);
    for device in &snapshot.devices {
        for session in device.apps.iter().filter(|a| !a.is_system_sounds) {
            let exe = stem(
                &session
                    .executable
                    .clone()
                    .unwrap_or_default()
                    .to_lowercase(),
            );
            if exe.len() >= 3 && (exe == source_stem || source.contains(&exe)) {
                return Ok(Some(MediaApp {
                    device_id: device.id.clone(),
                    app_key: session.key.clone(),
                    volume: session.volume,
                    muted: session.muted,
                }));
            }
        }
    }
    Ok(None)
}

/// Hides the popup unless the pointer returns to the tray or popup in time.
fn schedule_media_hide(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(MEDIA_HIDE_GRACE);
        let state = app.state::<App>();
        if !state.tray_hover.load(Ordering::Relaxed) && !state.popup_hover.load(Ordering::Relaxed) {
            hide_media(&app);
        }
    });
}

/// After the hover delay, shows the media popup if the pointer is still on
/// the tray icon, the flyout is closed and something is playing or paused.
fn schedule_media_show(app: AppHandle, tray: Rect) {
    let generation = app
        .state::<App>()
        .hover_generation
        .fetch_add(1, Ordering::Relaxed)
        + 1;
    std::thread::spawn(move || {
        std::thread::sleep(MEDIA_HOVER_DELAY);
        let state = app.state::<App>();
        if state.hover_generation.load(Ordering::Relaxed) != generation
            || !state.tray_hover.load(Ordering::Relaxed)
        {
            return;
        }
        let flyout_open = app
            .get_webview_window(FLYOUT)
            .is_some_and(|w| w.is_visible().unwrap_or(false));
        let playing = state
            .media
            .as_ref()
            .and_then(|m| m.now_playing().ok().flatten())
            .is_some();
        if flyout_open || !playing {
            return;
        }
        show_media(&app, tray);
    });
}

fn show(app: &AppHandle, tray: Option<Rect>) {
    hide_media(app);
    let Some(window) = app.get_webview_window(FLYOUT) else {
        return;
    };
    place_near_taskbar(&window, tray);
    set_active(&window, true);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("flyout-shown", ());
}

fn toggle(app: &AppHandle, tray: Rect) {
    let Some(window) = app.get_webview_window(FLYOUT) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        hide(&window);
        return;
    }
    let state = app.state::<App>();
    let recently_hidden = state
        .hidden_at
        .lock()
        .ok()
        .and_then(|t| *t)
        .is_some_and(|t| t.elapsed() < Duration::from_millis(250));
    if recently_hidden {
        return;
    }
    show(app, Some(tray));
}

/// Tray menu: output devices (checked = default), then app actions.
fn build_menu(app: &AppHandle, devices: &Snapshot) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    if devices.devices.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "none",
            "No output devices",
            false,
            None::<&str>,
        )?)?;
    }
    for device in &devices.devices {
        let id = format!("device:{}", device.id);
        menu.append(&CheckMenuItem::with_id(
            app,
            id,
            &device.name,
            true,
            device.is_default,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&CheckMenuItem::with_id(
        app,
        "startup",
        "Start with Windows",
        true,
        startup::is_enabled(),
        None::<&str>,
    )?)?;
    let shortcuts = app
        .state::<App>()
        .shortcuts
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    if !shortcuts.is_empty() {
        let submenu = Submenu::with_id(app, "shortcuts", "Keyboard shortcuts", true)?;
        for (i, label) in shortcuts.iter().enumerate() {
            submenu.append(&MenuItem::with_id(
                app,
                format!("shortcut:{i}"),
                label,
                false,
                None::<&str>,
            )?)?;
        }
        menu.append(&submenu)?;
    }
    menu.append(&MenuItem::with_id(
        app,
        "mixer",
        "Volume mixer",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "sound",
        "Sound settings",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Exit", true, None::<&str>)?)?;
    Ok(menu)
}

/// Rebuilds the tray menu and tooltip from the current devices.
fn refresh_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let Ok(devices) = app.state::<App>().audio.devices() else {
        return;
    };
    if let Ok(menu) = build_menu(app, &devices) {
        let _ = tray.set_menu(Some(menu));
    }
    present_tooltip(app);
}

/// Keeps the tray menu and tooltip current from Windows' change
/// notifications; bursts of events are coalesced into one refresh.
fn watch_devices(app: AppHandle) {
    let (signal, changes) = std::sync::mpsc::channel::<bt_audio::Change>();
    let audio = app.state::<App>().audio.clone();
    if audio
        .watch(move |change| {
            let _ = signal.send(change);
        })
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        let mut last_menu = String::new();
        while let Ok(first) = changes.recv() {
            let mut devices_changed = first == bt_audio::Change::Devices;
            std::thread::sleep(Duration::from_millis(120));
            while let Ok(more) = changes.try_recv() {
                devices_changed |= more == bt_audio::Change::Devices;
            }
            let Ok(devices) = audio.devices() else {
                continue;
            };
            let Some(tray) = app.tray_by_id("main") else {
                continue;
            };
            let key: String = devices
                .devices
                .iter()
                .map(|d| format!("{}|{}|{}\n", d.id, d.name, d.is_default))
                .collect();
            if devices_changed && key != last_menu {
                if let Ok(menu) = build_menu(&app, &devices) {
                    let _ = tray.set_menu(Some(menu));
                }
                last_menu = key;
            }
            present_tooltip(&app);
            if devices_changed {
                let _ = app.emit("devices-changed", ());
            }
        }
    });
}

/// Adjusts every unmuted output by `delta` percent, like the original's
/// absolute volume hotkeys.
fn step_all_devices(app: &AppHandle, delta: f32) {
    let audio = &app.state::<App>().audio;
    if let Ok(devices) = audio.devices() {
        for device in devices.devices.iter().filter(|d| !d.muted) {
            let _ = audio.set_device_volume(&device.id, device.volume + delta / 100.0);
        }
    }
}

/// Mouse wheel over the tray icon: 2% per notch on the default output.
fn start_tray_wheel(app: AppHandle) {
    let (sender, deltas) = std::sync::mpsc::channel::<i16>();
    let hover_app = app.clone();
    wheel::init(move |delta| {
        if hover_app.state::<App>().tray_hover.load(Ordering::Relaxed) {
            let _ = sender.send(delta);
        }
    });
    std::thread::spawn(move || {
        while let Ok(delta) = deltas.recv() {
            let audio = &app.state::<App>().audio;
            if let Ok(devices) = audio.devices() {
                if let Some(device) = devices.devices.iter().find(|d| d.is_default) {
                    let step = f32::from(delta.signum()) * 0.02;
                    let _ = audio.set_device_volume(&device.id, device.volume + step);
                }
            }
        }
    });
}

fn register_hotkeys(app: &tauri::App) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let mut labels = vec![];
    for binding in hotkeys::bindings() {
        let action = binding.action;
        let registered = app
            .global_shortcut()
            .on_shortcut(binding.shortcut, move |app, _, event| {
                if event.state != ShortcutState::Pressed {
                    return;
                }
                match action {
                    hotkeys::Action::ToggleFlyout => {
                        let visible = app
                            .get_webview_window(FLYOUT)
                            .is_some_and(|w| w.is_visible().unwrap_or(false));
                        if visible {
                            if let Some(w) = app.get_webview_window(FLYOUT) {
                                hide(&w);
                            }
                        } else {
                            show(app, None);
                        }
                    }
                    hotkeys::Action::VolumeUp => step_all_devices(app, 2.0),
                    hotkeys::Action::VolumeDown => step_all_devices(app, -2.0),
                }
            })
            .is_ok();
        let source = if binding.from_settings {
            " (from BetterTrumpet)"
        } else {
            ""
        };
        let status = if registered {
            ""
        } else {
            " — in use by another app"
        };
        labels.push(format!(
            "{}: {}{source}{status}",
            hotkeys::describe(action),
            binding.label
        ));
    }
    if let Ok(mut shortcuts) = app.state::<App>().shortcuts.lock() {
        *shortcuts = labels;
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "sound" => open("ms-settings:sound"),
        "mixer" => open("ms-settings:apps-volume"),
        "startup" => {
            if let Ok(exe) = std::env::current_exe() {
                let _ = startup::set_enabled(!startup::is_enabled(), &exe);
            }
            refresh_tray(app);
        }
        _ => {
            if let Some(device) = id.strip_prefix("device:") {
                let _ = app.state::<App>().audio.set_default_device(device);
                refresh_tray(app);
                let _ = app.emit("devices-changed", ());
            }
        }
    }
}

fn main() {
    let audio = match AudioEngine::start() {
        Ok(audio) => audio,
        Err(e) => {
            eprintln!("BetterTrumpet: audio engine failed to start: {e}");
            std::process::exit(1);
        }
    };
    tauri::Builder::default()
        // A second launch opens the running instance's flyout instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show(app, None);
        }))
        .manage(App {
            audio,
            hidden_at: Mutex::new(None),
            pinned: AtomicBool::new(std::env::args().any(|a| a == "--pinned")),
            icons: Mutex::new(HashMap::new()),
            media: MediaEngine::start().ok(),
            tray_hover: AtomicBool::new(false),
            popup_hover: AtomicBool::new(false),
            hover_generation: AtomicU64::new(0),
            media_generation: AtomicU64::new(0),
            media_hiding: AtomicBool::new(false),
            media_expanded: AtomicBool::new(false),
            media_tray: Mutex::new(None),
            shortcuts: Mutex::new(vec![]),
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            set_default_device,
            app_icon,
            now_playing,
            media_command,
            media_hover,
            set_app_device,
            media_expand,
            media_app,
            set_device_volume,
            set_device_mute,
            set_app_volume,
            set_app_mute,
            hide_flyout,
            toggle_pin,
            is_pinned,
            fit_flyout
        ])
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            register_hotkeys(app);
            let handle = app.handle().clone();
            // The tooltip never takes the pointer, like the original's.
            if let Some(tip) = app.get_webview_window(TIP) {
                let _ = tip.set_ignore_cursor_events(true);
            }
            let devices = app.state::<App>().audio.devices().unwrap_or_default();
            let menu = build_menu(&handle, &devices)?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("bundled icon"))
                .tooltip(tooltip(&devices))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| on_menu(app, event.id.as_ref()))
                .on_tray_icon_event(|tray, event| {
                    let app = tray.app_handle();
                    match event {
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            rect,
                            ..
                        } => {
                            hide_media(app);
                            toggle(app, rect);
                        }
                        TrayIconEvent::Enter { rect, .. } => {
                            app.state::<App>().tray_hover.store(true, Ordering::Relaxed);
                            wheel::install();
                            if media_showing(app) {
                                present_tooltip(app);
                            } else {
                                schedule_media_show(app.clone(), rect);
                            }
                        }
                        TrayIconEvent::Leave { .. } => {
                            app.state::<App>()
                                .tray_hover
                                .store(false, Ordering::Relaxed);
                            wheel::uninstall();
                            schedule_media_hide(app.clone());
                            present_tooltip(app);
                        }
                        _ => {}
                    }
                })
                .build(app)?;
            watch_devices(handle.clone());
            start_tray_wheel(handle);
            // `--show` opens the flyout at startup (testing, shortcuts).
            if std::env::args().any(|a| a == "--show") {
                show(app.handle(), None);
            }
            // `--media` opens the media popup once its page has loaded
            // (testing its placement and animation without hovering).
            if std::env::args().any(|a| a == "--media") {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(1500));
                    if let Some(Ok(Some(rect))) = handle.tray_by_id("main").map(|t| t.rect()) {
                        show_media(&handle, rect);
                    }
                });
            }
            Ok(())
        })
        // Both windows start hidden: suspend each page once it has loaded.
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && !webview.window().is_visible().unwrap_or(false)
            {
                let _ = webview.with_webview(|platform| webview::set_active(platform, false));
            }
        })
        .on_window_event(|window, event| {
            if window.label() != FLYOUT {
                return;
            }
            if let WindowEvent::Focused(false) = event {
                if window.state::<App>().pinned.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
                    if webview.is_visible().unwrap_or(false) {
                        if let Ok(mut t) = window.state::<App>().hidden_at.lock() {
                            *t = Some(Instant::now());
                        }
                        hide(&webview);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running BetterTrumpet");
}

fn open(target: &str) {
    let _ = std::process::Command::new("explorer").arg(target).spawn();
}
