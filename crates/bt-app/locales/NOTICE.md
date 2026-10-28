# Translations

`Resources*.resx` are copies of `EarTrumpet/Properties/Resources*.resx`
(BetterTrumpet commit `abfd10ed`), unchanged. They are the EarTrumpet and
BetterTrumpet string tables: English by the EarTrumpet authors (David Golden,
Rafael Rivera, Dave Amenta) and xammen, translations by the EarTrumpet
community through Crowdin (`crowdin.yml`). They are used under the MIT licence
in the repository's `LICENSE`, including its Excluded Entities preamble.

`Native.resx` holds the Rust UI's own strings that have no counterpart in the
original tables. It is English only for now; a translation goes in
`Native.<culture>.resx` beside it and is picked up by `build.rs`.

Keep the copies in step with the originals: when `EarTrumpet/Properties`
changes, copy the files again rather than editing them here.
