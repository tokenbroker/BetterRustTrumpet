//! The native UI: the tray icon and its menu, the flyout, the media popup
//! and the tooltip, all on the UI thread. Background threads (audio change
//! notifications, the tray wheel hook, a second launch) only post codes to
//! the hidden window.
pub mod anim;
mod blur;
mod flyout;
mod gif;
mod media;
mod menu;
mod mixer;
mod onboarding;
mod quick;
mod rows;
mod settings;
mod slider;
mod theme;
mod tip;
mod toast;
mod trayicon;
mod traymenu;

use crate::{
    core::{Core, HiddenEntry},
    focus, hotkeys, profiles,
};
use bt_app::i18n::{tr, trf};
use bt_app::win32::{
    gfx::Gfx,
    menu::{self as native_menu, Item},
    tray::Tray,
    trayicon as icon_adapter, wheel,
    wnd::{self, Event, Modifiers, TrayEvent, Window},
};
use bt_audio::Snapshot;
use bt_core::{
    models::AppVolumeEntry,
    settings::{Key, Value},
};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

/// The hidden window's title, which a second launch looks for.
pub const HIDDEN_TITLE: &str = "BetterTrumpet Rust";

// `Event::App` codes posted to the hidden window.
pub const APP_SHOW: usize = 1;
const APP_DEVICES: usize = 2;
const APP_SESSIONS: usize = 3;
const APP_WHEEL_UP: usize = 4;
const APP_WHEEL_DOWN: usize = 5;
const APP_SHOW_MEDIA: usize = 6;
/// Opens (or brings back) the settings window.
pub const APP_SETTINGS: usize = 7;
/// A media session's playback state changed (bt-media's watch).
const APP_MEDIA: usize = 60;

// Timers on the hidden window.
const TRAY_POLL: usize = 1;
const MEDIA_SHOW: usize = 2;
const MEDIA_HIDE: usize = 3;
const AUDIO: usize = 4;
/// Re-applies routed preset apps once the move has settled.
const PRESET_SETTLE: usize = 5;
/// The tray icon's playback animation, only while it runs.
const TRAY_ANIMATION: usize = 60;
/// While the tray icon can't be seen, how often the animation checks
/// whether it can again.
const UNSEEN_CHECK_MS: u32 = 1000;

/// Left Ctrl (Key.LeftCtrl).
const VK_LCONTROL: u16 = 0xa2;

/// The original's popup hide delay after the pointer leaves.
const MEDIA_HIDE_GRACE: u32 = 800;

#[derive(Clone, Debug)]
pub enum MenuCommand {
    Flyout(flyout::Command),
    /// From the mixer window's menus.
    MixerWindow(flyout::Command),
    Device(String),
    /// Device id, hidden entry.
    UnhideApp(String, HiddenEntry),
    UnhideDeviceApps(String),
    UnhideAllApps,
    UnhideDevice(String),
    UnhideAllDevices,
    /// A QuickTrumpet preset by list index.
    Preset(usize),
    Mixer,
    Settings,
    LegacySettings,
    Onboarding,
    GitHub,
    LegacyMixer,
    ControlPanel(&'static str),
    /// An ms-settings: page.
    SystemSettings(&'static str),
    SettingsChoice(settings::Command),
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
    quick: quick::Quick,
    /// The settings window while it is open.
    settings: Option<settings::Settings>,
    /// The setup window while it is open.
    onboarding: Option<onboarding::Onboarding>,
    toasts: toast::Toasts,
    /// The default output last seen, for NotifyOnDefaultDeviceChange.
    last_default: Option<String>,
    /// The mixer window while it is open.
    mixer: Option<mixer::Mixer>,
    gfx: std::rc::Rc<Gfx>,
    theme: theme::Theme,
    icon_font: &'static str,
    tray_hover: bool,
    tray_rect: Option<(i32, i32, i32, i32)>,
    /// Registered shortcut labels ("action: keys", with conflicts), for a
    /// settings page.
    pub shortcuts: Vec<String>,
    /// The actions for hotkey id `i + 1` (bindings sharing keys, in the
    /// original's dispatch order).
    hotkeys: Vec<Vec<hotkeys::Action>>,
    /// The outputs whose DeviceHotkey shortcuts are registered.
    hotkey_devices: Vec<String>,
    last_cycle: Option<Instant>,
    last_preset: Option<(String, Instant)>,
    devices_changed: bool,
    focus: focus::FocusLost,
    windows_11: bool,
    /// What the tray icon shows now (redrawn only when it changes).
    icon: Option<trayicon::State>,
    /// The static icon the default output calls for, under any animation.
    icon_base: Option<trayicon::State>,
    animator: trayicon::Animator,
    /// The display is off (GUID_CONSOLE_DISPLAY_STATE): the animation's
    /// timer stops until it is on again.
    display_off: bool,
    /// When `tray_unseen` last asked the shell, and its answer.
    unseen_checked: Option<(std::time::Instant, bool)>,
    /// The default output's form factor, cached per device id.
    form_factor: Option<(String, Option<u32>)>,
    /// Routed preset apps re-applied by PRESET_SETTLE, and ticks left.
    settle: Vec<AppVolumeEntry>,
    settle_ticks: u32,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

/// A surface for a window that follows the UI culture's flow direction
/// (the original's `FrameworkElementEx.FlowDirection="Auto"`): mirrored
/// for a right-to-left language.
fn flow_surface(gfx: &std::rc::Rc<Gfx>) -> windows::core::Result<bt_app::win32::gfx::Surface> {
    let mut surface = bt_app::win32::gfx::Surface::new(gfx.clone())?;
    surface.set_mirrored(bt_app::i18n::is_rtl());
    Ok(surface)
}

/// Runs `f` on the UI state (if the UI is running and not already in use).
fn with<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|ui| ui.try_borrow_mut().ok()?.as_mut().map(f))
}

/// The original's tray tooltip: "BetterTrumpet: 94% - Headset Earphone
/// (USB)" from the device description and enumerator, under 64 characters.
fn tooltip(devices: &Snapshot) -> String {
    let Some(d) = devices.devices.iter().find(|d| d.is_default) else {
        return tr("NoDeviceTrayText").into();
    };
    let state = if d.muted {
        tr("MutedText").to_string()
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
        quick: quick::Quick::new(gfx.clone())?,
        settings: None,
        onboarding: None,
        toasts: toast::Toasts::new(gfx.clone(), icon_font),
        last_default: devices
            .devices
            .iter()
            .find(|d| d.is_default)
            .map(|d| d.id.clone()),
        mixer: None,
        gfx: gfx.clone(),
        theme: theme::Theme::load(&core.store, icon_font),
        icon_font,
        core,
        hidden,
        tray_hover: false,
        tray_rect: None,
        shortcuts: vec![],
        hotkeys: vec![],
        hotkey_devices: vec![],
        last_cycle: None,
        last_preset: None,
        devices_changed: false,
        focus: focus::FocusLost::new(hidden, hidden.poster()),
        windows_11: icon_adapter::is_windows_11(),
        icon: None,
        icon_base: None,
        animator: trayicon::Animator::default(),
        display_off: false,
        unseen_checked: None,
        form_factor: None,
        settle: vec![],
        settle_ticks: 0,
    };
    ui.register_hotkeys();
    bt_app::win32::presence::watch_display(hidden);
    ui.share_contrast();
    ui.present_tooltip();
    ui.watch();
    // Launch rules for apps already playing, plus Lock and hard mute.
    ui.core.enforce_rules();
    ui.focus.start(&mut ui.core);
    UI.with(|cell| *cell.borrow_mut() = Some(ui));
    wnd::set_handler(|window, event| {
        let menu = with(|ui| ui.event(window, event)).flatten();
        if let Some(request) = menu {
            open_menu(request);
        }
    });
    // Q09: an ordinary launch only marks first run as shown; holding Left
    // Ctrl while the app starts opens the setup window.
    with(|ui| {
        ui.core.store.set(Key::HasShownFirstRun, Value::Bool(true));
        if wnd::key_down(VK_LCONTROL) {
            ui.open_onboarding();
        }
    });
    // `--show` opens the flyout at startup; `--media` the media popup.
    if std::env::args().any(|a| a == "--show") {
        hidden.post(APP_SHOW);
    }
    if std::env::args().any(|a| a == "--media") {
        hidden.post(APP_SHOW_MEDIA);
    }
    // The `bt` CLI's pipe; its commands run on this thread.
    let _pipe = crate::ipc::start(hidden.poster());
    wnd::run();
    // An open mixer keeps its placement, as when it is closed.
    with(|ui| {
        if let Some(mixer) = ui.mixer.as_mut() {
            mixer.close(&mut ui.core);
        }
    });
    // Gives focus-lost apps their volume back.
    with(|ui| ui.focus.stop(&mut ui.core));
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
    let mut tip = tip::Tip::new(gfx.clone())?;
    let devices = core.audio.devices().unwrap_or_default();
    save("tip.png", tip.render(&tooltip(&devices), tip::Look::Tray));
    save(
        "tip-flyout-dark.png",
        tip.render("Microsoft Edge", tip::Look::Wpf { light: false }),
    );
    save(
        "tip-flyout-light.png",
        tip.render(tr("FlyoutPinWindowToolTip"), tip::Look::Wpf { light: true }),
    );
    // Menus: the tray look (over its acrylic tint) and a window's context
    // menu, dark and light, one item lit; and a submenu.
    let sample = vec![
        Item::check("Speakers (Realtek(R) Audio)", 1, true),
        Item::command("Headphones (USB Audio)", 2),
        Item::Separator,
        Item::Submenu(
            trf("ContextMenuHiddenAppsTitleFormat", &[("0", "2")]),
            vec![Item::command("a", 3)],
        ),
        Item::Separator,
        Item::icon(
            tr("TrayOpenVolumeMixer"),
            4,
            traymenu::SLIDERS_HORIZONTAL,
            1.02,
        ),
        Item::icon(tr("TrayOpenSettingsNew"), 5, traymenu::GEAR_SIX, 0.96),
        Item::icon(tr("TrayOpenSettings"), 6, traymenu::LIST_BULLETS, 0.94),
        Item::Separator,
        Item::disabled(tr("ContextMenuNoDevices")),
        Item::Separator,
        Item::command(tr("ContextMenuExitTitle"), 7),
    ];
    for light in [false, true] {
        let mut look = theme.clone();
        look.light = light;
        look.accent_mode = false;
        let name = if light { "light" } else { "dark" };
        save(
            &format!("menu-tray-{name}.png"),
            menu::render(&gfx, &look, &sample, true, Some(5)),
        );
        save(
            &format!("menu-flyout-{name}.png"),
            menu::render(&gfx, &look, &sample[..3], false, Some(1)),
        );
    }
    // High contrast, with Windows 11's "Night sky" colours: the flyout,
    // media popup, tooltips and menus.
    let hc = theme::HighContrast::sample();
    let contrast = theme.clone().high_contrast(hc);
    save("flyout-hc.png", flyout.render(&mut core, &contrast, false));
    media.contrast = Some(hc);
    save("media-hc.png", media.render(&core, tray, true));
    tip.contrast = Some(hc);
    save(
        "tip-hc.png",
        tip.render(&tooltip(&devices), tip::Look::Tray),
    );
    save(
        "tip-flyout-hc.png",
        tip.render("Microsoft Edge", tip::Look::Wpf { light: false }),
    );
    save(
        "menu-tray-hc.png",
        menu::render(&gfx, &contrast, &sample, true, Some(5)),
    );
    save(
        "menu-flyout-hc.png",
        menu::render(&gfx, &contrast, &sample[..3], false, Some(1)),
    );
    // The mixer in the app theme, dark and light, and with a popup open.
    let mut mixer = mixer::Mixer::new(gfx.clone(), &theme)?;
    save(
        "mixer-dark.png",
        mixer.render(&mut core, &theme, false, false),
    );
    save(
        "mixer-light.png",
        mixer.render(&mut core, &theme, true, false),
    );
    save(
        "mixer-popup.png",
        mixer.render(&mut core, &theme, false, true),
    );
    mixer.destroy();
    let mut quick = quick::Quick::new(gfx.clone())?;
    save("quick.png", quick.render(palette_entries(&mut core), ""));
    save(
        "quick-filtered.png",
        quick.render(palette_entries(&mut core), "mute"),
    );
    // Every tray icon at 16, 24 and 32 pixels: one sheet per size and
    // look (modern dark, modern light, legacy dark, legacy light).
    let kinds = [
        trayicon::Kind::Bars(3),
        trayicon::Kind::Muted,
        trayicon::Kind::Bars(1),
        trayicon::Kind::NoDevice,
    ];
    let glyphs = [
        trayicon::Glyph::Speaker,
        trayicon::Glyph::Headphones,
        trayicon::Glyph::Monitor,
    ];
    for size in [16u32, 24, 32] {
        // Modern dark, modern light, legacy dark, legacy light, then high
        // contrast (legacy and "no device" recoloured): WindowText, and
        // HighlightText under the pointer.
        let contrast = theme::HighContrast::sample();
        let looks = [
            (false, false, None),
            (false, true, None),
            (true, false, None),
            (true, true, None),
            (true, false, Some(contrast.tray_icon(false))),
            (true, false, Some(contrast.tray_icon(true))),
        ];
        for (row, (legacy, light, contrast)) in looks.into_iter().enumerate() {
            let states: Vec<_> = glyphs
                .iter()
                .flat_map(|&glyph| {
                    kinds.iter().map(move |&kind| trayicon::State {
                        kind,
                        glyph,
                        light,
                        legacy,
                        size,
                        frame: None,
                        contrast,
                    })
                })
                .collect();
            save(&format!("tray-{size}-{row}.png"), trayicon::sheet(&states));
        }
        // Every frame of each glyph's playback loop, dark then light.
        for glyph in glyphs {
            for light in [false, true] {
                let states: Vec<_> = (0..trayicon::frame_count(glyph))
                    .map(|frame| trayicon::State {
                        kind: trayicon::Kind::Bars(3),
                        glyph,
                        light,
                        legacy: false,
                        size,
                        frame: Some(frame),
                        contrast: None,
                    })
                    .collect();
                let name = format!("{glyph:?}").to_lowercase();
                let look = if light { "light" } else { "dark" };
                save(
                    &format!("tray-anim-{name}-{size}-{look}.png"),
                    trayicon::sheet(&states),
                );
            }
        }
    }
    // Every settings page at full length.
    let mut settings = settings::Settings::new(gfx.clone(), &mut core, icon_font)?;
    for page in settings::PageId::all() {
        let name = format!("settings-{}.png", format!("{page:?}").to_lowercase());
        save(&name, settings.render(&mut core, page, None, false));
    }
    save(
        "settings-appearance-picker.png",
        settings.render(&mut core, settings::PageId::Appearance, None, true),
    );
    save(
        "settings-general-hc.png",
        settings.render_contrast(&mut core, settings::PageId::General, &hc),
    );
    save(
        "settings-general-light.png",
        settings.render(&mut core, settings::PageId::General, Some(true), false),
    );
    for page in [settings::PageId::QuickTrumpet, settings::PageId::AppRules] {
        let name = format!("settings-{}-open.png", format!("{page:?}").to_lowercase());
        save(&name, settings.render_open(&mut core, page));
    }
    settings.close();
    // The setup window's pages (the privacy page also with its
    // confirmation open) and toasts (one look, as in the original).
    let mut setup = onboarding::Onboarding::new(gfx.clone(), &mut core)?;
    for page in 0..onboarding::PAGES {
        save(
            &format!("onboarding-{}.png", page + 1),
            setup.render(page, false),
        );
    }
    save("onboarding-3-confirm.png", setup.render(2, true));
    setup.close();
    let toasts = toast::Toasts::new(gfx, icon_font);
    let device = trf(
        "DefaultDeviceChangedNotificationFormat",
        &[("0", "Speakers (Realtek(R) Audio)")],
    );
    save(
        "toast-device.png",
        toasts.render(&format!(
            "{}\n{device}",
            tr("DefaultDeviceChangedNotificationTitle")
        )),
    );
    save(
        "toast-preset.png",
        toasts.render(&format!(
            "{}\nDefault output: Headphones (USB Audio). 2 devices and 5 apps set; one app was not running.",
            profiles::toast_title("Gaming")
        )),
    );
    Ok(())
}

/// CommandPaletteHelper.BuildCommands from the current devices and apps.
fn palette_entries(core: &mut Core) -> Vec<quick::Entry> {
    use quick::{Command as C, Entry as E};
    let mut list = vec![
        E::new(
            tr("CommandPaletteUndo"),
            "Ctrl+Z",
            &["undo", "revert", "back"],
            C::Undo,
        ),
        E::new(
            tr("CommandPaletteRedo"),
            "Ctrl+Y",
            &["redo", "forward"],
            C::Redo,
        ),
        E::new(
            tr("CommandPaletteOpenMixer"),
            "",
            &["mixer", "full", "window", "volume"],
            C::Mixer,
        ),
        E::new(
            tr("CommandPaletteOpenSettings"),
            "",
            &["settings", "preferences", "config", "options"],
            C::Settings,
        ),
        E::new(
            tr("CommandPaletteOpenPlaybackDevices"),
            "",
            &["playback", "output", "speakers", "devices", "control panel"],
            C::ControlPanel("playback"),
        ),
        E::new(
            tr("CommandPaletteOpenRecordingDevices"),
            "",
            &["recording", "input", "microphone", "mic", "devices"],
            C::ControlPanel("recording"),
        ),
        E::new(
            tr("CommandPaletteOpenSoundSettings"),
            "",
            &["sound", "settings", "windows", "system"],
            C::SoundSettings,
        ),
    ];
    for device in core.view().devices {
        let (id, name) = (device.device.id.clone(), device.device.name.clone());
        let lower = name.to_lowercase();
        let mut entry = E::new(
            trf("CommandPaletteToggleMuteFormat", &[("Name", &name)]),
            "",
            &["mute", "unmute", "toggle"],
            C::ToggleDeviceMute(id.clone(), name.clone()),
        );
        entry.tokens.push(lower.clone());
        list.push(entry);
        let mut entry = E::new(
            trf("CommandPaletteSetDefaultFormat", &[("Name", &name)]),
            "",
            &["default", "output", "device"],
            C::SetDefault(id.clone()),
        );
        entry.tokens.push(lower);
        list.push(entry);
        for app in device.apps.iter().filter(|a| !a.hidden) {
            let app_name = app.app.display_name.clone();
            let mut entry = E::new(
                trf("CommandPaletteToggleMuteFormat", &[("Name", &app_name)]),
                "",
                &["mute", "unmute", "app"],
                C::ToggleAppMute(id.clone(), app.app.key.clone(), app_name.clone()),
            );
            entry.tokens.push(app_name.to_lowercase());
            entry.tokens.push(app.exe_name.to_lowercase());
            list.push(entry);
        }
    }
    list.push(E::new(
        tr("CommandPaletteExit"),
        "",
        &["exit", "quit", "close", "shutdown"],
        C::Exit,
    ));
    list
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
        // Menus follow the same system theme as the flyout; the tray's is
        // owned by the hidden window. The system menu is the fallback.
        let context = with(|ui| (ui.gfx.clone(), ui.theme.clone(), owner == ui.hidden));
        let chosen = context
            .and_then(|(gfx, theme, tray)| {
                menu::popup(&gfx, &theme, owner, &items, x, y, above, tray).ok()
            })
            .unwrap_or_else(|| {
                let dark = !bt_app::win32::theme::system_light();
                native_menu::popup(owner, &items, x, y, above, dark, bt_app::i18n::is_rtl())
            });
        let command = chosen.and_then(|id| commands.get(id as usize - 1).cloned());
        with(|ui| {
            if let Some(command) = command {
                ui.menu_command(command);
            }
            // Keyboard focus returns to the flyout after its menus.
            if ui.flyout.showing() && owner == ui.flyout.window {
                ui.flyout.window.focus();
            }
            if let Some(mixer) = ui.mixer.as_ref().filter(|m| m.window == owner) {
                mixer.window.focus();
            }
        });
    });
}

impl Ui {
    /// Registers every binding (AppSettings.RegisterHotkeys, the preset
    /// shortcuts and the active outputs' DeviceHotkey shortcuts); keys
    /// shared by several bindings are registered once.
    fn register_hotkeys(&mut self) {
        let presets = profiles::read(&self.core.store);
        let devices: Vec<String> = self
            .core
            .audio
            .devices()
            .map(|s| s.devices.into_iter().map(|d| d.id).collect())
            .unwrap_or_default();
        let bindings = hotkeys::bindings(&self.core.store, &presets, &devices);
        let mut keys: Vec<(u32, u32)> = vec![];
        for binding in bindings {
            if let Some(i) = keys
                .iter()
                .position(|&k| k == (binding.modifiers, binding.vk))
            {
                self.hotkeys[i].push(binding.action);
                continue;
            }
            let id = self.hotkeys.len() as i32 + 1;
            let registered = wnd::register_hotkey(self.hidden, id, binding.modifiers, binding.vk);
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
                hotkeys::describe(&binding.action),
                binding.label
            ));
            keys.push((binding.modifiers, binding.vk));
            self.hotkeys.push(vec![binding.action]);
        }
        self.hotkey_devices = devices;
    }

    /// Unregisters and registers every hotkey again: after the presets,
    /// the shortcut settings or the outputs change.
    pub fn reregister_hotkeys(&mut self) {
        for id in 1..=self.hotkeys.len() as i32 {
            wnd::unregister_hotkey(self.hidden, id);
        }
        self.hotkeys.clear();
        self.shortcuts.clear();
        self.register_hotkeys();
    }

    fn hotkey(&mut self, id: i32) {
        let Some(actions) = self.hotkeys.get((id - 1).max(0) as usize).cloned() else {
            return;
        };
        // The first AppSettings binding wins; device shortcuts fire too.
        let main = actions.iter().find(|a| !a.is_device()).cloned();
        let device = actions.iter().find(|a| a.is_device()).cloned();
        for action in main.into_iter().chain(device) {
            self.run_hotkey(action);
        }
    }

    fn run_hotkey(&mut self, action: hotkeys::Action) {
        use hotkeys::Action as A;
        match action {
            A::ToggleFlyout => {
                if self.flyout.visible {
                    self.flyout.hide();
                } else {
                    self.show_flyout();
                }
            }
            A::OpenMixer => self.toggle_mixer(),
            A::OpenSettings => self.open_settings(),
            A::VolumeUp => self.core.step_all_devices(2),
            A::VolumeDown => self.core.step_all_devices(-2),
            A::SwitchDevice => self.cycle_default_device(),
            A::PresetNext | A::PresetPrevious => {
                // WM_HOTKEY repeats while held: one step per 350 ms.
                if self
                    .last_cycle
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(350))
                {
                    return;
                }
                self.last_cycle = Some(Instant::now());
                let direction = if action == A::PresetNext { 1 } else { -1 };
                let target = profiles::Service::open(&mut self.core).cycle_target(direction);
                if let Some(index) = target {
                    self.apply_preset(index);
                }
                return;
            }
            A::Preset(key) => {
                // The same preset's shortcut is ignored for 700 ms.
                if self.last_preset.as_ref().is_some_and(|(last, t)| {
                    last.to_lowercase() == key.to_lowercase()
                        && t.elapsed() < Duration::from_millis(700)
                }) {
                    return;
                }
                self.last_preset = Some((key.clone(), Instant::now()));
                let target = profiles::Service::open(&mut self.core).find(&key);
                if let Some(index) = target {
                    self.apply_preset(index);
                }
                return;
            }
            A::Device(id) => {
                if self.core.audio.set_default_device(&id).is_ok() {
                    let name = self.device_name(&id);
                    self.device_switch_notification(&name);
                }
            }
        }
        self.after_volume_change();
    }

    fn device_name(&self, id: &str) -> String {
        self.core
            .audio
            .endpoints(false)
            .unwrap_or_default()
            .into_iter()
            .find(|e| e.id == id)
            .map(|e| e.name)
            .unwrap_or_default()
    }

    /// ShowDeviceChangeNotification (NotifyOnDeviceHotkeyChange).
    fn device_switch_notification(&mut self, name: &str) {
        if self.core.store.bool(Key::NotifyOnDeviceHotkeyChange) {
            self.toasts.notify(
                tr("DefaultDeviceChangedNotificationTitle"),
                &trf("DefaultDeviceChangedNotificationFormat", &[("0", name)]),
                toast::DEFAULT_MS,
            );
        }
    }

    /// CycleDefaultDevice: the next active output in Windows' order.
    fn cycle_default_device(&mut self) {
        let list = self.core.audio.endpoints(false).unwrap_or_default();
        if list.len() < 2 {
            return;
        }
        let current = self
            .core
            .audio
            .default_endpoint(bt_audio::Role::Playback)
            .ok()
            .flatten();
        let index = current
            .and_then(|c| list.iter().position(|e| e.id == c.id))
            .map_or(0, |i| (i + 1) % list.len());
        let next = &list[index];
        if self.core.audio.set_default_device(&next.id).is_ok()
            && self.core.store.bool(Key::ShowDeviceSwitchNotification)
        {
            self.device_switch_notification(&next.name);
        }
    }

    /// Applies a QuickTrumpet preset and shows its confirmation
    /// (ShowQuickTrumpetConfirmation) as a toast for
    /// QuickTrumpetNotificationSeconds. Settings
    /// pages call this too.
    pub fn apply_preset(&mut self, index: usize) {
        let mut service = profiles::Service::open(&mut self.core);
        let name = service
            .profiles()
            .get(index)
            .and_then(|p| p.name.clone())
            .unwrap_or_default();
        let result = service.apply(index);
        if !result.settle.is_empty() {
            self.settle = result.settle.clone();
            self.settle_ticks = 3;
            self.hidden.set_timer(PRESET_SETTLE, 250);
        }
        if self.core.store.bool(Key::ShowQuickTrumpetConfirmation) {
            let seconds = self.core.store.int(Key::QuickTrumpetNotificationSeconds);
            self.toasts.notify(
                &profiles::toast_title(&name),
                &profiles::describe_result(&result),
                seconds.clamp(1, 10) as u32 * 1000,
            );
        }
        self.after_volume_change();
    }

    /// For the settings window after it saves: redraws the tray icon
    /// (UseLegacyIcon, TrayIconShowsDeviceType) and registers the hotkeys
    /// again (shortcut settings, VolumeProfilesJson, DeviceHotkey:<id>);
    /// services that cache settings re-read them (focus lost).
    pub fn settings_changed(&mut self) {
        self.focus.settings_changed(&mut self.core);
        self.icon = None;
        self.form_factor = None;
        self.reregister_hotkeys();
        self.present_tooltip();
    }

    /// Settings > About's Export button, after its Save dialog
    /// (SettingsExportService.ExportToFile).
    #[allow(dead_code)] // Called by the settings window once it lands.
    pub fn export_settings(&mut self, path: &std::path::Path) -> bool {
        self.core.export_settings(path)
    }

    /// Settings > About's Import button, after its Open dialog; `bt
    /// import-settings` comes here too. Reloads everything that caches
    /// settings: theme, hotkeys, focus lost, tray icon, the settings window.
    #[allow(dead_code)] // Called by the settings window once it lands.
    pub fn import_settings(&mut self, path: &std::path::Path) -> bool {
        let ok = self.core.import_settings(path);
        if ok {
            self.settings_imported();
        }
        ok
    }

    fn settings_imported(&mut self) {
        self.settings_changed();
        self.reload_theme();
        if self.flyout.visible {
            self.flyout.window.set_acrylic(self.theme.acrylic);
            self.flyout.paint(&self.theme);
        }
        if let Some(settings) = &mut self.settings {
            settings.refresh(&mut self.core);
        }
        if let Some(mixer) = &mut self.mixer {
            mixer.retheme(&self.theme);
        }
    }

    /// WindowHolder.OpenOrBringToFront: the tray menu, the palette and the
    /// command palette open the mixer window or bring it forward.
    pub fn open_mixer(&mut self) {
        if let Some(mixer) = self.mixer.as_mut().filter(|m| !m.closing()) {
            mixer.raise();
            return;
        }
        if let Some(mixer) = self.mixer.take() {
            mixer.destroy();
        }
        match mixer::Mixer::new(self.gfx.clone(), &self.theme) {
            Ok(mut mixer) => {
                mixer.open(&mut self.core);
                self.mixer = Some(mixer);
            }
            Err(e) => eprintln!("BetterTrumpet: the mixer window failed to open: {e}"),
        }
    }

    /// WindowHolder.OpenOrClose (the mixer shortcut).
    fn toggle_mixer(&mut self) {
        match self.mixer.as_mut().filter(|m| !m.closing()) {
            Some(mixer) => {
                mixer.close(&mut self.core);
                self.reap_mixer();
            }
            None => self.open_mixer(),
        }
    }

    /// Destroys the mixer window once its exit fade has played.
    fn reap_mixer(&mut self) {
        if self.mixer.as_ref().is_some_and(|m| m.closed) {
            if let Some(mixer) = self.mixer.take() {
                mixer.destroy();
            }
        }
    }

    /// Ctrl+Shift+P: the command palette.
    fn open_palette(&mut self) {
        let entries = palette_entries(&mut self.core);
        self.quick.show(entries);
    }

    fn palette_command(&mut self, command: quick::Command) {
        use quick::Command as C;
        match command {
            C::Undo => {
                self.core.replay(true);
            }
            C::Redo => {
                self.core.replay(false);
            }
            C::Mixer => self.open_mixer(),
            C::Settings => self.open_settings(),
            C::ControlPanel(panel) => traymenu::control_panel(panel),
            C::SoundSettings => open("ms-settings:sound"),
            C::ToggleDeviceMute(id, name) => {
                let muted = self
                    .core
                    .audio
                    .devices()
                    .ok()
                    .and_then(|s| s.devices.into_iter().find(|d| d.id == id))
                    .is_some_and(|d| d.muted);
                self.core.set_device_mute(&id, &name, !muted);
            }
            C::SetDefault(id) => {
                let _ = self.core.audio.set_default_device(&id);
            }
            C::ToggleAppMute(device, key, name) => {
                let muted = self
                    .core
                    .view()
                    .app(&device, &key)
                    .is_some_and(|a| a.app.muted);
                self.core.set_app_mute(&device, &key, &name, !muted);
            }
            C::Exit => wnd::quit(),
        }
        self.after_volume_change();
    }

    fn unregister_hotkeys(&mut self) {
        for id in 1..=self.hotkeys.len() {
            wnd::unregister_hotkey(self.hidden, id as i32);
        }
        self.hotkeys.clear();
        self.shortcuts.clear();
    }

    /// Registers the shortcuts again after the settings changed them.
    fn reload_hotkeys(&mut self) {
        self.unregister_hotkeys();
        self.register_hotkeys();
    }

    // ---------- setup window ----------

    /// ShowOnboardingManually (the tray menu) and Left Ctrl at startup:
    /// opens the setup window, or brings the open one forward.
    fn open_onboarding(&mut self) {
        if self.onboarding.is_none() {
            match onboarding::Onboarding::new(self.gfx.clone(), &mut self.core) {
                Ok(window) => self.onboarding = Some(window),
                Err(e) => eprintln!("BetterTrumpet: the setup window failed to open: {e}"),
            }
        }
        if let Some(onboarding) = &mut self.onboarding {
            onboarding.present();
        }
    }

    fn onboarding_outcome(&mut self, outcome: onboarding::Outcome) {
        if outcome.settings {
            // The theme, consent and startup reach everything that shows
            // or caches them.
            self.settings_imported();
        }
        if outcome.device {
            self.after_volume_change();
        }
        if outcome.close {
            if let Some(onboarding) = self.onboarding.take() {
                onboarding.close();
            }
        }
    }

    // ---------- settings window ----------

    /// Opens (or brings forward) the settings window; the tray menu, the
    /// palette and the settings hotkey all come through here.
    pub fn open_settings(&mut self) {
        if self.settings.is_none() {
            match settings::Settings::new(self.gfx.clone(), &mut self.core, self.icon_font) {
                Ok(window) => self.settings = Some(window),
                Err(e) => {
                    eprintln!("BetterTrumpet: the settings window failed to open: {e}");
                    bt_app::log::line(format!("Settings window failed to open: {e}"));
                }
            }
        }
        if let Some(settings) = &mut self.settings {
            settings.present();
        }
    }

    /// Applies what a settings change asks of the rest of the app: the
    /// flyout theme, hotkey registration and closing.
    fn settings_outcome(&mut self, outcome: settings::Outcome) -> Option<MenuRequest> {
        if outcome.theme {
            // Every setting change lands here: services that cache
            // settings re-read them and the tray icon is redrawn.
            self.focus.settings_changed(&mut self.core);
            self.icon = None;
            self.form_factor = None;
            self.present_tooltip();
            self.reload_theme();
            if self.flyout.visible {
                self.flyout.window.set_acrylic(self.theme.acrylic);
                self.flyout.paint(&self.theme);
            }
            if let Some(mixer) = &mut self.mixer {
                mixer.retheme(&self.theme);
            }
        }
        // Recording a shortcut must not trigger the registered ones.
        match outcome.capture {
            Some(true) => self.unregister_hotkeys(),
            Some(false) => self.reload_hotkeys(),
            None if outcome.hotkeys => self.reload_hotkeys(),
            None => {}
        }
        if let Some(index) = outcome.apply_preset {
            self.apply_preset(index);
            if let Some(settings) = &mut self.settings {
                settings.refresh(&mut self.core);
            }
        }
        if outcome.close {
            if let Some(settings) = self.settings.take() {
                let recording = settings.recording();
                settings.close();
                if recording {
                    self.reload_hotkeys();
                }
            }
        }
        outcome.menu
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
        // Playback changes drive the tray animation; the current state is
        // read once now (the hidden window handles the post after startup).
        if let Some(media) = &self.core.media {
            let poster = self.hidden.poster();
            let _ = media.watch(move || poster.post(APP_MEDIA));
        }
        self.hidden.post(APP_MEDIA);
        self.animator.legacy_playing = crate::legacy_media::current(&self.core).is_some();
    }

    fn reload_theme(&mut self) {
        self.theme = theme::Theme::load(&self.core.store, self.icon_font);
        self.share_contrast();
    }

    /// Hands the high-contrast colours to the windows that draw their own
    /// look (tooltips, the media popup).
    fn share_contrast(&mut self) {
        let hc = self.theme.hc;
        self.tip.contrast = hc;
        self.flyout.tip.contrast = hc;
        self.media.contrast = hc;
    }

    fn event(&mut self, window: Window, event: Event) -> Option<MenuRequest> {
        if window == self.hidden {
            return self.hidden_event(event);
        }
        if window == self.quick.window {
            if let quick::Reply::Run(command) = self.quick.event(event) {
                self.palette_command(command);
            }
            return None;
        }
        // Ctrl+Shift+P: the command palette, from the mixer window as in the
        // original, and from the flyout too.
        let palette = matches!(
            event,
            Event::Key(
                0x50,
                Modifiers {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    win: false,
                },
            )
        );
        if self.mixer.as_ref().is_some_and(|m| m.window == window) {
            let menu = if palette {
                self.open_palette();
                None
            } else {
                let mixer = self.mixer.as_mut()?;
                match event {
                    Event::Timer(id) => {
                        mixer.timer(id, &mut self.core);
                        None
                    }
                    event => mixer.event(&mut self.core, event),
                }
            };
            self.reap_mixer();
            return menu;
        }
        if let Some(mixer) = self.mixer.as_mut().filter(|m| m.tip.window == window) {
            if let Event::Timer(id) = event {
                mixer.tip.timer(id);
            }
            return None;
        }
        if window == self.flyout.window {
            if palette {
                self.open_palette();
                return None;
            }
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
        if self.toasts.owns(window) {
            self.toasts.event(window, event);
            return None;
        }
        if self.onboarding.as_ref().is_some_and(|o| o.window == window) {
            let outcome = self
                .onboarding
                .as_mut()
                .map(|o| o.event(&mut self.core, event))?;
            self.onboarding_outcome(outcome);
            return None;
        }
        if self.settings.as_ref().is_some_and(|s| s.window == window) {
            let outcome = self
                .settings
                .as_mut()
                .map(|s| s.event(&mut self.core, event))?;
            return self.settings_outcome(outcome);
        }
        if window == self.tip.window {
            if let Event::Timer(id) = event {
                self.tip.timer(id);
            }
        }
        if window == self.flyout.tip.window {
            if let Event::Timer(id) = event {
                self.flyout.tip.timer(id);
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
            // TertiaryInvoke: mute or unmute the default output.
            Event::Tray(TrayEvent::MiddleUp) => {
                let devices = self.core.audio.devices().unwrap_or_default();
                if let Some(d) = devices.devices.iter().find(|d| d.is_default) {
                    self.core.set_device_mute(&d.id, &d.name, !d.muted);
                }
                self.after_volume_change();
            }
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
            Event::Timer(TRAY_ANIMATION) => {
                if let Some(base) = self.icon_base {
                    // Nobody sees the icon under a full-screen app or a
                    // locked session: check again in a second instead.
                    if self.tray_unseen() {
                        self.hidden.set_timer(TRAY_ANIMATION, UNSEEN_CHECK_MS);
                    } else {
                        self.animator.tick(&base);
                        self.show_icon();
                        self.hidden.set_timer(TRAY_ANIMATION, self.animator.delay());
                    }
                }
            }
            Event::Display(on) => {
                self.display_off = !on;
                if self.animator.running() {
                    if on {
                        self.hidden.set_timer(TRAY_ANIMATION, self.animator.delay());
                    } else {
                        self.hidden.kill_timer(TRAY_ANIMATION);
                    }
                }
            }
            Event::App(APP_MEDIA) => {
                let playing = self
                    .core
                    .media
                    .as_ref()
                    .is_some_and(|m| m.any_playing().unwrap_or(false));
                if playing != self.animator.session_playing {
                    bt_app::log::line(format!(
                        "MediaSessionService: Playback state changed - IsPlaying={playing}"
                    ));
                }
                self.animator.session_playing = playing;
                self.animate();
            }
            Event::Timer(focus::FADE_TIMER) => self.focus.timer(&mut self.core),
            Event::App(focus::APP_FOREGROUND) => self.focus.foreground_changed(&mut self.core),
            Event::Timer(AUDIO) => {
                self.hidden.kill_timer(AUDIO);
                self.audio_changed();
            }
            Event::Timer(PRESET_SETTLE) => {
                for app in &self.settle {
                    profiles::apply_app_state(&self.core, app);
                }
                self.settle_ticks = self.settle_ticks.saturating_sub(1);
                if self.settle_ticks == 0 {
                    self.hidden.kill_timer(PRESET_SETTLE);
                    self.settle.clear();
                }
            }
            Event::Hotkey(id) => self.hotkey(id),
            Event::App(APP_SHOW) => self.show_flyout(),
            Event::App(crate::ipc::APP_IPC) => {
                let effects = crate::ipc::drain(&mut self.core);
                if effects.reload {
                    self.settings_imported();
                } else if effects.hotkeys {
                    self.reload_hotkeys();
                }
                if !effects.settle.is_empty() {
                    self.settle = effects.settle;
                    self.settle_ticks = 3;
                    self.hidden.set_timer(PRESET_SETTLE, 250);
                }
            }
            Event::App(APP_SETTINGS) => self.open_settings(),
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
                // UseScrollWheelInTray; with the global wheel hook the
                // flyout takes the wheel while it is open.
                let store = &self.core.store;
                let enabled = store.bool(Key::UseScrollWheelInTray)
                    && (!store.bool(Key::UseGlobalMouseWheelHook) || !self.flyout.visible);
                if self.tray_hover && enabled {
                    self.core
                        .step_default_device(if code == APP_WHEEL_UP { 1 } else { -1 });
                    self.after_volume_change();
                }
            }
            Event::TaskbarCreated => {
                self.tray.add();
                self.icon = None;
                self.present_tooltip();
            }
            Event::SettingChange => {
                self.reload_theme();
                self.present_tooltip();
                if self.flyout.visible {
                    self.flyout.paint(&self.theme);
                }
                if let Some(mixer) = &mut self.mixer {
                    mixer.retheme(&self.theme);
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
        if let Some(mixer) = &mut self.mixer {
            mixer.refresh(&mut self.core);
        }
    }

    /// Windows reported device or session changes: enforce rules and bring
    /// the tooltip and an open flyout up to date.
    fn audio_changed(&mut self) {
        if self.devices_changed {
            let ids: Vec<String> = self
                .core
                .audio
                .devices()
                .map(|s| s.devices.into_iter().map(|d| d.id).collect())
                .unwrap_or_default();
            if ids != self.hotkey_devices {
                self.reregister_hotkeys();
            }
        }
        self.devices_changed = false;
        self.default_change_notification();
        // A session started or stopped playing: maybe a legacy player.
        let legacy = crate::legacy_media::current(&self.core);
        if legacy.is_some() != self.animator.legacy_playing {
            bt_app::log::line(match &legacy {
                Some(player) => {
                    format!("LegacyMediaPlayerService: {} is playing", player.exe_name())
                }
                None => "LegacyMediaPlayerService: no legacy player playing".to_owned(),
            });
        }
        self.animator.legacy_playing = legacy.is_some();
        self.core.enforce_rules();
        self.focus.audio_changed(&mut self.core);
        self.present_tooltip();
        if self.flyout.showing() {
            self.flyout.refresh(&mut self.core);
            self.flyout.paint(&self.theme);
        }
        if let Some(mixer) = &mut self.mixer {
            mixer.refresh(&mut self.core);
        }
    }

    /// OnDefaultPlaybackDeviceChanged: a toast when the default output
    /// changed (NotifyOnDefaultDeviceChange), not for the first one seen.
    fn default_change_notification(&mut self) {
        let Some(device) = self
            .core
            .audio
            .devices()
            .ok()
            .and_then(|s| s.devices.into_iter().find(|d| d.is_default))
        else {
            return;
        };
        if bt_core::window_policy::should_notify_default_change(
            self.last_default.as_deref(),
            Some(&device.id),
            self.core.store.bool(Key::NotifyOnDefaultDeviceChange),
        ) {
            self.toasts.notify(
                tr("DefaultDeviceChangedNotificationTitle"),
                &trf(
                    "DefaultDeviceChangedNotificationFormat",
                    &[("0", &device.name)],
                ),
                toast::DEFAULT_MS,
            );
        }
        if !device.id.trim().is_empty() {
            self.last_default = Some(device.id);
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
        let playing = crate::legacy_media::now_playing(&self.core)
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
        let devices = self.core.audio.devices();
        if let Ok(devices) = &devices {
            self.refresh_icon(devices);
        }
        let text = devices.map(|d| tooltip(&d)).unwrap_or_default();
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

    // ---------- tray icon ----------

    /// TaskbarIconSource: redraws the icon when the default output's level
    /// or mute, its kind, the theme, the taskbar DPI or UseLegacyIcon
    /// change.
    fn refresh_icon(&mut self, devices: &Snapshot) {
        let store = &self.core.store;
        let legacy = store.bool(Key::UseLegacyIcon);
        let shows_type = store.bool(Key::TrayIconShowsDeviceType);
        self.animator.disabled = !store.bool(Key::AnimateTrayIcon);
        let default = devices.devices.iter().find(|d| d.is_default);
        let form = match default {
            Some(d) if shows_type && !legacy => match &self.form_factor {
                Some((id, form)) if *id == d.id => *form,
                _ => {
                    let form = self.core.audio.form_factor(&d.id).ok().flatten();
                    self.form_factor = Some((d.id.clone(), form));
                    form
                }
            },
            _ => None,
        };
        let monitor = match self.tray.rect() {
            Some((x, y, w, h)) => wnd::monitor_at(x + w / 2, y + h / 2),
            None => wnd::monitor_at(0, 0),
        };
        let dpi = (monitor.scale * 96.0).round() as u32;
        let state = trayicon::State {
            kind: trayicon::kind(default.map(|d| (d.volume, d.muted)), self.windows_11),
            glyph: trayicon::glyph(form, shows_type, legacy),
            light: bt_app::win32::theme::system_light(),
            legacy,
            size: icon_adapter::small_icon_size(dpi),
            frame: None,
            contrast: self.theme.hc.map(|hc| hc.tray_icon(self.tray_hover)),
        };
        self.icon_base = Some(state);
        self.animate();
    }

    /// Starts or stops the playback animation for the current icon (and
    /// its timer, which runs only while animating), then shows the icon.
    fn animate(&mut self) {
        let Some(base) = self.icon_base else {
            return;
        };
        if self.animator.update(&base) {
            bt_app::log::line(if self.animator.running() {
                "TaskbarIconSource: Animation started"
            } else {
                "TaskbarIconSource: Animation stopped"
            });
            if self.animator.running() {
                self.hidden.set_timer(TRAY_ANIMATION, self.animator.delay());
            } else {
                self.hidden.kill_timer(TRAY_ANIMATION);
            }
        }
        self.show_icon();
    }

    /// Whether nobody can see the tray icon right now: the display is off,
    /// the session is locked or a screen saver runs, or a full-screen app
    /// covers the taskbar's monitor. Asked of the shell at most once a
    /// second.
    fn tray_unseen(&mut self) -> bool {
        if self.display_off {
            return true;
        }
        let now = std::time::Instant::now();
        if let Some((at, unseen)) = self.unseen_checked
            && now.duration_since(at).as_millis() < u128::from(UNSEEN_CHECK_MS)
        {
            return unseen;
        }
        let rect = self.tray_rect.or_else(|| self.tray.rect());
        let unseen = bt_app::win32::presence::tray_unseen(rect);
        self.unseen_checked = Some((now, unseen));
        unseen
    }

    /// Pushes the icon to the shell only when what it shows changed (the
    /// animation's repeated poses included, GitHub #74).
    fn show_icon(&mut self) {
        let Some(base) = self.icon_base else {
            return;
        };
        let state = self.animator.shown(base);
        if self.icon == Some(state) {
            return;
        }
        if let Some(icon) = self.animator.icon(state) {
            self.tray.set_icon(icon);
            self.icon = Some(state);
        }
    }

    // ---------- menus ----------

    fn menu_command(&mut self, command: MenuCommand) {
        match command {
            MenuCommand::Flyout(command) => {
                self.flyout.command(&mut self.core, &self.theme, command)
            }
            MenuCommand::MixerWindow(command) => {
                if let Some(mixer) = &mut self.mixer {
                    mixer.command(&mut self.core, command);
                }
            }
            MenuCommand::Device(id) => {
                let _ = self.core.audio.set_default_device(&id);
                self.after_volume_change();
            }
            command @ (MenuCommand::UnhideApp(..)
            | MenuCommand::UnhideDeviceApps(_)
            | MenuCommand::UnhideAllApps
            | MenuCommand::UnhideDevice(_)
            | MenuCommand::UnhideAllDevices) => {
                self.unhide(&command);
                self.after_volume_change();
            }
            MenuCommand::Preset(index) => self.apply_preset(index),
            MenuCommand::Mixer => self.open_mixer(),
            MenuCommand::Settings | MenuCommand::LegacySettings => self.open_settings(),
            MenuCommand::Onboarding => self.open_onboarding(),
            MenuCommand::GitHub => open("https://github.com/xammen/BetterTrumpet"),
            MenuCommand::LegacyMixer => traymenu::legacy_mixer(),
            MenuCommand::ControlPanel(panel) => traymenu::control_panel(panel),
            MenuCommand::SystemSettings(page) => open(&format!("ms-settings:{page}")),
            MenuCommand::SettingsChoice(choice) => {
                if let Some(outcome) = self
                    .settings
                    .as_mut()
                    .map(|s| s.command(&mut self.core, choice))
                {
                    if let Some(menu) = self.settings_outcome(outcome) {
                        open_menu(menu);
                    }
                }
            }
            MenuCommand::Quit => wnd::quit(),
        }
    }
}

fn open(target: &str) {
    let _ = std::process::Command::new("explorer").arg(target).spawn();
}
