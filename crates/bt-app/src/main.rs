//! BetterTrumpet Rust port — preview tray app.
//!
//! Daily-use features first: a tray flyout with every output device and its
//! apps (volume, mute, peaks, names and icons), default-device switching from
//! the flyout and the tray menu, a live tooltip, single instance and
//! "Start with Windows". Everything else in PARITY.md is still to come.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bt_app::win32::{icons, startup};
use bt_audio::{AudioEngine, Snapshot};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalRect, Rect, State, WebviewWindow,
    WindowEvent,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const FLYOUT: &str = "flyout";
/// Logical flyout width (tauri.conf.json uses the same value).
const FLYOUT_WIDTH: f64 = 380.0;

struct App {
    audio: AudioEngine,
    /// When focus loss last hid the flyout; a tray click right after it is the
    /// same gesture and must not reopen the window.
    hidden_at: Mutex<Option<Instant>>,
    /// Pinned flyouts stay open when they lose focus (Ctrl+P).
    pinned: AtomicBool,
    /// Icon data URLs per executable path.
    icons: Mutex<HashMap<String, Option<String>>>,
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
    }
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

fn show(app: &AppHandle, tray: Option<Rect>) {
    let Some(window) = app.get_webview_window(FLYOUT) else {
        return;
    };
    place_near_taskbar(&window, tray);
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

fn tooltip(devices: &Snapshot) -> String {
    let text = match devices.devices.iter().find(|d| d.is_default) {
        Some(d) if d.muted => format!("BetterTrumpet\n{}: muted", d.name),
        Some(d) => format!("BetterTrumpet\n{}: {:.0}%", d.name, d.volume * 100.0),
        None => "BetterTrumpet\nNo output device".into(),
    };
    // Windows truncates tray tooltips at 127 characters.
    text.chars().take(127).collect()
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
    let _ = tray.set_tooltip(Some(tooltip(&devices)));
}

/// Keeps the tray menu and tooltip current. Devices only (no sessions), and
/// the menu is rebuilt only when the device list or default changes.
fn watch_devices(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_menu = String::new();
        let mut last_tip = String::new();
        loop {
            if let Ok(devices) = app.state::<App>().audio.devices() {
                let key: String = devices
                    .devices
                    .iter()
                    .map(|d| format!("{}|{}|{}\n", d.id, d.name, d.is_default))
                    .collect::<String>()
                    + &startup::is_enabled().to_string();
                if let Some(tray) = app.tray_by_id("main") {
                    if key != last_menu {
                        if let Ok(menu) = build_menu(&app, &devices) {
                            let _ = tray.set_menu(Some(menu));
                        }
                        last_menu = key;
                    }
                    let tip = tooltip(&devices);
                    if tip != last_tip {
                        let _ = tray.set_tooltip(Some(&tip));
                        last_tip = tip;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1000));
        }
    });
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
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            set_default_device,
            app_icon,
            set_device_volume,
            set_device_mute,
            set_app_volume,
            set_app_mute,
            hide_flyout,
            toggle_pin,
            is_pinned,
            fit_flyout
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let devices = app.state::<App>().audio.devices().unwrap_or_default();
            let menu = build_menu(&handle, &devices)?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("bundled icon"))
                .tooltip(tooltip(&devices))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| on_menu(app, event.id.as_ref()))
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle(tray.app_handle(), rect);
                    }
                })
                .build(app)?;
            watch_devices(handle);
            // `--show` opens the flyout at startup (testing, shortcuts).
            if std::env::args().any(|a| a == "--show") {
                show(app.handle(), None);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
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
