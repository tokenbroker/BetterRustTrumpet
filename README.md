# BetterTrumpet Rust port

This workspace contains the Rust rewrite of BetterTrumpet. It is in development
and is not a replacement release yet. The compatibility inventory, architecture
and acceptance checklist are in [docs/port](../docs/port/INVENTORY.md).

Run the portable core tests with:

```powershell
cd rust
cargo test -p bt-core
```

## Workspace

`bt-core` contains portable compatibility behavior. Windows audio/media, IPC,
Tauri app and CLI crates will be added in the ordered milestones after core
acceptance. The early standalone audio probe has been retired in favor of the
audited architecture.

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
