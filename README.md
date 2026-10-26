# BetterTrumpet Rust port

This workspace contains the Rust rewrite of BetterTrumpet. It is in development
and is not a replacement release yet. The compatibility inventory, architecture
and acceptance checklist are in [docs/port](../docs/port/INVENTORY.md).

Run the portable core tests with:

```powershell
cd rust
cargo test -p bt-core
```

## Preview app

A runnable preview exists while the parity milestones continue (D34). It is a
tray icon whose left click opens a flyout with every active output device and
its apps (volume, mute, peak meters); right click offers the Windows volume
mixer, Sound settings and Exit. It does not read or change BetterTrumpet
settings yet.

```powershell
cd rust
cargo run -p bt-app --release            # tray only
cargo run -p bt-app --release -- --show  # also open the flyout at startup
cargo run -p bt-audio --example sessions # print devices and sessions
```

The release binary is `rust/target/release/BetterTrumpet.exe`. Requires
Windows 10/11 with the WebView2 runtime (preinstalled on Windows 11).

## Workspace

`bt-core` contains portable compatibility behavior. `bt-audio` is the Core
Audio engine (preview scope so far: endpoints, app sessions, volume, mute,
peaks on a dedicated COM thread). `bt-app` is the Tauri v2 tray app. Media,
IPC and CLI crates follow in their milestones.

Model golden fixtures can be regenerated on Windows with .NET 8 desktop runtime:

```powershell
python ../docs/port/capture_model_fixtures.py
```

That helper executes original model source in an isolated temporary project;
it does not start the legacy tray application or access user settings.
It also captures pure profile service scenarios and emits the versioned .NET
text tables used for portable matching and preset slugs.
Portable app/folder/focus/window policy fixtures link the original C# helpers;
Windows full-path resolution remains an explicit native adapter boundary.

See [third-party notices](THIRD_PARTY_NOTICES.md) for adapted runtime helpers.

Based on EarTrumpet by David Golden, Rafael Rivera and Dave Amenta and
BetterTrumpet by xammen. Preserve the entire upstream [LICENSE](../LICENSE),
including its preamble, in source and distributions.
