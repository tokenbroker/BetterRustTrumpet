//! The tray context menu (App.GetTrayContextMenuItems): output devices
//! (checked = default), hidden app entries and hidden devices to restore,
//! the QuickTrumpet presets, the app's windows, Star on GitHub, the Windows
//! audio tools and Exit, with the original's labels and order.
use super::{MenuCommand, MenuRequest, Ui};
use crate::{core::HiddenEntry, profiles, store::with_env};
use bt_app::win32::menu::Item;
use bt_core::{hidden::AppKey, settings::Key, text::ordinal_ignore_case};

fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Builds a menu while assigning command ids.
#[derive(Default)]
struct Builder {
    commands: Vec<MenuCommand>,
}

impl Builder {
    fn command(&mut self, text: impl Into<String>, command: MenuCommand) -> Item {
        self.check(text, command, false)
    }
    fn check(&mut self, text: impl Into<String>, command: MenuCommand, checked: bool) -> Item {
        self.commands.push(command);
        Item::check(text, self.commands.len() as u32, checked)
    }
}

/// Opens a panel of the classic Sound control panel (LegacyControlPanelHelper).
pub fn control_panel(panel: &str) {
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let _ = std::process::Command::new(format!(r"{system}\System32\rundll32.exe"))
        .arg(format!("shell32.dll,Control_RunDLL mmsys.cpl,,{panel}"))
        .spawn();
}

/// The classic volume mixer next to the pointer (sndvol.exe -a).
pub fn legacy_mixer() {
    let (x, y) = bt_app::win32::wnd::cursor_pos();
    let packed = (u32::from(y as u16) << 16) | u32::from(x as u16);
    let _ = std::process::Command::new("sndvol.exe")
        .arg("-a")
        .arg(packed.to_string())
        .spawn();
}

impl Ui {
    pub(super) fn tray_menu(&mut self, x: i32, y: i32) -> MenuRequest {
        let mut b = Builder::default();
        let mut items = vec![];
        let mut devices = self.core.audio.devices().unwrap_or_default().devices;
        devices.sort_by(|a, b| compare(&a.name, &b.name));
        for device in &devices {
            items.push(b.check(
                &device.name,
                MenuCommand::Device(device.id.clone()),
                device.is_default,
            ));
        }
        if devices.is_empty() {
            items.push(Item::disabled("No playback devices found"));
        }

        // Hidden app entries of the active outputs, per device.
        let store = &mut self.core.store;
        let hidden: Vec<(String, String, Vec<HiddenEntry>)> = with_env(|env| {
            devices
                .iter()
                .map(|d| {
                    let entries = store
                        .hidden_apps
                        .for_device(Some(&d.id), env, compare)
                        .into_iter()
                        .map(|e| HiddenEntry {
                            app_id: e.app_id.unwrap_or_default(),
                            exe_name: e.exe_name.unwrap_or_default(),
                            display_name: e.display_name.unwrap_or_default(),
                        })
                        .collect::<Vec<_>>();
                    (d.id.clone(), d.name.clone(), entries)
                })
                .filter(|(_, _, entries)| !entries.is_empty())
                .collect()
        });
        if !hidden.is_empty() {
            let total: usize = hidden.iter().map(|(_, _, e)| e.len()).sum();
            let mut children = vec![];
            for (id, name, entries) in hidden {
                let count = entries.len();
                let mut per_device: Vec<Item> = entries
                    .into_iter()
                    .map(|entry| {
                        let label = entry.label().to_owned();
                        b.command(label, MenuCommand::UnhideApp(id.clone(), entry))
                    })
                    .collect();
                per_device.push(Item::Separator);
                per_device.push(b.command(
                    "Restore all on this device",
                    MenuCommand::UnhideDeviceApps(id.clone()),
                ));
                children.push(Item::Submenu(format!("{name} ({count})"), per_device));
            }
            children.push(Item::Separator);
            children.push(b.command("Restore all hidden app entries", MenuCommand::UnhideAllApps));
            items.push(Item::Separator);
            items.push(Item::Submenu(
                format!("Hidden app entries ({total})"),
                children,
            ));
        }

        let store = &mut self.core.store;
        let hidden_devices = with_env(|env| store.hidden_devices.sorted(env, compare));
        if !hidden_devices.is_empty() {
            let count = hidden_devices.len();
            let mut children: Vec<Item> = hidden_devices
                .into_iter()
                .map(|entry| {
                    let id = entry.device_id.clone().unwrap_or_default();
                    let label = entry.display_name.clone().unwrap_or_else(|| id.clone());
                    b.command(label, MenuCommand::UnhideDevice(id))
                })
                .collect();
            children.push(Item::Separator);
            children.push(b.command("Show all hidden devices", MenuCommand::UnhideAllDevices));
            if !items.iter().any(|i| matches!(i, Item::Submenu(..))) {
                items.push(Item::Separator);
            }
            items.push(Item::Submenu(format!("Hidden devices ({count})"), children));
        }

        // Not in the original's menu: the presets to apply (their hotkeys,
        // the CLI and the settings window are its other ways in).
        let presets = profiles::read(&self.core.store);
        if !presets.is_empty() {
            let last = self
                .core
                .store
                .text(Key::QuickTrumpetLastAppliedId.storage_key())
                .unwrap_or_default();
            let children = presets
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let current =
                        p.id.as_deref()
                            .is_some_and(|id| !last.is_empty() && ordinal_ignore_case(id, &last));
                    b.check(
                        p.name.clone().unwrap_or_default(),
                        MenuCommand::Preset(i),
                        current,
                    )
                })
                .collect();
            items.push(Item::Separator);
            items.push(Item::Submenu("QuickTrumpet presets".into(), children));
        }

        items.push(Item::Separator);
        items.push(b.command("Open volume mixer", MenuCommand::Mixer));
        items.push(b.command("Open settings (beta)", MenuCommand::Settings));
        items.push(b.command("Open settings", MenuCommand::LegacySettings));
        items.push(Item::Separator);
        items.push(b.command("Star on GitHub", MenuCommand::GitHub));
        items.push(Item::Separator);
        let apps_volume = if self.windows_11 {
            "Volume mixer"
        } else {
            "App volume and device preferences"
        };
        let tools = vec![
            b.command("Volume Mixer", MenuCommand::LegacyMixer),
            b.command("Playback devices", MenuCommand::ControlPanel("playback")),
            b.command("Recording devices", MenuCommand::ControlPanel("recording")),
            b.command("Sounds", MenuCommand::ControlPanel("sounds")),
            b.command("Sound settings", MenuCommand::SystemSettings("sound")),
            b.command(apps_volume, MenuCommand::SystemSettings("apps-volume")),
        ];
        items.push(Item::Submenu("Windows audio tools".into(), tools));
        items.push(Item::Separator);
        items.push(b.command("Exit", MenuCommand::Quit));
        MenuRequest {
            owner: self.hidden,
            items,
            commands: b.commands,
            x,
            y,
            above: true,
        }
    }

    /// Restores hidden entries chosen in the tray menu.
    pub(super) fn unhide(&mut self, command: &MenuCommand) {
        let store = &mut self.core.store;
        with_env(|env| match command {
            MenuCommand::UnhideApp(device, entry) => store.hidden_apps.unhide(
                AppKey {
                    device_id: Some(device),
                    app_id: Some(&entry.app_id),
                    exe_name: Some(&entry.exe_name),
                },
                env,
            ),
            MenuCommand::UnhideDeviceApps(device) => {
                store.hidden_apps.unhide_device(Some(device), env)
            }
            MenuCommand::UnhideAllApps => store.hidden_apps.clear(env),
            MenuCommand::UnhideDevice(id) => store.hidden_devices.unhide(Some(id), env),
            MenuCommand::UnhideAllDevices => store.hidden_devices.clear(env),
            _ => {}
        });
        self.core.store.flush();
    }
}
