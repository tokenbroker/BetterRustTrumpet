# BetterTrumpet Rust parity checklist

This file is the source of truth for completion. An unchecked item is incomplete, even if code or a prototype exists. Inventory IDs (A01, C11, S07...) refer to the Phase 0 audit (INVENTORY.md, BEHAVIOR_REVIEW.md, COVERAGE.md, PROGRESS.md and the source catalog), which lives in the private audit workspace and is not published here. No Rust milestone is complete.

## Status at a glance (reviewed against the code on 2026-10-29)

Every open item below was re-checked against the code; the notes under each say what exists and what is missing. Most features are built but, by D47, only compile-checked, unit-tested or drawn offscreen: the testing pass has not happened yet.

- **Checked live:** the update check, update notification and one-click update (MANUAL_TESTS 46, 47, 49), the setup (48), the tray menu's update item, exited-process sessions on a user's PC (alpha.6), and `bt ping`.
- **Switches that do nothing yet:** Logarithmic volume (stored, never applied), Dynamic album-art theme (stored, never applied) and the monkey sound (no unlock and no sound files).
- **Missing features:** settings migration from a portable or Microsoft Store (MSIX) BetterTrumpet (only the installed/registry one is read); the tray update badge; settings search, sidebar collapse and window placement; theme import/export and the 300 ms transition; per-channel volume and input-device control; screen-reader support; RTL and high contrast in the settings window, palette, onboarding and toasts; seek retry and the media session fallback; health monitoring and crash exports; arm64 builds; winget and Chocolatey manifests; Crowdin workflow.
- **Decisions still needed:** the Actions addon (B03/D22); whether the IPC pipe keeps its own name (it differs from the original's on purpose, so the two apps can run side by side, which rules out C#/Rust cross-compatibility); `BetterTrumpet.exe` does not answer CLI arguments itself and no `bt.cmd` ships; items written for the web UI that D48 replaced. Telemetry, Sentry, the announcement feed and votes are not ported (D59).
- **Not measured:** warm flyout latency, idle RAM/CPU and size against the C# build.

## Phase 0 — source audit gate

- [x] Clone BetterTrumpet and record current revision/working-tree state.
- [x] Mechanically read/hash every tracked file in requested projects and named documents; record [source catalog](SOURCE_CATALOG.md).
- [x] Create source-cited INVENTORY.md, PARITY.md and DECISIONS.md.
- [x] Extract settings calls/defaults/dynamic keys; preserve exact CLI implementations and integration call sites.
- [x] Catalog all RESX keys/translations and binary assets.
- [x] Complete semantic review of every project, including remaining UI control/navigation details, legacy media behavior, Actions/MEF reachability/serialization and worker request branches. See BEHAVIOR_REVIEW.md and referenced source.
- [x] Expand settings bridge into exact typed inbound/outbound payload contracts for every action/settings/state/hotkey message; review file-format/export omissions and migrations. See BRIDGE.md and SETTINGS_MIGRATION.md (static review; runtime checks remain open).
- [x] Confirm all inventory features have explicit parity acceptance items, including extra features outside the user's outline; complete ambiguity/quirk dispositions. All 96 IDs mapped in COVERAGE.md; later runtime questions retain explicit milestone requirements.
- [x] Review all source citations and audit extraction coverage; commit the completed Phase 0 baseline and note it in PROGRESS.md. Reproducible checks: `python docs/port/verify_audit.py`.

## Milestone 1 — workspace and core models

In progress: workspace/bt-core, CLI/protocol/response DTOs, batch/Doctor aggregation, undo/redo, scalar settings, persisted DTOs, profile normalization/lookup/cycle/import/edit logic, rule plans and theme palettes/import format are implemented. Actual-C# model/service-method/response golden fixtures and Windows tests pass (Linux deferred, D44). Complete settings/migration/rule lifecycle, Newtonsoft noncanonical input conversions and portable CLI validation remain open; no milestone completion is claimed.

- [ ] Implement D01–D04 workspace and portable bt-core; no unsafe/Windows/Tauri dependencies in core.
  - The workspace is at the repository root and has no separate `bt-ipc` crate (protocol in bt-core, pipe in bt-app). bt-core forbids unsafe and has no Windows dependencies. Unsafe also lives in bt-cli's `win32.rs` and across bt-app's `win32/` shim; D04 should be updated to say so.
- [ ] Model every S01–S14 setting, default, clamp, enum, computed key, hotkey representation and notification-relevant change.
  - [x] Decoded scalar settings/defaults, eight hotkeys, consent-key presence, lazy vote ID, device/poll keys, battery eco/FPS helpers, read/write clamps and notification/batch behavior match executed C# property fixtures.
  - [ ] Complete color/placement values, raw JSON collection setter lifecycles, addon/local-variable namespace models, native-boundary command/result models and legacy decoding. Native registration/storage/startup execution is accepted in milestones 4/9/10.
  - Still open: colour values are untyped text outside `Key`, no addon namespace model, JSON collection setter lifecycles incomplete.
- [ ] Preserve F01–F06 registry/MSIX/portable XML/JSON representations through read-only migration adapters; source data never overwritten; preserve unknown fields.
  - [x] XmlSerializer scalar (bool/int/uint/float/double/string), HotkeyData and (nullable) WINDOWPLACEMENT decoding matches the original Serializer.FromString across captured samples and reader edge cases (D30/Q30).
  - [x] Per-flavor `Get<T>` read semantics for registry, package and portable JSON bags (including Newtonsoft lenient parsing, date reparsing, fallback conversions and namespaced keys) match the original bags executed against in-memory stand-ins (D31/Q31).
  - [x] Color values: WPF ColorConverter parsing, Color.ToString text and ParseColorSetting fallback match .NET 8 WPF under invariant and de-DE cultures (D32/Q32). The native current-culture adapter is accepted with milestone 3's culture boundary (D29).
  - [x] Portable store-selection policy matches StorageFactory precedence for all marker/identity combinations; portable read-only snapshots retain exact original bytes, including unknown values and malformed files. Native probes and enumeration remain open.
  - [x] Portable file migration archives the complete original bytes under config/rust/migration.v1.json, reloads legacy reads without a whitelist, distinguishes malformed/missing input and never replaces an existing archive. Windows/Linux tests cover concurrent initializers, failed reads/writes, interrupted staging and a source hard-link alias (D40).
  - [x] Registry raw snapshot/archive format and query-only Windows HKCU enumeration preserve UTF-16 names, type tags and exact NT data bytes. Installed archive persistence is idempotent and bypasses later capture; native tests run against disposable keys (D41).
  - [x] Registry raw-value to .NET GetValue projection, typed bag reads, null-default string routing, HasKey and key lookup match native .NET/original-bag captures (D42/Q40). Windows casing/expansion adapters pass native tests; cross-version deployment remains MANUAL_TESTS 21.
  - [x] Editable Rust overrides layered over the immutable archives, with lossless unknown-field preservation and conflict-safe atomic saves (D43).
  - [x] Packaged LocalSettings discovery, lossless read-only enumeration, archive and bag read semantics (D45); verified natively against installed packages. End-to-end migration from a real BetterTrumpet MSIX install is MANUAL_TESTS 23.
  - [ ] App startup integration: select the installed/portable/packaged source, run migration and load archive plus overrides in the app. Libraries alone do not satisfy this gate.
    - Installed (registry) only: `Store::open` calls `registry::migrate_legacy_settings`. The packaged reader (`win32/package.rs`) is never called and portable selection is not wired, so Store and portable users start with defaults.
- [ ] Model schema 1/2/3 profiles, default roles, IncludeVolume/RouteApps/ApplyAppsOnly and migration quirks (S13/B02).
  - Done in bt-core with compatibility tests (`profile_compatibility.rs`, `typed_profile_compatibility.rs`); not round-tripped through the running app.
- [ ] Model app/folder rules, hidden app/device identity and pinned neutral rules (S02–S04).
  - [x] App-rule/hidden-list normalization and parsed legacy hard-mute fallback match executed C# fixtures: first duplicate wins, pinned neutral entries, unknown modes, clamps, invariant lowercase and delimiter collisions.
  - [x] Folder-rule load/normalization/ID and time repair, ordering, serialization and add/update/remove/set/match lifecycle, including Newtonsoft typed `ReadAs*`/DateTime decoding, match executed AppSettings members (D33/Q33).
  - [x] Typed Newtonsoft decoding and loader fallbacks for app rules, legacy hard mutes and hidden app/device lists match the original loaders on hand-edited input (Q34); the canonical rule fixture also runs through these decoders.
  - [x] App-rule add/mutate/remove/clear, cache invalidation, raw JSON passthrough, legacy import merge, cloning, lazy migration writes and notification decisions match executed AppSettings lifecycle scenarios (D38/Q37). Current-culture sorting is injected; its native adapter remains open.
  - [x] Hidden app/device mutation, lookup/count/sorting, cloned results, save and notification decisions match original lifecycle scenarios (D39/Q38). Native culture comparison and observer delivery remain acceptance work in milestone 4.
  - [x] Typed Newtonsoft profile list/single-import decoding and normalization match the original loader and ImportProfile on noncanonical inputs, including nested object reuse, duplicate-array append and ApplyAppsOnly field order (D36/Q35).
  - [x] Dynamic Newtonsoft custom-theme import and saved-list decoding match ColorTheme.FromJson and LoadCustomThemes, including case-sensitive keys, explicit-null distinctions, scalar conversions and per-item versus whole-list failures (D37/Q36). Native store integration is accepted in milestone 4.
  - All sub-items done; only the native current-culture sort (D29) remains.
- [x] Port portable app identity/scoring, folder precedence/logical matching, focus snapshot/fade, window sizing and notification policies; compare linked original C# scenarios. Native full-path resolver and live polling remain explicit app/behavior acceptance items.
- [x] Model all W04 theme values, formats, opacity and old theme import defaults. Twelve palettes compared against actual .NET 8 exports; runtime theme application remains milestone 6.
- [x] Implement S14 undo/redo capacity, 500ms coalescing, no-op behavior, mute/replay suppression and stack invalidation. Includes backward-clock/id-only coalescing quirks and change revisions.
- [ ] Implement exact CLI tokenizer, normalization/aliases, batch splitting, rule grammar/matching and C01–C24 output types, preserving Q01–Q08.
  - [x] Portable rule assignments/flags/errors with supplied current-culture number signs, scored matching, stable ordering, first-wins planning, warnings and snapshot application match executed C# methods. CLI aliases use captured invariant/ordinal casing.
  - [x] Remaining success response DTOs and error envelope have source-expression golden schemas; sequential batch quoting/aggregation and Doctor raw counts/degraded state match original methods for canonical inputs.
  - [x] NumberStyles.Integer signs/whitespace/NUL/overflow, relative-volume classification boundary/wrapping clamp, raw-query app matching and save-name/flag validation match original methods across 19 culture/sign configurations.
  - [ ] Finish remaining portable command validation/resource-resolution models. Live execution/culture adapters are accepted in milestone 3.
  - `cli_validation.rs` covers names, flags and numbers; completeness against every C# validation branch is not confirmed.
- [x] Add valid golden CLI JSON fixtures derived from the captured C# expressions, including PascalCase rule DTOs, optional/null properties, errors and false-ok responses. These are model/serialization fixtures; native command acceptance remains in milestone 3.
  - [x] Original C# `rule-preview`, `rule-apply`, `resolve-apps` JSON plus mutated snapshots captured in `cli_rules_reference.json`; null/error/empty/overlap/device-scope cases covered.
  - [x] `cli_responses_reference.json` captures remaining success shapes, generic error escaping, batch false-ok/error-presence/raw fallback behavior and whole list/watch/default/volume/profile/Doctor projections. Noncanonical Newtonsoft parsing remains an open decoder gate above.
- [ ] ~~`cargo test -p bt-core` passes on Linux~~ Deferred by user decision D44 (Windows-only goal). Last Linux evidence: Ubuntu WSL2, 96 bt-core/bt-storage tests passing at the packaged-settings increment. Re-enable when Linux testing resumes.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 2 — audio engine

- [ ] A01 endpoint input/output enumeration, names/IDs/form factors/default roles/channels and notifications.
  - Outputs: full snapshot, names, IDs, form factor, default, change notifications. Inputs: listed by name and ID with default get/set per role only; no input volume, mute or sessions. No channel counts; no per-role (console vs communications) default flags in the snapshot.
- [ ] A02 sessions, grouping, app identities/icons, System Sounds, UWP/desktop/stale process cases and session callbacks.
  - System sounds, exited processes dropped and protected processes named from the process list (as DesktopAppInfo; checked on a user's PC), file-description names, exe icons. Grouping is by exe path (the session grouping parameter is ignored); no UWP/AUMID identity.
- [ ] A03 endpoint/session/channel volume/mute/peaks with proper callback ownership and device-removal recovery.
  - Device, app and per-process volume, mute and peaks. Missing: channel volumes, capture side, recovery when the Windows Audio service restarts (the enumerator is created once).
- [ ] A04 global defaults and role-specific get/set/verification.
  - Sets console and multimedia together (`set_default`), plus single roles; no read-back verification in bt-audio, and the CLI checks only the multimedia default.
- [ ] A05 app routing with correct downlevel/21H2 interfaces, endpoint encoding, console/multimedia roles and error behavior.
  - 21H2 and downlevel factories, endpoint ID packing, both roles. Render only; needs a live session (no process fallback).
- [ ] Thread-affine safe COM API, documented SAFETY blocks, deterministic callback revocation/apartment shutdown.
  - Dedicated MTA worker, SAFETY comments, Drop unregisters every callback and process wait. The worker holds its own job sender, so its loop never ends on its own; shutdown relies on process exit.
- [ ] Live-session example binary; Windows runtime scenarios and unrun tests recorded in MANUAL_TESTS.
  - `examples/sessions.rs` and `reassert_default.rs` exist; MANUAL_TESTS 3, 25, 26 not run.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 3 — IPC and CLI

- [ ] I01/I02 exact pipe name/session suffix, current-user ACL/owner checks, byte-mode LF/CR/EOF/UTF-8 framing, timeout and stop behavior.
  - Implemented, untested live (MANUAL_TESTS 36): Rust pipe `BetterTrumpet-Rust_IPC_v1_<session>` (deliberately not the original's name, so both apps can run at once), current-user ACL/owner check, line framing, stop; tests in `bt-app/tests/pipe.rs`. All 24 commands have handlers, including presets, update check and settings import/export. Only `bt ping` has been run live.
- [ ] C01–C10 ping/device/app/volume/mute/default/routing commands, absolute/relative volume and name matching.
  - C10's process fallback returns 0 (routing needs a session); numbers parse with invariant culture (D29 open); C09 does not verify the console role separately.
- [ ] C11–C14 presets/list/apply/save/delete/cycle, all capture flags/default slots and alias fallback.
  - Implemented with tests (`cli_presets.rs`); not run live.
- [ ] C15–C19 snapshot watch/update/settings import-export/doctor/batch with exact fields and error rules.
  - C16 (`check-update`) omits `lastChecked`, adds `checking` and does not wait up to 5 s for the result; C18's `deviceManagerReady` is always true.
- [ ] C20–C23 resolve-apps/rule preview/rule apply/preset create, ranking/warnings/PascalCase changes.
  - Implemented with C# reference fixtures (`cli_rule_compatibility.rs`); native ordering (MANUAL_TESTS 15) not run.
- [ ] C24 exact error messages, usage errors, parse/missing resources, unavailable settings/audio/updater handling.
  - Original messages throughout (test `errors_use_the_original_messages`); the unavailable-updater branch is missing.
- [ ] Populate all core response DTOs from live providers; verify native side effects, complete error branches, summary/timestamp formatting and transport/client presentation against C# (golden DTO round-trips alone are insufficient).
- [ ] Supply current-culture numeric signs and collation-aware StartsWith via D29; verify against .NET on supported Windows versions and MANUAL_TESTS 16. Invariant convenience parsing is insufficient for the installed dispatcher.
  - Missing: `cli_dispatch.rs` uses invariant signs and ordinal StartsWith; `win32/locale.rs` reads only the UI language.
- [ ] All volume/mute/mode/profile/preset/device/apps/update/settings aliases and all legacy flag prefixes work.
  - Aliases and flag prefixes implemented (`cli.rs`, `cli_cases.json` fixture); not run live.
- [ ] Direct exe help/version/no-server output, UTF-8/formatting/console startup, and bt.cmd discovery/redirect/exit behavior match.
  - `bt.exe` help, version and no-server text exist. No `bt.cmd`; `BetterTrumpet.exe` does not answer CLI arguments; `bt` with no arguments prints help instead of starting the app.
- [ ] Existing C# client→Rust server and Rust client→C# server cross-compatibility captured with Unicode/quotes/large responses.
  - Not possible as built: the pipe names differ (see I01). Needs a decision.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 4 — tray, flyout and mixer

- [ ] U01/U02 device sections/sliders/focus/channel details/routing/hide/solo, full mixer, placement and restore behavior.
  - Native mixer window (FullWindow columns, popups, sizing policy, FullMixerWindowPlacement), untested live (MANUAL_TESTS 42); popups clamped inside the window.
- [ ] T01 tray primary/secondary/tertiary/keyboard/hover/scroll, bounds, shell restart, loading/no-device/mute/light/dark/animation/badge/icon preferences.
  - Playback animation (event-driven, timer only while animating) and high-contrast legacy icons done (tests in `trayicon.rs`); loading state N/A (audio loads before the icon). Update badge still missing although the update check now exists (D55).
  - Implemented, untested (MANUAL_TESTS 37): middle click, scroll settings, dynamic icon (level/mute/light/legacy/device type/DPI).
- [ ] T02 complete right-click device/settings/hidden items/legacy panels/update/onboarding/feed/GitHub/exit menu.
  - Implemented, untested (MANUAL_TESTS 37): the original's items and order plus a presets submenu, the update item (first, when one is out; checked live, MANUAL_TESTS 46), Show onboarding and the project links (D54). No What's new item (no feed, D59).
- [ ] H01–H03 hotkeys, recording suspension, per-device/profile shortcuts, debounce, Ctrl+Z/Y/P and Ctrl+click.
  - Implemented, untested (MANUAL_TESTS 37, 38): preset/next/previous/per-device/settings/mixer hotkeys, recording suspension in the settings window.
  - No hotkey debounce found; a 350 ms repeat guard exists for preset cycling.
- [ ] U06 toast/dialog stacking and focus/keyboard/monitor/taskbar behavior.
  - The original's toasts (stacking, placement, timing); never take focus. Untested live (MANUAL_TESTS 43).
  - Toasts are not high contrast and not mirrored for RTL.
- [ ] U07 command palette reachability/disposition and behavior, including keyboard invocation/navigation if shipped.
  - Implemented, untested (MANUAL_TESTS 37): native command palette, Ctrl+Shift+P from the flyout.
  - Palette is not high contrast and not mirrored for RTL.
- [ ] BEHAVIOR_REVIEW row keys, middle-click hide, per-output solo toggle/unmute semantics, focused rule/routing/reset menus, slider wheel/touch/drag handoff/final commit and tray focus/re-add match.
  - Middle-click hide, solo and Ctrl+click exist; no touch (WM_POINTER) handling in sliders; none verified.
- [ ] Native backdrop/accessibility/high-contrast/RTL/DPI/multiple monitors, keyboard tab/focus and screen-reader semantics reviewed.
  - High contrast centralised in `ui/theme.rs` for tray, flyout, menus, tooltips, media card and settings; palette, onboarding, toasts and mixer pending. RTL mirrors the flyout, mixer, menus and tooltips only (not settings, palette, onboarding, toasts or media). Keyboard Tab focus in every window. No screen-reader support at all (no UI Automation or WM_GETOBJECT).
- [ ] Native Windows full-path resolver matches D25/context/error behavior and MANUAL_TESTS 14; pure logical matching tests alone do not satisfy this gate.
  - `win32/paths.rs` with a basic test; MANUAL_TESTS 14 not run.
- [ ] Integrate native settings selection and read-only first-run migration into the app, with persisted rule/hidden lists, unknown-field retention, safe destination writes and startup-setting/hotkey adapters; original stores remain unchanged.
  - [x] Installed (registry) mode wired into the app: first run archives HKCU\Software\EarTrumpet read-only, the app reads archive + overrides, writes only `settings.v1.json`; hidden app/device lists, app rules, IsExpanded, media-popup expansion and hotkeys use it. Portable and packaged startup selection are still open (see milestone 1).
- [ ] Warm flyout <150ms measured including first visible frame; document any alternative proposal before switching UI architecture.
  - Not measured; no timing code.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 5 — settings window

- [ ] W01 exact bridge adapter messages, state schema, localized labels, status and hotkey recording.
  - The web bridge no longer exists (D48); what remains applicable is localized labels, status and hotkey recording (test `hotkey_text_and_recording_match_the_original_format`). Reword for the native window.
- [ ] W02 all pages including General/Mouse, Shortcuts, QuickTrumpet, Rules/folders, Appearance, Media, Performance, Updates, Privacy, About/Diagnostics.
  - Native settings window (D48), untested live (MANUAL_TESTS 38): all eleven pages, grouped as the original's, including QuickTrumpet presets and App rules with folder rules. Privacy is a note only (no telemetry, D59); settings export/import sits on About rather than Privacy.
- [ ] W03 all mutations/actions/import/export/confirmations and value conversions.
  - Confirmations, preset import/export and value formatting exist (tests in `settings`); MANUAL_TESTS 41 not run.
- [ ] Sidebar collapse, static/dynamic deep search, loading/error, window chrome, placement and browser shortcut behavior.
  - Missing: no search, no collapse (fixed sidebar width) and no saved window placement.
- [ ] Classic settings entry point and any addon settings pages have explicit compatibility behavior; no dead openLegacy action.
  - The tray's classic item opens the same native window, so there is no dead action; not yet written down as a decision.
- [ ] Classic category/page backstack, navigation veto, close state and modern/classic handoff match BEHAVIOR_REVIEW.
  - Missing: no back navigation.
- [ ] Reuse existing React/Fluent UI assets/components; preserve W05 delivered styling.
  - Superseded by D48 (native UI); reword as "match the original's look".
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 6 — themes

- [ ] W04 all twelve exact palettes/five categories/seven channels and per-theme opacity/default opacity retention.
  - Flyout follows ActiveThemeName and saved themes; select/save/delete/randomize/reset in settings (no 300 ms transition, no import/export). Untested live (MANUAL_TESTS 38).
  - Palettes match the C# exports (test `all_twelve_palettes_match_actual_csharp_color_exports`).
- [ ] Theme selection/custom save/rename where present/delete/import/export/random/reset and legacy format defaults.
  - Missing in the UI: import/export (clipboard and file), rename and the 300 ms transition. Import parsing exists and is tested in bt-core.
- [ ] Album-art dynamic colors/cache/fallback/high contrast and every peak-meter style.
  - All five peak-meter styles are drawn. The dynamic album-art theme is missing: `UseDynamicAlbumArtTheme` is stored but nothing applies it (the media card's own accent does follow the art).
- [ ] D18 ColorTool classified as developer tooling or any discovered production dependency covered.
  - Not classified yet.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 7 — media popup

- [ ] M01 GSMTC discovery/session choice/metadata/art/timeline/change events and control capabilities.
  - Capabilities, art cache per track and session events exist. Session choice is only the current session (the original falls back to any playing one); no media/timeline property-change subscriptions; no tests in bt-media.
- [ ] U04 hover/hide timers/paused policy/expansion memory/blur/placement and source-app volume.
  - Hover delay, playing-only policy, expansion memory, blur (tested) and source-app volume exist; MANUAL_TESTS 24, 30, 31 not run.
- [ ] Play/pause/skip/seek+retry/shuffle/repeat and unavailable capability states.
  - Shuffle and repeat shown when the app supports them. Seek has a 2 s pending window; the original's 5 s window with retry is missing.
- [ ] M02 legacy media fallback, supported player detection, controls and cleanup.
  - LegacyMediaPlayerService port (player list, title cleanup, exe icon art, no-op transport, zero timeline, Q15), event-driven. Untested live (MANUAL_TESTS 44).
- [ ] Preserve original legacy no-op transport/seek, zero timeline, detection polling/title/icon behavior and GSMTC fallback result quirks; no unsupported capability claims.
  - No-op transport and zero timeline kept; detection is event-driven where the original polls.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 8 — rules, presets and behaviors

- [ ] B01 launch/lock/hard mute/folder defaults/pinned profiles/process identity and rule precedence.
  - Folder defaults implemented with the original's precedence, untested (MANUAL_TESTS 35).
  - Pinned neutral profiles missing; process tracking is by snapshot rather than a process watcher.
- [ ] S10 focus lost target/mute/fade/eligibility/per-app scope, snapshot restoration, exit/device/process failure handling.
  - Implemented with an EVENT_SYSTEM_FOREGROUND hook instead of the 250 ms poll, untested (MANUAL_TESTS 35).
- [ ] B02 capture/edit/rename/import/export/update/current/default slots/device/app/mute/routing/missing-resource/retry/results behavior.
  - Presets service implemented (capture/apply/edit/rename/delete/import/export, settle re-apply), untested (MANUAL_TESTS 37).
- [ ] Per-device/profile hotkey lifecycle and next/previous list order/wrap behavior.
  - Reload on change, 350 ms repeat guard and tested cycle order exist; MANUAL_TESTS 27, 39 not run.
- [ ] S07 eco/auto-battery rates, smooth/log volume, sounds and monkey unlock gate.
  - Eco Mode and AutoEcoMode done (D53), smooth volume and the tick sound done. **Logarithmic volume is missing**: the switch is stored but never applied. **Monkey sound is missing**: no unlock trigger and no sound files, so its switch does nothing.
- [ ] B03 shipped Actions triggers/conditions/actions/XML settings have a tested migration or explicitly agreed disposition; external MEF DLL limitation documented.
  - Missing: no Actions/MEF code. Needs a decision.
- [ ] Preserve Actions dev activation/schema/order/context-menu condition bypass and observed trigger/process/default-role quirks; demonstrate external-addon host compatibility under D22 before claiming parity.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 9 — onboarding

- [ ] U05 all five pages, Next/Back/Skip and close behavior, audio/theme/startup/privacy writes and consent confirmation.
  - Native onboarding, all five pages and writes, consent stored only; untested live (MANUAL_TESTS 43).
- [ ] L02 silent ordinary first launch, Left Ctrl forced startup, tray reentry, no destructive settings reset.
  - A first launch with no settings from EarTrumpet, BetterTrumpet or this app opens onboarding (D54, a deliberate change from Q09's silent launch); launches with carried-over settings stay silent. Left Ctrl and the tray item open it later (MANUAL_TESTS 43, 45).
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 10 — services and lifecycle

- [ ] N01 same GitHub release API/version channels/check delays/manual behavior, checksum/installer/portable update paths, Store suppression and first-run gate.
  - Replaced by D55, D56 and D58: this project's releases list (not `/latest`), off by default for carried-over settings, signed one-click update instead of the installer path, Windows notification. Checked live (MANUAL_TESTS 46, 47, 49). Differences to settle: no Store suppression (no Store build) and no silent-installer path. Reword this item.
- [ ] N02 same feed/version filtering/unread/polls/surveys/comparisons/votes/results/salted identity/retries/toggles; worker stays unchanged.
  - Not ported (D59).
- [ ] BEHAVIOR_REVIEW exact announcement bridge/feed/vote/results shapes, inclusive version filters, local-first votes, final HTTP failures and ephemeral retry queue validated with local fixtures.
  - Not ported (D59).
- [ ] N03/Q10/Q11 original enabled startup ping/identity/region defaults and explicit consent rules, with no extra collection.
  - Not ported (D59); only the consent key is kept so settings round-trip.
- [ ] N04 original Sentry consent/payload scope/flush/crash behavior and opt-out; network fixtures assert forbidden data absent.
  - Not ported (D59).
- [ ] F07/F08/N06 local rotated/circular logging, diagnostics review/sanitization/zip retention, crash exports and health thresholds.
  - Local rotated log and the diagnostic report (D49, tests in `log.rs` and `diagnostics.rs`). A panic is logged as FATAL, but no crash zip is written. N06 health monitoring (memory and handle thresholds) is missing.
- [ ] N05 external-link allowlist/navigation security and original user-opened links.
  - Links are constants in `links.rs` (D54) and update URLs are prefix-checked; there is no central open-URL guard. The original's changelog, privacy, discussions and fixfonts links were dropped without a recorded decision.
- [ ] L01/L02 identity/version/CLI interception/single instance/loading tray/audio/services startup and deterministic shutdown.
  - Port-only mutex and a second launch shows the flyout; quit and tray removal are clean. Missing: CLI interception in `BetterTrumpet.exe`, WM_ENDSESSION handling.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 11 — localization

- [ ] R01 all existing RESX keys/translations/placeholders and culture fallback loaded in Rust/web UI.
  - Native UI localized with the original's tables, fallback tested (`i18n.rs`, 7 tests, including that every key used in code exists; untested live, MANUAL_TESTS 40); RTL mirrors the flyout, mixer, menus and tooltips, not the settings window or palette.
- [ ] Crowdin import/export workflow retained with repeatable generation; no missing dynamic labels.
  - Missing: no crowdin config; the port's own strings (`Native.resx`) are English only.
- [ ] RTL/high contrast/localized health/colon stripping and keyboard labels match.
  - No colon stripping found; no localized health display (no N06).
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 12 — packaging, CI and release

- [ ] R02 Windows CI fmt, clippy -D warnings, tests, x64+arm64 release builds, portable zip and installer.
  - CI runs fmt, clippy -D warnings, tests and an MSRV build; the release builds the zip, setup, signed update files and a VirusTotal report. x64 only: no arm64 build.
- [ ] GitVersion-equivalent version numbers and exe/CLI/installer/manifests/release asset consistency.
  - The release refuses a tag that differs from the workspace version; exe, CLI and installer take it from Cargo.
- [ ] Update installer.iss, winget and Chocolatey manifests for new targets; startup/PATH/uninstall/wrapper flows.
  - Own `packaging/installer.iss` (D57) with startup, PATH and uninstall, checked live (MANUAL_TESTS 48). No winget or Chocolatey manifests yet.
- [ ] Store/package identity behavior preserved or explicit agreed delivery disposition; current minimum OS conflict resolved through real OS checks.
  - No Store build; the installer requires Windows 10 1803 (10.0.17134). Needs a delivery decision and a real-OS check.
- [ ] R03 entire LICENSE/attribution present in distribution, README and About.
  - LICENSE ships in the zip and the setup; README credits the original. About shows a one-line credit, not the full licence.
- [x] Include rust/THIRD_PARTY_NOTICES.md and required dependency notices in release artifacts. `THIRD_PARTY_NOTICES.md` is copied into the zip (release.yml) and installed by the setup (installer.iss).
- [ ] MANUAL_TESTS covers all unautomated scenarios with actual run results or clearly marked unrun status.
  - Entries 46–49 record live runs; the rest are unrun.
- [ ] Migration regression on copies of installed/portable/packaged settings, profiles and real CLI scripts; legacy originals unchanged.
- [ ] Release binary/install size, total-process idle RAM/CPU and warm flyout latency measured against C# baseline on same machine/workload.
- [ ] CI green for x64 and arm64; full parity checklist reviewed, no silently omitted items.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

The only approved "won't port" disposition is D59 (telemetry, Sentry, announcement feed and votes). Documentation/extraction checks are not implementation tests or completed milestone checks.
