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

Based on EarTrumpet by David Golden, Rafael Rivera and Dave Amenta and
BetterTrumpet by xammen. Preserve the entire upstream [LICENSE](../LICENSE),
including its preamble, in source and distributions.
