//! BetterTrumpet Rust port — preview tray app.
//!
//! A native Win32 app: a tray icon, the flyout with every output device and
//! its apps, the media popup and the tray tooltip, drawn with Direct2D into
//! layered windows like the original's WPF windows. No web view.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod focus;
mod folders;
mod hotkeys;
mod ipc;
mod profiles;
mod rules;
mod settings_file;
mod store;
mod ui;

use bt_app::win32::{instance, wnd};
use bt_audio::AudioEngine;
use bt_core::undo::UndoHistory;
use bt_media::MediaEngine;

fn main() {
    // `--render <dir>` draws the windows to PNGs and exits; it runs beside
    // a running copy without touching it.
    let args: Vec<String> = std::env::args().collect();
    let render = args
        .iter()
        .position(|a| a == "--render")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    // A second launch opens the running instance's flyout instead.
    let instance = if render.is_some() {
        None
    } else {
        match instance::acquire() {
            Some(instance) => Some(instance),
            None => {
                if let Some(running) = instance::find(ui::HIDDEN_TITLE) {
                    running.post(ui::APP_SHOW);
                }
                return;
            }
        }
    };
    let _instance = instance;
    // The UI language; BT_CULTURE overrides it for checking translations.
    let culture = (render.is_some() || cfg!(debug_assertions))
        .then(|| std::env::var("BT_CULTURE").ok())
        .flatten();
    bt_app::i18n::init(culture.as_deref());
    wnd::init_thread();
    let audio = match AudioEngine::start() {
        Ok(audio) => audio,
        Err(e) => {
            eprintln!("BetterTrumpet: audio engine failed to start: {e}");
            std::process::exit(1);
        }
    };
    let core = core::Core {
        audio,
        media: MediaEngine::start().ok(),
        store: store::Store::open(),
        undo: UndoHistory::default(),
        launch: rules::LaunchTracker::default(),
    };
    if let Some(dir) = render {
        let _ = ui::render(core, &dir);
        return;
    }
    if let Err(e) = ui::run(core) {
        eprintln!("BetterTrumpet: the UI failed to start: {e}");
        std::process::exit(1);
    }
}
