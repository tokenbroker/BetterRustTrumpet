# BetterTrumpet Rust parity checklist

This file is the source of truth for completion. An unchecked item is incomplete, even if code or a prototype exists. Inventory IDs refer to [INVENTORY.md](INVENTORY.md), including its source appendices. No Rust milestone is complete. Runtime tests and performance measurements have not been run as part of Phase 0.

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

In progress: workspace/bt-core, CLI/protocol/response DTOs, batch/Doctor aggregation, undo/redo, scalar settings, persisted DTOs, profile normalization/lookup/cycle/import/edit logic, rule plans and theme palettes/import format are implemented. Actual-C# model/service-method/response golden fixtures and Windows/Linux tests pass. Complete settings/migration/rule lifecycle, Newtonsoft noncanonical input conversions and portable CLI validation remain open; no milestone completion is claimed.

- [ ] Implement D01–D04 workspace and portable bt-core; no unsafe/Windows/Tauri dependencies in core.
- [ ] Model every S01–S14 setting, default, clamp, enum, computed key, hotkey representation and notification-relevant change.
  - [x] Decoded scalar settings/defaults, eight hotkeys, consent-key presence, lazy vote ID, device/poll keys, battery eco/FPS helpers, read/write clamps and notification/batch behavior match executed C# property fixtures.
  - [ ] Complete color/placement values, raw JSON collection setter lifecycles, addon/local-variable namespace models, native-boundary command/result models and legacy decoding. Native registration/storage/startup execution is accepted in milestones 4/9/10.
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
- [ ] Model schema 1/2/3 profiles, default roles, IncludeVolume/RouteApps/ApplyAppsOnly and migration quirks (S13/B02).
- [ ] Model app/folder rules, hidden app/device identity and pinned neutral rules (S02–S04).
  - [x] App-rule/hidden-list normalization and parsed legacy hard-mute fallback match executed C# fixtures: first duplicate wins, pinned neutral entries, unknown modes, clamps, invariant lowercase and delimiter collisions.
  - [x] Folder-rule load/normalization/ID and time repair, ordering, serialization and add/update/remove/set/match lifecycle, including Newtonsoft typed `ReadAs*`/DateTime decoding, match executed AppSettings members (D33/Q33).
  - [x] Typed Newtonsoft decoding and loader fallbacks for app rules, legacy hard mutes and hidden app/device lists match the original loaders on hand-edited input (Q34); the canonical rule fixture also runs through these decoders.
  - [x] App-rule add/mutate/remove/clear, cache invalidation, raw JSON passthrough, legacy import merge, cloning, lazy migration writes and notification decisions match executed AppSettings lifecycle scenarios (D38/Q37). Current-culture sorting is injected; its native adapter remains open.
  - [x] Hidden app/device mutation, lookup/count/sorting, cloned results, save and notification decisions match original lifecycle scenarios (D39/Q38). Native culture comparison and observer delivery remain acceptance work in milestone 4.
  - [x] Typed Newtonsoft profile list/single-import decoding and normalization match the original loader and ImportProfile on noncanonical inputs, including nested object reuse, duplicate-array append and ApplyAppsOnly field order (D36/Q35).
  - [x] Dynamic Newtonsoft custom-theme import and saved-list decoding match ColorTheme.FromJson and LoadCustomThemes, including case-sensitive keys, explicit-null distinctions, scalar conversions and per-item versus whole-list failures (D37/Q36). Native store integration is accepted in milestone 4.
- [x] Port portable app identity/scoring, folder precedence/logical matching, focus snapshot/fade, window sizing and notification policies; compare linked original C# scenarios. Native full-path resolver and live polling remain explicit app/behavior acceptance items.
- [x] Model all W04 theme values, formats, opacity and old theme import defaults. Twelve palettes compared against actual .NET 8 exports; runtime theme application remains milestone 6.
- [x] Implement S14 undo/redo capacity, 500ms coalescing, no-op behavior, mute/replay suppression and stack invalidation. Includes backward-clock/id-only coalescing quirks and change revisions.
- [ ] Implement exact CLI tokenizer, normalization/aliases, batch splitting, rule grammar/matching and C01–C24 output types, preserving Q01–Q08.
  - [x] Portable rule assignments/flags/errors with supplied current-culture number signs, scored matching, stable ordering, first-wins planning, warnings and snapshot application match executed C# methods. CLI aliases use captured invariant/ordinal casing.
  - [x] Remaining success response DTOs and error envelope have source-expression golden schemas; sequential batch quoting/aggregation and Doctor raw counts/degraded state match original methods for canonical inputs.
  - [x] NumberStyles.Integer signs/whitespace/NUL/overflow, relative-volume classification boundary/wrapping clamp, raw-query app matching and save-name/flag validation match original methods across 19 culture/sign configurations.
  - [ ] Finish remaining portable command validation/resource-resolution models. Live execution/culture adapters are accepted in milestone 3.
- [x] Add valid golden CLI JSON fixtures derived from the captured C# expressions, including PascalCase rule DTOs, optional/null properties, errors and false-ok responses. These are model/serialization fixtures; native command acceptance remains in milestone 3.
  - [x] Original C# `rule-preview`, `rule-apply`, `resolve-apps` JSON plus mutated snapshots captured in `cli_rules_reference.json`; null/error/empty/overlap/device-scope cases covered.
  - [x] `cli_responses_reference.json` captures remaining success shapes, generic error escaping, batch false-ok/error-presence/raw fallback behavior and whole list/watch/default/volume/profile/Doctor projections. Noncanonical Newtonsoft parsing remains an open decoder gate above.
- [ ] ~~`cargo test -p bt-core` passes on Linux~~ Deferred by user decision D44 (Windows-only goal). Last Linux evidence: Ubuntu WSL2, 96 bt-core/bt-storage tests passing at the packaged-settings increment. Re-enable when Linux testing resumes.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 2 — audio engine

- [ ] A01 endpoint input/output enumeration, names/IDs/form factors/default roles/channels and notifications.
- [ ] A02 sessions, grouping, app identities/icons, System Sounds, UWP/desktop/stale process cases and session callbacks.
- [ ] A03 endpoint/session/channel volume/mute/peaks with proper callback ownership and device-removal recovery.
- [ ] A04 global defaults and role-specific get/set/verification.
- [ ] A05 app routing with correct downlevel/21H2 interfaces, endpoint encoding, console/multimedia roles and error behavior.
- [ ] Thread-affine safe COM API, documented SAFETY blocks, deterministic callback revocation/apartment shutdown.
- [ ] Live-session example binary; Windows runtime scenarios and unrun tests recorded in MANUAL_TESTS.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 3 — IPC and CLI

- [ ] I01/I02 exact pipe name/session suffix, current-user ACL/owner checks, byte-mode LF/CR/EOF/UTF-8 framing, timeout and stop behavior.
  - Implemented, untested live (MANUAL_TESTS 36): Rust pipe `BetterTrumpet-Rust_IPC_v1_<session>`, current-user ACL/owner check, line framing, stop. C15/C18-C22 live; C01-C10 partial (invariant number parsing, session-only routing, single default flag); presets, update check and settings import/export pending.
- [ ] C01–C10 ping/device/app/volume/mute/default/routing commands, absolute/relative volume and name matching.
- [ ] C11–C14 presets/list/apply/save/delete/cycle, all capture flags/default slots and alias fallback.
- [ ] C15–C19 snapshot watch/update/settings import-export/doctor/batch with exact fields and error rules.
- [ ] C20–C23 resolve-apps/rule preview/rule apply/preset create, ranking/warnings/PascalCase changes.
- [ ] C24 exact error messages, usage errors, parse/missing resources, unavailable settings/audio/updater handling.
- [ ] Populate all core response DTOs from live providers; verify native side effects, complete error branches, summary/timestamp formatting and transport/client presentation against C# (golden DTO round-trips alone are insufficient).
- [ ] Supply current-culture numeric signs and collation-aware StartsWith via D29; verify against .NET on supported Windows versions and MANUAL_TESTS 16. Invariant convenience parsing is insufficient for the installed dispatcher.
- [ ] All volume/mute/mode/profile/preset/device/apps/update/settings aliases and all legacy flag prefixes work.
- [ ] Direct exe help/version/no-server output, UTF-8/formatting/console startup, and bt.cmd discovery/redirect/exit behavior match.
- [ ] Existing C# client→Rust server and Rust client→C# server cross-compatibility captured with Unicode/quotes/large responses.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 4 — tray, flyout and mixer

- [ ] U01/U02 device sections/sliders/focus/channel details/routing/hide/solo, full mixer, placement and restore behavior.
  - Native mixer window (FullWindow columns, popups, sizing policy, FullMixerWindowPlacement), untested live (MANUAL_TESTS 42); popups clamped inside the window.
- [ ] T01 tray primary/secondary/tertiary/keyboard/hover/scroll, bounds, shell restart, loading/no-device/mute/light/dark/animation/badge/icon preferences.
  - Playback animation (event-driven, timer only while animating) and high-contrast legacy icons done; loading state N/A (audio loads before the icon); update badge not ported (network-only trigger).
  - Implemented, untested (MANUAL_TESTS 37): middle click, scroll settings, dynamic icon (level/mute/light/legacy/device type/DPI). Animation, high contrast and badge remain.
- [ ] T02 complete right-click device/settings/hidden items/legacy panels/update/onboarding/feed/GitHub/exit menu.
  - Implemented, untested (MANUAL_TESTS 37): the original's items and order plus a presets submenu; update/onboarding/what's-new items omitted (no services).
- [ ] H01–H03 hotkeys, recording suspension, per-device/profile shortcuts, debounce, Ctrl+Z/Y/P and Ctrl+click.
  - Implemented, untested (MANUAL_TESTS 37, 38): preset/next/previous/per-device/settings/mixer hotkeys, recording suspension in the settings window.
- [ ] U06 toast/dialog stacking and focus/keyboard/monitor/taskbar behavior.
  - The original's toasts (stacking, placement, timing); never take focus. Untested live (MANUAL_TESTS 43).
- [ ] U07 command palette reachability/disposition and behavior, including keyboard invocation/navigation if shipped.
  - Implemented, untested (MANUAL_TESTS 37): native command palette, Ctrl+Shift+P from the flyout.
- [ ] BEHAVIOR_REVIEW row keys, middle-click hide, per-output solo toggle/unmute semantics, focused rule/routing/reset menus, slider wheel/touch/drag handoff/final commit and tray focus/re-add match.
- [ ] Native backdrop/accessibility/high-contrast/RTL/DPI/multiple monitors, keyboard tab/focus and screen-reader semantics reviewed.
  - High contrast centralised in `ui/theme.rs` for tray, flyout, menus, tooltips, media card and settings; palette, onboarding, toasts and mixer pending. No screen-reader support yet.
- [ ] Native Windows full-path resolver matches D25/context/error behavior and MANUAL_TESTS 14; pure logical matching tests alone do not satisfy this gate.
- [ ] Integrate native settings selection and read-only first-run migration into the app, with persisted rule/hidden lists, unknown-field retention, safe destination writes and startup-setting/hotkey adapters; original stores remain unchanged.
  - [x] Installed (registry) mode wired into the app: first run archives HKCU\Software\EarTrumpet read-only, the app reads archive + overrides, writes only `settings.v1.json`; hidden app/device lists, app rules, IsExpanded, media-popup expansion and hotkeys use it. Portable and packaged startup selection are still open.
- [ ] Warm flyout <150ms measured including first visible frame; document any alternative proposal before switching UI architecture.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 5 — settings window

- [ ] W01 exact bridge adapter messages, state schema, localized labels, status and hotkey recording.
- [ ] W02 all pages including General/Mouse, Shortcuts, QuickTrumpet, Rules/folders, Appearance, Media, Performance, Updates, Privacy, About/Diagnostics.
  - Native settings window (D48), untested live (MANUAL_TESTS 38): General, Volume & mouse, Shortcuts, Media popup, Appearance, Performance, Updates, Privacy, About; QuickTrumpet presets and App rules pages in progress.
- [ ] W03 all mutations/actions/import/export/confirmations and value conversions.
- [ ] Sidebar collapse, static/dynamic deep search, loading/error, window chrome, placement and browser shortcut behavior.
- [ ] Classic settings entry point and any addon settings pages have explicit compatibility behavior; no dead openLegacy action.
- [ ] Classic category/page backstack, navigation veto, close state and modern/classic handoff match BEHAVIOR_REVIEW.
- [ ] Reuse existing React/Fluent UI assets/components; preserve W05 delivered styling.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 6 — themes

- [ ] W04 all twelve exact palettes/five categories/seven channels and per-theme opacity/default opacity retention.
  - Flyout follows ActiveThemeName and saved themes; select/save/delete/randomize/reset in settings (no 300 ms transition, no import/export). Untested live (MANUAL_TESTS 38).
- [ ] Theme selection/custom save/rename where present/delete/import/export/random/reset and legacy format defaults.
- [ ] Album-art dynamic colors/cache/fallback/high contrast and every peak-meter style.
- [ ] D18 ColorTool classified as developer tooling or any discovered production dependency covered.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 7 — media popup

- [ ] M01 GSMTC discovery/session choice/metadata/art/timeline/change events and control capabilities.
- [ ] U04 hover/hide timers/paused policy/expansion memory/blur/placement and source-app volume.
- [ ] Play/pause/skip/seek+retry/shuffle/repeat and unavailable capability states.
- [ ] M02 legacy media fallback, supported player detection, controls and cleanup.
  - LegacyMediaPlayerService port (player list, title cleanup, exe icon art, no-op transport, zero timeline, Q15), event-driven. Untested live (MANUAL_TESTS 44).
- [ ] Preserve original legacy no-op transport/seek, zero timeline, detection polling/title/icon behavior and GSMTC fallback result quirks; no unsupported capability claims.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 8 — rules, presets and behaviors

- [ ] B01 launch/lock/hard mute/folder defaults/pinned profiles/process identity and rule precedence.
  - Folder defaults implemented with the original's precedence, untested (MANUAL_TESTS 35).
- [ ] S10 focus lost target/mute/fade/eligibility/per-app scope, snapshot restoration, exit/device/process failure handling.
  - Implemented with an EVENT_SYSTEM_FOREGROUND hook instead of the 250 ms poll, untested (MANUAL_TESTS 35).
- [ ] B02 capture/edit/rename/import/export/update/current/default slots/device/app/mute/routing/missing-resource/retry/results behavior.
  - Presets service implemented (capture/apply/edit/rename/delete/import/export, settle re-apply), untested (MANUAL_TESTS 37).
- [ ] Per-device/profile hotkey lifecycle and next/previous list order/wrap behavior.
- [ ] S07 eco/auto-battery rates, smooth/log volume, sounds and monkey unlock gate.
- [ ] B03 shipped Actions triggers/conditions/actions/XML settings have a tested migration or explicitly agreed disposition; external MEF DLL limitation documented.
- [ ] Preserve Actions dev activation/schema/order/context-menu condition bypass and observed trigger/process/default-role quirks; demonstrate external-addon host compatibility under D22 before claiming parity.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 9 — onboarding

- [ ] U05 all five pages, Next/Back/Skip and close behavior, audio/theme/startup/privacy writes and consent confirmation.
  - Native onboarding, all five pages and writes, consent stored only; untested live (MANUAL_TESTS 43).
- [ ] L02 silent ordinary first launch, Left Ctrl forced startup, tray reentry, no destructive settings reset.
  - Ordinary launch silently sets hasShownFirstRun; Left Ctrl and the tray item open onboarding (MANUAL_TESTS 43).
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 10 — services and lifecycle

- [ ] N01 same GitHub release API/version channels/check delays/manual behavior, checksum/installer/portable update paths, Store suppression and first-run gate.
- [ ] N02 same feed/version filtering/unread/polls/surveys/comparisons/votes/results/salted identity/retries/toggles; worker stays unchanged.
- [ ] BEHAVIOR_REVIEW exact announcement bridge/feed/vote/results shapes, inclusive version filters, local-first votes, final HTTP failures and ephemeral retry queue validated with local fixtures.
- [ ] N03/Q10/Q11 original enabled startup ping/identity/region defaults and explicit consent rules, with no extra collection.
- [ ] N04 original Sentry consent/payload scope/flush/crash behavior and opt-out; network fixtures assert forbidden data absent.
- [ ] F07/F08/N06 local rotated/circular logging, diagnostics review/sanitization/zip retention, crash exports and health thresholds.
  - Local rotated log and the diagnostic report (D49). N06 health thresholds and crash exports remain.
- [ ] N05 external-link allowlist/navigation security and original user-opened links.
- [ ] L01/L02 identity/version/CLI interception/single instance/loading tray/audio/services startup and deterministic shutdown.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 11 — localization

- [ ] R01 all existing RESX keys/translations/placeholders and culture fallback loaded in Rust/web UI.
  - Native UI localized with the original's tables (untested live, MANUAL_TESTS 40); RTL mirrors the flyout, menus and tooltips, not the settings window or palette.
- [ ] Crowdin import/export workflow retained with repeatable generation; no missing dynamic labels.
- [ ] RTL/high contrast/localized health/colon stripping and keyboard labels match.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

## Milestone 12 — packaging, CI and release

- [ ] R02 Windows CI fmt, clippy -D warnings, tests, x64+arm64 release builds, portable zip and installer.
- [ ] GitVersion-equivalent version numbers and exe/CLI/installer/manifests/release asset consistency.
- [ ] Update installer.iss, winget and Chocolatey manifests for new targets; startup/PATH/uninstall/wrapper flows.
- [ ] Store/package identity behavior preserved or explicit agreed delivery disposition; current minimum OS conflict resolved through real OS checks.
- [ ] R03 entire LICENSE/attribution present in distribution, README and About.
- [ ] Include rust/THIRD_PARTY_NOTICES.md and required dependency notices in release artifacts.
- [ ] MANUAL_TESTS covers all unautomated scenarios with actual run results or clearly marked unrun status.
- [ ] Migration regression on copies of installed/portable/packaged settings, profiles and real CLI scripts; legacy originals unchanged.
- [ ] Release binary/install size, total-process idle RAM/CPU and warm flyout latency measured against C# baseline on same machine/workload.
- [ ] CI green for x64 and arm64; full parity checklist reviewed, no silently omitted items.
- [ ] Build/tests pass, PARITY updated, focused milestone commit and PROGRESS entry created.

No “won't port” dispositions have been approved. Documentation/extraction checks are not implementation tests or completed milestone checks.
