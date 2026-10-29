# Better Rust Trumpet

A rewrite of [BetterTrumpet](https://github.com/xammen/BetterTrumpet) (itself
built on [EarTrumpet](https://github.com/File-New-Project/EarTrumpet)) in Rust:
per-app volume control from the Windows notification area, with the same
features and settings, in a small native app.

- Native Win32 windows drawn with Direct2D: no web view, no .NET runtime.
- About 5 MB of memory and no CPU while idle; one process.
- Reads your existing EarTrumpet/BetterTrumpet settings but never changes
  them. Its own settings live in `%LOCALAPPDATA%\BetterTrumpet-Rust`.
- No telemetry and no network access.

**Status: alpha.** Most features are in but much of it has not been tested on
other machines yet. Bug reports are very welcome (see below).

## Trying it

1. Download `BetterRustTrumpet.zip` from the
   [releases](../../releases) and unzip it anywhere.
2. Run `BetterTrumpet.exe`. Windows SmartScreen may warn because the file is
   not signed: choose **More info**, then **Run anyway**.
3. Quit the C# BetterTrumpet or EarTrumpet first if it is running; both would
   add a tray icon and claim the same shortcuts.

`bt.exe` is an optional command-line tool (`bt --help`) that talks to the
running app.

To uninstall: quit it from the tray menu, turn off "Run at Windows startup"
first if you turned it on, then delete the folder and
`%LOCALAPPDATA%\BetterTrumpet-Rust`.

## Reporting a problem

Open an [issue](../../issues/new/choose) with what you did, what happened and
your Windows version. Please attach the newest log from
`%LOCALAPPDATA%\BetterTrumpet-Rust\logs`, or a diagnostic report from
Settings > About > Export diagnostic report (which replaces user names in
paths). Logs and reports stay on your PC unless you attach them; they can
contain device and app names, so have a look first.

[docs/MANUAL_TESTS.md](docs/MANUAL_TESTS.md) lists things worth trying.

## Building

Requires Windows 10 or 11 and [Rust](https://rustup.rs) 1.85 or newer (the
MSVC toolchain, with the Visual Studio C++ build tools).

```powershell
cargo build --release
cargo test --workspace
```

The app is `target\release\BetterTrumpet.exe` and the CLI
`target\release\bt.exe`. The C runtime is linked statically
(`.cargo/config.toml`), so the exe runs without the Visual C++
redistributable.

`cargo run -p bt-app --release -- --render <folder>` draws every window to
PNG files without opening anything, which is handy for checking UI changes.

## Workspace

- `bt-core`: settings, rules, presets, CLI and compatibility logic, portable
  and tested against behaviour captured from the original (`tests/fixtures`).
- `bt-storage`: reading the original's settings and the app's own file.
- `bt-audio`: Windows Core Audio (devices, app sessions, volume, peaks).
- `bt-media`: the Windows media session (now playing, controls, art).
- `bt-app`: the tray app; all Win32 code is in `src/win32`.
- `bt-cli`: the `bt` command-line client.

Design decisions are recorded in [docs/DECISIONS.md](docs/DECISIONS.md) (the
`Dnn` and `Qnn` references in the code), feature parity in
[docs/PARITY.md](docs/PARITY.md).

## Credits and licence

Better Rust Trumpet ports the behaviour, translations, sounds and artwork of:

- **EarTrumpet** by David Golden, Rafael Rivera and Dave Amenta, with
  translations by the EarTrumpet community.
- **BetterTrumpet** by xammen.

Code adapted from the .NET runtime, WPF and Newtonsoft.Json is listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md); the translation files are
described in [crates/bt-app/locales/NOTICE.md](crates/bt-app/locales/NOTICE.md).

Released under the [MIT licence](LICENSE), including the upstream preamble
that excludes certain entities. Keep the whole `LICENSE` file, preamble
included, in copies and distributions.
