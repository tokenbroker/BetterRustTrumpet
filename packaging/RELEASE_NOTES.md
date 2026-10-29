Better Rust Trumpet {version}: BetterTrumpet (EarTrumpet) rewritten in Rust as a small native Windows app.

**Download** `{setup}` below and run it: it installs for your account only (no admin needed), adds a Start menu entry and an uninstaller, and can start with Windows. Or take `{zip}`, unzip it anywhere and run `BetterTrumpet.exe`. Windows SmartScreen may warn because the files aren't code-signed: **More info**, then **Run anyway**. Quit the C# BetterTrumpet or EarTrumpet first if it's running. The other files (`BetterTrumpet.exe`, `bt.exe`, `update.json` and its signature) are for the app's built-in updater.

- Flyout with per-app volume, mute and peak meters; media popup; tray icon that follows the volume.
- Settings window, named themes, settings export/import, the volume mixer window.
- QuickTrumpet presets, the command palette and global shortcuts; focus-lost volume and folder rules.
- The original's translations; high contrast; the `bt` command-line tool.
- About 5 MB of memory and no CPU when idle. No telemetry. Opt-in update check; **Update now** installs a newer release in one click, only if its signature verifies.
- Reads your existing EarTrumpet/BetterTrumpet settings but never changes them.

This is an alpha: much of it hasn't been tested on other PCs yet. Please [open an issue](https://github.com/tokenbroker/BetterRustTrumpet/issues/new/choose) for anything that breaks or looks off, with your Windows version and, if you can, a log from `%LOCALAPPDATA%\BetterTrumpet-Rust\logs`. [docs/MANUAL_TESTS.md](https://github.com/tokenbroker/BetterRustTrumpet/blob/main/docs/MANUAL_TESTS.md) lists things worth trying.

Windows 10 or 11, x64. SHA-256: setup `{setup_sha256}`, zip `{sha256}`
