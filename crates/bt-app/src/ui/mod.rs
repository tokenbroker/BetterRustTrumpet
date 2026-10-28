//! The native UI: the tray icon and its menu, the flyout, the media popup
//! and the tooltip, all on the UI thread. Background threads (audio change
//! notifications, the tray wheel hook, a second launch) only post codes to
//! the hidden window.
pub mod anim;
mod flyout;
mod media;
mod slider;
mod theme;
mod tip;

use crate::{core::Core, hotkeys};
use bt_app::win32::{
    gfx::Gfx,
    menu::{self, Item},
    startup,
    tray::Tray,
    wheel,
    wnd::{self, Event, TrayEvent, Window},
};
use bt_audio::Snapshot;
use bt_core::settings::{Key, Value};
use std::{cell::RefCell, time::Duration};

/// The hidden window's title, which a second launch looks for.
pub const HIDDEN_TITLE: &str = "BetterTrumpet Rust";

// `Event::App` codes posted to the hidden window.
pub const APP_SHOW: usize = 1;
const APP_DEVICES: usize = 2;
const APP_SESSIONS: usize = 3;
const APP_WHEEL_UP: usize = 4;
const APP_WHEEL_DOWN: usize = 5;
const APP_SHOW_MEDIA: usize = 6;

// Timers on the hidden window.
const TRAY_POLL: usize = 1;
const MEDIA_SHOW: usize = 2;
const MEDIA_HIDE: usize = 3;
const AUDIO: usize = 4;

/// The original's popup hide delay after the pointer leaves.
const MEDIA_HIDE_GRACE: u32 = 800;

#[derive(Clone, Debug)]
pub enum MenuCommand {
    Flyout(flyout::Command),
    Device(String),
    Startup,
    Mixer,
    Sound,
    Quit,
}

/// A popup menu to show once the current event has been handled (menus
/// run a modal loop of their own).
pub struct MenuRequest {
    pub owner: Window,
    pub items: Vec<Item>,
    /// Command for item id `i + 1`.
    pub commands: Vec<MenuCommand>,
    pub x: i32,
    pub y: i32,
    pub above: bool,
}

pub struct Ui {
    core: Core,
    hidden: Window,
    tray: Tray,
    flyout: flyout::Flyout,
    media: media::Media,
    tip: tip::Tip,
    theme: theme::Theme,
    icon_font: &'static str,
    tray_hover: bool,
    tray_rect: Option<(i32, i32, i32, i32)>,
    /// Registered shortcut labels for the tray menu ("action: keys").
    shortcuts: Vec<String>,
    /// The action for hotkey id `i + 1`.
    hotkeys: Vec<hotkeys::Action>,
    devices_changed: bool,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

/// Runs `f` on the UI state (if the UI is running and not already in use).
fn with<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|ui| ui.try_borrow_mut().ok()?.as_mut().map(f))
}

/// The original's tray tooltip: "BetterTrumpet: 94% - Headset Earphone
/// (USB)" from the device description and enumerator, under 64 characters.
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

/// Creates the windows and the tray icon and runs the message loop.
pub fn run(core: Core) -> windows::core::Result<()> {
    let gfx = Gfx::new()?;
    let icon_font = if gfx.has_font("Segoe Fluent Icons") {
        "Segoe Fluent Icons"
    } else {
        "Segoe MDL2 Assets"
    };
    let hidden = wnd::create_hidden(HIDDEN_TITLE)?;
    let media_expanded = core.store.bool(Key::MediaPopupRememberExpanded)
        && core.store.bool(Key::MediaPopupIsExpanded);
    let devices = core.audio.devices().unwrap_or_default();
    let mut ui = Ui {
        tray: Tray::new(hidden, &tooltip(&devices)),
        flyout: flyout::Flyout::new(gfx.clone(), &core.store)?,
        media: media::Media::new(gfx.clone(), media_expanded)?,
        tip: tip::Tip::new(gfx.clone())?,
        theme: theme::Theme::load(&core.store, icon_font),
        icon_font,
        core,
        hidden,
        tray_hover: false,
        tray_rect: None,
        shortcuts: vec![],
        hotkeys: vec![],
        devices_changed: false,
    };
    ui.register_hotkeys();
    ui.watch();
    // Launch rules for apps already playing, plus Lock and hard mute.
    ui.core.enforce_rules();
    UI.with(|cell| *cell.borrow_mut() = Some(ui));
    wnd::set_handler(|window, event| {
        let menu = with(|ui| ui.event(window, event)).flatten();
        if let Some(request) = menu {
            open_menu(request);
        }
    });
    // `--show` opens the flyout at startup; `--media` the media popup.
    if std::env::args().any(|a| a == "--show") {
        hidden.post(APP_SHOW);
    }
    if std::env::args().any(|a| a == "--media") {
        hidden.post(APP_SHOW_MEDIA);
    }
    wnd::run();
    // Removes the tray icon and releases everything before exiting.
    UI.with(|cell| cell.borrow_mut().take());
    Ok(())
}

/// `--render <dir>`: draws each window to a PNG without showing anything
/// (for checking the drawing without taking over the screen).
pub fn render(mut core: Core, dir: &std::path::Path) -> windows::core::Result<()> {
    let gfx = Gfx::new()?;
    let icon_font = if gfx.has_font("Segoe Fluent Icons") {
        "Segoe Fluent Icons"
    } else {
        "Segoe MDL2 Assets"
    };
    let theme = theme::Theme::load(&core.store, icon_font);
    let _ = std::fs::create_dir_all(dir);
    let save = |name: &str, image: Option<(u32, u32, Vec<u8>)>| {
        if let Some((w, h, rgba)) = image {
            let _ = std::fs::write(dir.join(name), bt_app::win32::icons::png_rect(&rgba, w, h));
        }
    };
    let mut flyout = flyout::Flyout::new(gfx.clone(), &core.store)?;
    save("flyout.png", flyout.render(&mut core, &theme, false));
    save("flyout-popup.png", flyout.render(&mut core, &theme, true));
    // A tray icon position near the bottom-right of the primary monitor.
    let monitor = wnd::monitor_at(0, 0);
    let (x, y, w, h) = monitor.bounds;
    let tray = (x + w - 200, y + h - 40, 24, 40);
    let mut media = media::Media::new(gfx.clone(), false)?;
    save("media.png", media.render(&core, tray, false));
    save("media-expanded.png", media.render(&core, tray, true));
    let mut tip = tip::Tip::new(gfx)?;
    let devices = core.audio.devices().unwrap_or_default();
    save("tip.png", tip.render(&tooltip(&devices)));
    Ok(())
}

fn open_menu(request: MenuRequest) {
    let MenuRequest {
        owner,
        items,
        commands,
        x,
        y,
        above,
    } = request;
    wnd::post_task(owner, move || {
        // Menus follow the same system theme as the flyout.
        let dark = !bt_app::win32::theme::system_light();
        let chosen = menu::popup(owner, &items, x, y, above, dark);
        let command = chosen.and_then(|id| commands.get(id as usize - 1).cloned());
        with(|ui| {
            if let Some(command) = command {
                ui.menu_command(command);
            }
            // Keyboard focus returns to the flyout after its menus.
            if ui.flyout.showing() && owner == ui.flyout.window {
                ui.flyout.window.focus();
            }
        });
    });
}

impl Ui {
    fn register_hotkeys(&mut self) {
        let bindings = hotkeys::bindings(&self.core.store);
        for (i, binding) in bindings.iter().enumerate() {
            let registered =
                wnd::register_hotkey(self.hidden, i as i32 + 1, binding.modifiers, binding.vk);
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
            self.shortcuts.push(format!(
                "{}: {}{source}{status}",
                hotkeys::describe(binding.action),
                binding.label
            ));
            self.hotkeys.push(binding.action);
        }
    }

    /// Windows' change notifications and the tray wheel post to the hidden
    /// window; bursts are coalesced into one refresh.
    fn watch(&mut self) {
        let poster = self.hidden.poster();
        let _ = self.core.audio.watch(move |change| {
            poster.post(if change == bt_audio::Change::Devices {
                APP_DEVICES
            } else {
                APP_SESSIONS
            });
        });
        let poster = self.hidden.poster();
        wheel::init(move |delta| {
            poster.post(if delta > 0 {
                APP_WHEEL_UP
            } else {
                APP_WHEEL_DOWN
            });
        });
    }

    fn reload_theme(&mut self) {
        self.theme = theme::Theme::load(&self.core.store, self.icon_font);
    }

    fn event(&mut self, window: Window, event: Event) -> Option<MenuRequest> {
        if window == self.hidden {
            return self.hidden_event(event);
        }
        if window == self.flyout.window {
            return match event {
                Event::Timer(id) => {
                    self.flyout.timer(id, &mut self.core, &self.theme);
                    None
                }
                Event::DpiChanged => {
                    self.flyout.dpi_changed(&self.theme);
                    None
                }
                event => self.flyout.event(&mut self.core, &self.theme, event),
            };
        }
        if window == self.media.window {
            match event {
                Event::Timer(id) => {
                    let was = self.media.visible;
                    self.media.timer(id, &self.core);
                    if was && !self.media.visible {
                        self.present_tooltip();
                    }
                }
                Event::DpiChanged => {
                    self.media.place();
                    self.media.paint();
                }
                event => match self.media.event(&mut self.core, event) {
                    media::Reply::Hover(true) => self.hidden.kill_timer(MEDIA_HIDE),
                    media::Reply::Hover(false) => {
                        self.hidden.set_timer(MEDIA_HIDE, MEDIA_HIDE_GRACE)
                    }
                    media::Reply::Expanded(expanded) => {
                        if self.core.store.bool(Key::MediaPopupRememberExpanded) {
                            self.core
                                .store
                                .set(Key::MediaPopupIsExpanded, Value::Bool(expanded));
                        }
                        if self.tray_hover {
                            self.present_tooltip();
                        }
                    }
                    media::Reply::None => {}
                },
            }
            return None;
        }
        if window == self.tip.window {
            if let Event::Timer(id) = event {
                self.tip.timer(id);
            }
        }
        None
    }

    fn hidden_event(&mut self, event: Event) -> Option<MenuRequest> {
        match event {
            Event::Tray(TrayEvent::LeftUp | TrayEvent::Select) => {
                self.media.hide();
                self.toggle_flyout();
            }
            Event::Tray(TrayEvent::Context(x, y)) => return Some(self.tray_menu(x, y)),
            Event::Tray(TrayEvent::Move) => self.tray_enter(),
            Event::Tray(TrayEvent::MiddleUp) => {}
            Event::Timer(TRAY_POLL) => {
                let (x, y) = wnd::cursor_pos();
                let inside = self.tray.rect().is_some_and(|(rx, ry, rw, rh)| {
                    x >= rx && x < rx + rw && y >= ry && y < ry + rh
                });
                if !inside {
                    self.tray_leave();
                }
            }
            Event::Timer(MEDIA_SHOW) => {
                self.hidden.kill_timer(MEDIA_SHOW);
                self.maybe_show_media();
            }
            Event::Timer(MEDIA_HIDE) => {
                self.hidden.kill_timer(MEDIA_HIDE);
                if !self.tray_hover && !self.media.hovering {
                    self.media.hide();
                    self.present_tooltip();
                }
            }
            Event::Timer(AUDIO) => {
                self.hidden.kill_timer(AUDIO);
                self.audio_changed();
            }
            Event::Hotkey(id) => {
                match self.hotkeys.get((id - 1).max(0) as usize).copied() {
                    Some(hotkeys::Action::ToggleFlyout) => {
                        if self.flyout.visible {
                            self.flyout.hide();
                        } else {
                            self.show_flyout();
                        }
                    }
                    Some(hotkeys::Action::VolumeUp) => self.core.step_all_devices(2),
                    Some(hotkeys::Action::VolumeDown) => self.core.step_all_devices(-2),
                    None => {}
                }
                self.after_volume_change();
            }
            Event::App(APP_SHOW) => self.show_flyout(),
            Event::App(APP_SHOW_MEDIA) => {
                if let Some(rect) = self.tray.rect() {
                    self.media.show(&self.core, rect);
                    self.present_tooltip();
                }
            }
            Event::App(code @ (APP_DEVICES | APP_SESSIONS)) => {
                self.devices_changed |= code == APP_DEVICES;
                self.hidden.set_timer(AUDIO, 120);
            }
            Event::App(code @ (APP_WHEEL_UP | APP_WHEEL_DOWN)) => {
                if self.tray_hover {
                    self.core
                        .step_default_device(if code == APP_WHEEL_UP { 1 } else { -1 });
                    self.after_volume_change();
                }
            }
            Event::TaskbarCreated => self.tray.add(),
            Event::SettingChange => {
                self.reload_theme();
                if self.flyout.visible {
                    self.flyout.paint(&self.theme);
                }
            }
            _ => {}
        }
        None
    }

    fn after_volume_change(&mut self) {
        self.present_tooltip();
        if self.flyout.showing() {
            self.flyout.refresh(&mut self.core);
            self.flyout.paint(&self.theme);
        }
    }

    /// Windows reported device or session changes: enforce rules and bring
    /// the tooltip and an open flyout up to date.
    fn audio_changed(&mut self) {
        self.devices_changed = false;
        self.core.enforce_rules();
        self.present_tooltip();
        if self.flyout.showing() {
            self.flyout.refresh(&mut self.core);
            self.flyout.paint(&self.theme);
        }
    }

    // ---------- flyout ----------

    fn show_flyout(&mut self) {
        self.media.hide();
        self.tip.hide();
        self.reload_theme();
        let tray = self.tray.rect();
        self.flyout.show(&mut self.core, &self.theme, tray);
    }

    fn toggle_flyout(&mut self) {
        if self.flyout.showing() {
            self.flyout.hide();
            return;
        }
        // A click right after focus loss hid the flyout is the same gesture.
        let recently_hidden = self
            .flyout
            .hidden_at
            .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
        if !recently_hidden {
            self.show_flyout();
        }
    }

    // ---------- tray hover, media popup, tooltip ----------

    fn tray_enter(&mut self) {
        if self.tray_hover {
            return;
        }
        self.tray_hover = true;
        self.tray_rect = self.tray.rect();
        wheel::install();
        self.hidden.set_timer(TRAY_POLL, 100);
        self.hidden.kill_timer(MEDIA_HIDE);
        if self.media.showing() {
            self.present_tooltip();
        } else {
            self.schedule_media_show();
        }
    }

    fn tray_leave(&mut self) {
        self.tray_hover = false;
        wheel::uninstall();
        self.hidden.kill_timer(TRAY_POLL);
        self.hidden.kill_timer(MEDIA_SHOW);
        if self.media.visible {
            self.hidden.set_timer(MEDIA_HIDE, MEDIA_HIDE_GRACE);
        }
        self.present_tooltip();
    }

    /// After MediaPopupHoverDelay, if the popup is enabled.
    fn schedule_media_show(&mut self) {
        let store = &self.core.store;
        if !store.bool(Key::MediaPopupEnabled) {
            return;
        }
        let delay = store.float(Key::MediaPopupHoverDelay);
        let delay = if delay.is_finite() {
            delay.clamp(0.0, 10.0)
        } else {
            2.0
        };
        self.hidden
            .set_timer(MEDIA_SHOW, (delay * 1000.0).round() as u32);
    }

    /// Shows the media popup if the pointer is still on the tray icon, the
    /// flyout is closed and something is playing (or paused, unless
    /// MediaPopupShowOnlyWhenPlaying).
    fn maybe_show_media(&mut self) {
        if !self.tray_hover || self.flyout.visible {
            return;
        }
        let only_playing = self.core.store.bool(Key::MediaPopupShowOnlyWhenPlaying);
        let playing = self
            .core
            .media
            .as_ref()
            .and_then(|m| m.now_playing().ok().flatten())
            .is_some_and(|now| now.playing || !only_playing);
        let Some(rect) = self.tray_rect.or_else(|| self.tray.rect()) else {
            return;
        };
        if playing {
            self.media.show(&self.core, rect);
            self.present_tooltip();
        }
    }

    /// Like the original's RefreshTrayTooltipPresentation: while the popup
    /// is up the native tooltip is blanked and the text shows above the
    /// popup instead (only while the pointer is on the icon).
    fn present_tooltip(&mut self) {
        let text = self
            .core
            .audio
            .devices()
            .map(|d| tooltip(&d))
            .unwrap_or_default();
        if self.media.showing() {
            self.tray.set_tip("");
            match (self.tray_hover, self.media.tray_anchor()) {
                (true, Some((tx, ty, tw, th))) => {
                    let monitor = wnd::monitor_at(tx + tw / 2, ty + th / 2);
                    let screen_bottom = monitor.bounds.1 + monitor.bounds.3;
                    let top = ty.min(screen_bottom - th);
                    // The tip's bottom edge sits 12 DIP over the popup.
                    let bottom = top
                        - ((self.media.height() + 12.0) * monitor.scale).round() as i32
                        + (6.0 * monitor.scale).round() as i32;
                    self.tip.show(&text, tx + tw / 2, bottom);
                }
                _ => self.tip.hide(),
            }
        } else {
            self.tip.hide();
            self.tray.set_tip(&text);
        }
    }

    // ---------- menus ----------

    /// The tray menu: output devices (checked = default), then app actions.
    fn tray_menu(&mut self, x: i32, y: i32) -> MenuRequest {
        let mut items = vec![];
        let mut commands = vec![];
        let devices = self.core.audio.devices().unwrap_or_default();
        if devices.devices.is_empty() {
            items.push(Item::disabled("No output devices"));
        }
        for device in &devices.devices {
            commands.push(MenuCommand::Device(device.id.clone()));
            items.push(Item::check(
                &device.name,
                commands.len() as u32,
                device.is_default,
            ));
        }
        items.push(Item::Separator);
        commands.push(MenuCommand::Startup);
        items.push(Item::check(
            "Start with Windows",
            commands.len() as u32,
            startup::is_enabled(),
        ));
        if !self.shortcuts.is_empty() {
            let shortcuts = self.shortcuts.iter().map(Item::disabled).collect();
            items.push(Item::Submenu("Keyboard shortcuts".into(), shortcuts));
        }
        commands.push(MenuCommand::Mixer);
        items.push(Item::command("Volume mixer", commands.len() as u32));
        commands.push(MenuCommand::Sound);
        items.push(Item::command("Sound settings", commands.len() as u32));
        items.push(Item::Separator);
        commands.push(MenuCommand::Quit);
        items.push(Item::command("Exit", commands.len() as u32));
        MenuRequest {
            owner: self.hidden,
            items,
            commands,
            x,
            y,
            above: true,
        }
    }

    fn menu_command(&mut self, command: MenuCommand) {
        match command {
            MenuCommand::Flyout(command) => {
                self.flyout.command(&mut self.core, &self.theme, command)
            }
            MenuCommand::Device(id) => {
                let _ = self.core.audio.set_default_device(&id);
                self.present_tooltip();
                if self.flyout.showing() {
                    self.flyout.refresh(&mut self.core);
                    self.flyout.paint(&self.theme);
                }
            }
            MenuCommand::Startup => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = startup::set_enabled(!startup::is_enabled(), &exe);
                }
            }
            MenuCommand::Mixer => open("ms-settings:apps-volume"),
            MenuCommand::Sound => open("ms-settings:sound"),
            MenuCommand::Quit => wnd::quit(),
        }
    }
}

fn open(target: &str) {
    let _ = std::process::Command::new("explorer").arg(target).spawn();
}
