//! The tray context menu (App.GetTrayContextMenuItems): output devices
//! (checked = default), hidden app entries and hidden devices to restore,
//! the QuickTrumpet presets, the app's windows, Star on GitHub, the Windows
//! audio tools and Exit, with the original's labels and order.
use super::{MenuCommand, MenuRequest, Ui};
use crate::{core::HiddenEntry, profiles, store::with_env};
use bt_app::i18n::{tr, trf};
use bt_app::win32::menu::Item;
use bt_core::{hidden::AppKey, settings::Key, text::ordinal_ignore_case};

// The original's menu icons (PhosphorIconData) and their IconScale.
pub(super) const SLIDERS_HORIZONTAL: &str = "M40,92H70.06a36,36,0,0,0,67.88,0H216a12,12,0,0,0,0-24H137.94a36,36,0,0,0-67.88,0H40a12,12,0,0,0,0,24Zm64-24A12,12,0,1,1,92,80,12,12,0,0,1,104,68Zm112,96H201.94a36,36,0,0,0-67.88,0H40a12,12,0,0,0,0,24h94.06a36,36,0,0,0,67.88,0H216a12,12,0,0,0,0-24Zm-48,24a12,12,0,1,1,12-12A12,12,0,0,1,168,188Z";
pub(super) const GEAR_SIX: &str = "M128,76a52,52,0,1,0,52,52A52.06,52.06,0,0,0,128,76Zm0,80a28,28,0,1,1,28-28A28,28,0,0,1,128,156Zm113.86-49.57A12,12,0,0,0,236,98.34L208.21,82.49l-.11-31.31a12,12,0,0,0-4.25-9.12,116,116,0,0,0-38-21.41,12,12,0,0,0-9.68.89L128,37.27,99.83,21.53a12,12,0,0,0-9.7-.9,116.06,116.06,0,0,0-38,21.47,12,12,0,0,0-4.24,9.1l-.14,31.34L20,98.35a12,12,0,0,0-5.85,8.11,110.7,110.7,0,0,0,0,43.11A12,12,0,0,0,20,157.66l27.82,15.85.11,31.31a12,12,0,0,0,4.25,9.12,116,116,0,0,0,38,21.41,12,12,0,0,0,9.68-.89L128,218.73l28.14,15.74a12,12,0,0,0,9.7.9,116.06,116.06,0,0,0,38-21.47,12,12,0,0,0,4.24-9.1l.14-31.34,27.81-15.81a12,12,0,0,0,5.85-8.11A110.7,110.7,0,0,0,241.86,106.43Zm-22.63,33.18-26.88,15.28a11.94,11.94,0,0,0-4.55,4.59c-.54,1-1.11,1.93-1.7,2.88a12,12,0,0,0-1.83,6.31L184.13,199a91.83,91.83,0,0,1-21.07,11.87l-27.15-15.19a12,12,0,0,0-5.86-1.53h-.29c-1.14,0-2.3,0-3.44,0a12.08,12.08,0,0,0-6.14,1.51L93,210.82A92.27,92.27,0,0,1,71.88,199l-.11-30.24a12,12,0,0,0-1.83-6.32c-.58-.94-1.16-1.91-1.7-2.88A11.92,11.92,0,0,0,63.7,155L36.8,139.63a86.53,86.53,0,0,1,0-23.24l26.88-15.28a12,12,0,0,0,4.55-4.58c.54-1,1.11-1.94,1.7-2.89a12,12,0,0,0,1.83-6.31L71.87,57A91.83,91.83,0,0,1,92.94,45.17l27.15,15.19a11.92,11.92,0,0,0,6.15,1.52c1.14,0,2.3,0,3.44,0a12.08,12.08,0,0,0,6.14-1.51L163,45.18A92.27,92.27,0,0,1,184.12,57l.11,30.24a12,12,0,0,0,1.83,6.32c.58.94,1.16,1.91,1.7,2.88A11.92,11.92,0,0,0,192.3,101l26.9,15.33A86.53,86.53,0,0,1,219.23,139.61Z";
pub(super) const LIST_BULLETS: &str = "M76,64A12,12,0,0,1,88,52H216a12,12,0,0,1,0,24H88A12,12,0,0,1,76,64Zm140,52H88a12,12,0,0,0,0,24H216a12,12,0,0,0,0-24Zm0,64H88a12,12,0,0,0,0,24H216a12,12,0,0,0,0-24ZM44,112a16,16,0,1,0,16,16A16,16,0,0,0,44,112Zm0-64A16,16,0,1,0,60,64,16,16,0,0,0,44,48Zm0,128a16,16,0,1,0,16,16A16,16,0,0,0,44,176Z";

pub(super) const SPARKLE: &str = "M199,125.31l-49.88-18.39L130.69,57a19.92,19.92,0,0,0-37.38,0L74.92,106.92,25,125.31a19.92,19.92,0,0,0,0,37.38l49.88,18.39L93.31,231a19.92,19.92,0,0,0,37.38,0l18.39-49.88L199,162.69a19.92,19.92,0,0,0,0-37.38Zm-63.38,35.16a12,12,0,0,0-7.11,7.11L112,212.28l-16.47-44.7a12,12,0,0,0-7.11-7.11L43.72,144l44.7-16.47a12,12,0,0,0,7.11-7.11L112,75.72l16.47,44.7a12,12,0,0,0,7.11,7.11L180.28,144ZM140,40a12,12,0,0,1,12-12h12V16a12,12,0,0,1,24,0V28h12a12,12,0,0,1,0,24H188V64a12,12,0,0,1-24,0V52H152A12,12,0,0,1,140,40ZM252,88a12,12,0,0,1-12,12h-4v4a12,12,0,0,1-24,0v-4h-4a12,12,0,0,1,0-24h4V72a12,12,0,0,1,24,0v4h4A12,12,0,0,1,252,88Z";

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
    fn icon(
        &mut self,
        text: impl Into<String>,
        command: MenuCommand,
        path: &'static str,
        scale: f32,
    ) -> Item {
        self.commands.push(command);
        Item::icon(text, self.commands.len() as u32, path, scale)
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
        // D55: a newer release the update check found comes first.
        if let bt_app::updates::Status::Available(release) = &self.core.update {
            items.push(b.command(
                trf("UpdateTextAvailable", &[("0", &release.version)]),
                MenuCommand::UpdatePage,
            ));
            items.push(Item::Separator);
        }
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
            items.push(Item::disabled(tr("ContextMenuNoDevices")));
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
                    tr("RestoreHiddenAppsForDeviceAll"),
                    MenuCommand::UnhideDeviceApps(id.clone()),
                ));
                children.push(Item::Submenu(
                    trf(
                        "ContextMenuRestoreHiddenAppsForDeviceFormat",
                        &[("0", &name), ("1", &count.to_string())],
                    ),
                    per_device,
                ));
            }
            children.push(Item::Separator);
            children.push(b.command(
                tr("ContextMenuRestoreAllHiddenApps"),
                MenuCommand::UnhideAllApps,
            ));
            items.push(Item::Separator);
            items.push(Item::Submenu(
                trf(
                    "ContextMenuHiddenAppsTitleFormat",
                    &[("0", &total.to_string())],
                ),
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
            children.push(b.command(
                tr("ContextMenuRestoreAllHiddenDevices"),
                MenuCommand::UnhideAllDevices,
            ));
            if !items.iter().any(|i| matches!(i, Item::Submenu(..))) {
                items.push(Item::Separator);
            }
            items.push(Item::Submenu(
                trf(
                    "ContextMenuHiddenDevicesTitleFormat",
                    &[("0", &count.to_string())],
                ),
                children,
            ));
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
            items.push(Item::Submenu(tr("SettingsSavedProfiles").into(), children));
        }

        items.push(Item::Separator);
        items.push(b.icon(
            tr("TrayOpenVolumeMixer"),
            MenuCommand::Mixer,
            SLIDERS_HORIZONTAL,
            1.02,
        ));
        items.push(b.icon(
            tr("TrayOpenSettingsNew"),
            MenuCommand::Settings,
            GEAR_SIX,
            0.96,
        ));
        items.push(b.icon(
            tr("TrayOpenSettings"),
            MenuCommand::LegacySettings,
            LIST_BULLETS,
            0.94,
        ));
        items.push(Item::Separator);
        items.push(b.icon(
            tr("TrayShowOnboarding"),
            MenuCommand::Onboarding,
            SPARKLE,
            0.92,
        ));
        items.push(b.command(
            tr("TrayReportProblem"),
            MenuCommand::Url(bt_app::links::NEW_ISSUE),
        ));
        // ShowProjectLinks (D54): this project and the original.
        if self.core.store.bool(Key::ShowProjectLinks) {
            items.push(Item::Submenu(
                tr("TrayStarProject").into(),
                vec![
                    b.command(
                        tr("TrayStarThisProject"),
                        MenuCommand::Url(bt_app::links::REPO),
                    ),
                    b.command(
                        tr("TrayStarOriginal"),
                        MenuCommand::Url(bt_app::links::ORIGINAL),
                    ),
                ],
            ));
        }
        items.push(Item::Separator);
        let apps_volume = if self.windows_11 {
            tr("OpenAppsVolume_Windows11_Text")
        } else {
            tr("OpenAppsVolume_Windows10_Text")
        };
        let tools = vec![
            b.command(tr("LegacyVolumeMixerText"), MenuCommand::LegacyMixer),
            b.command(
                tr("PlaybackDevicesText"),
                MenuCommand::ControlPanel("playback"),
            ),
            b.command(
                tr("RecordingDevicesText"),
                MenuCommand::ControlPanel("recording"),
            ),
            b.command(
                tr("SoundsControlPanelText"),
                MenuCommand::ControlPanel("sounds"),
            ),
            b.command(
                tr("OpenSoundSettingsText"),
                MenuCommand::SystemSettings("sound"),
            ),
            b.command(apps_volume, MenuCommand::SystemSettings("apps-volume")),
        ];
        items.push(Item::Submenu(tr("TrayWindowsAudioTools").into(), tools));
        items.push(Item::Separator);
        items.push(b.command(tr("ContextMenuExitTitle"), MenuCommand::Quit));
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
