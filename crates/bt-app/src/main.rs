//! BetterTrumpet Rust port — preview tray app.
//!
//! Scope of this preview: a tray icon whose left click toggles a flyout with
//! every active output device and its apps (volume, mute, peak meters), and a
//! right-click menu. Everything else in PARITY.md is still to come.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use bt_audio::{AudioEngine, Snapshot};
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalRect, Rect, State, WebviewWindow,
    WindowEvent,
    menu::{Menu, MenuItem, PredefinedMenuItem},
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
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn snapshot(app: State<'_, App>) -> Result<Snapshot, String> {
    app.audio.snapshot().map_err(error)
}
#[tauri::command]
fn set_device_volume(app: State<'_, App>, device_id: String, volume: f32) -> Result<(), String> {
    app.audio
        .set_device_volume(&device_id, volume)
        .map_err(error)
}
#[tauri::command]
fn set_device_mute(app: State<'_, App>, device_id: String, muted: bool) -> Result<(), String> {
    app.audio.set_device_mute(&device_id, muted).map_err(error)
}
#[tauri::command]
fn set_app_volume(
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
fn set_app_mute(
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
    place_near_taskbar(&window, Some(tray));
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("flyout-shown", ());
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
        .manage(App {
            audio,
            hidden_at: Mutex::new(None),
            pinned: AtomicBool::new(std::env::args().any(|a| a == "--pinned")),
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
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
            let sound = MenuItem::with_id(app, "sound", "Sound settings", true, None::<&str>)?;
            let mixer = MenuItem::with_id(app, "mixer", "Volume mixer", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Exit", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&mixer, &sound, &separator, &quit])?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("bundled icon"))
                .tooltip("BetterTrumpet")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "sound" => open("ms-settings:sound"),
                    "mixer" => open("ms-settings:apps-volume"),
                    _ => {}
                })
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
            // `--show` opens the flyout at startup (testing, shortcuts).
            if std::env::args().any(|a| a == "--show") {
                if let Some(window) = app.get_webview_window(FLYOUT) {
                    place_near_taskbar(&window, None);
                    let _ = window.show();
                    let _ = window.set_focus();
                    let _ = window.emit("flyout-shown", ());
                }
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
