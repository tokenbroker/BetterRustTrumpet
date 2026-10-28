//! `bt`: the BetterTrumpet command line (CliEntryPoint). Output follows the
//! original: help and version are plain text, a remote command prints a
//! blank line and then the answer indented (or as received when it is not
//! JSON), and a missing app prints two lines of plain text.
use bt_core::protocol::{self, Invocation};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Console.WriteLine on Windows.
fn line(text: &str) {
    print!("{text}\r\n");
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    match protocol::classify(&args) {
        Invocation::Help => help(),
        Invocation::Version => line(&format!("BetterTrumpet v{VERSION}")),
        Invocation::Remote(command) => remote(&command),
        // The original starts the app here; `bt` is only the CLI.
        Invocation::None => help(),
    }
}

#[cfg(windows)]
fn remote(command: &str) {
    line("");
    match bt_cli::client::send(command) {
        Some(response) => line(&protocol::indent_json(&response).unwrap_or(response)),
        None => {
            line("Error: BetterTrumpet is not running or not responding.");
            line("Start BetterTrumpet first, then use CLI commands.");
        }
    }
}

#[cfg(not(windows))]
fn remote(_command: &str) {
    line("");
    line("Error: BetterTrumpet is not running or not responding.");
    line("Start BetterTrumpet first, then use CLI commands.");
}

const HELP: &str = "
BetterTrumpet v{version} - CLI Interface
═══════════════════════════════════════════

Usage: bt <command> [options]

Commands:
  --list-devices                          List all audio devices and apps
  --list-apps                             List running apps with audio sessions
  --get-volume [--device ID]              Get volume of default/specified device
  --set-volume VALUE [--device ID]        Set volume (0-100 or +N/-N relative)
  --set-volume VALUE --app NAME           Set app volume (absolute or relative)
  --mute [--device ID|--app NAME]         Mute device or app
  --unmute [--device ID|--app NAME]       Unmute device or app
  --toggle-mute [--device ID|--app NAME]  Toggle mute on device or app
  --get-default                           Show current default playback device
  --set-default DEVICE_NAME               Change default playback device
  --set-device APP_EXE DEVICE_NAME        Route app audio to specific device
  --list-profiles                         List saved volume profiles
  --apply-profile NAME                    Apply a saved volume profile
  presets                                 List QuickTrumpet presets
  save NAME [--all-devices] [--apps-only] Save current setup as a QuickTrumpet preset
       [--defaults] [--no-apps] [--no-route]  ...also switch default devices / skip apps / don't pin apps
  apply NAME                              Apply a QuickTrumpet preset
  preset next | preset prev               Apply the next/previous preset (wraps around)
  NAME                                    Apply a QuickTrumpet preset directly
  --watch                                 Snapshot all devices/volumes (JSON)
  --check-update                          Check for new version on GitHub
  --export-settings [path]                Export all settings to .btsettings file
  --import-settings <path>                Import settings from .btsettings file
  --ping                                  Check if BetterTrumpet is running
  doctor                                  Diagnose CLI/audio readiness (JSON)
  batch <commands...>                     Run multiple CLI commands sequentially
  resolve-apps QUERY [QUERY...]           Resolve app names for automation
  rule preview --keep APP=VOL --others VOL Preview an app volume rule
  rule apply --keep APP=VOL --others VOL  Apply an app volume rule
  preset create NAME --keep APP=VOL       Create QuickTrumpet preset from rule
  volume [APP] VALUE                      Friendly alias for set-volume
  mute APP | unmute APP                   Friendly app mute aliases
  toggle-mute APP                         Friendly app toggle mute alias
  mode NAME                               Apply a QuickTrumpet preset
  --version                               Show version
  --help                                  Show this help

Examples:
  bt --list-devices
  bt --set-volume 75
  bt --set-volume +10
  bt --set-volume -5 --app spotify
  bt --mute --device \"Speakers\"
  bt --toggle-mute --app discord
  bt --set-default \"Headphones (BEACN Mic)\"
  bt --set-device spotify.exe \"Headphones\"
  bt --apply-profile \"Night Mode\"
  bt save focus
  bt save discord --apps-only
  bt save streaming --all-devices
  bt focus
  bt volume discord 67
  bt toggle-mute discord
  bt rule preview --keep valorant=100 --others 15
  bt preset create \"Valorant Focus\" --keep valorant=100 --others 15 --apps-only
  bt batch --set-volume 67 --app discord --set-volume 30 --app vivaldi
  bt mode Discord

Note: BetterTrumpet must be running for remote commands to work.
All output is JSON for easy scripting and automation.

";

fn help() {
    for text in HELP.replace("{version}", VERSION).lines() {
        line(text);
    }
}
