# Menus + Keybinding Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver production-ready WiFi, Mount/SSHFS/Loop/VCVolume, and CalDAV/Calendar menus, plus a native keybinding engine that supports press/release/modrelease and a migration path from `sxhkd`.

**Architecture:** Extend existing `unilii/bin` menu/tray flow with dedicated menu controllers and snapshot providers. Move keybinding semantics into a deterministic engine in `unilii/core`, keeping execution and parsing isolated. Add calendar provider/cache support in `unilii/lib` and wire all menus through shared async action infrastructure.

**Tech Stack:** Rust, Iced, Tokio, serde/TOML, tracing, nmcli, lsblk/udisksctl/sshfs/veracrypt command backends, SQLite (calendar cache)

---

## 0. Scope Split and Delivery Tracks

Work is split into tracks that can run in parallel after foundation:

- Track A: Shared menu/action infrastructure
- Track B: WiFi full menu
- Track C: Mount/SSHFS/Loop/VCVolume full menu
- Track D: Calendar/CalDAV menu
- Track E: Keybinding engine + `sxhkd` migration tooling
- Track F: i3 menu + i3 internal actions (`i3-msg`)
- Track G: tmux menu + tmux internal actions (`tmux` CLI/lib adapter)

Release gating order:

1. Foundation + Key engine hardening
2. WiFi
3. Mount stack
4. Calendar read-only
5. Calendar write + migration tooling default-on
6. i3 first-class menu + hotkey actions
7. tmux first-class menu + hotkey actions

## 1. File Map (Planned)

Create/modify these paths.

- Modify: `unilii/bin/src/main.rs`
- Modify: `unilii/bin/src/app.rs`
- Create: `unilii/bin/src/menus/mod.rs`
- Create: `unilii/bin/src/menus/common.rs`
- Create: `unilii/bin/src/menus/wifi.rs`
- Create: `unilii/bin/src/menus/mount.rs`
- Create: `unilii/bin/src/menus/calendar.rs`
- Create: `unilii/bin/src/menus/custom.rs`
- Create: `unilii/bin/src/menus/i3.rs`
- Create: `unilii/bin/src/menus/tmux.rs`
- Create: `unilii/bin/src/menus/types.rs`
- Create: `unilii/bin/src/menus/tests/*.rs`
- Modify: `unilii/bin/src/enhanced_tray/state.rs`
- Modify: `unilii/bin/src/enhanced_tray/rendering.rs`
- Create: `unilii/bin/src/action_runner.rs`

- Create: `unilii/core/src/key_engine.rs`
- Create: `unilii/core/src/key_import_sxhkd.rs`
- Modify: `unilii/core/src/keys.rs`
- Modify: `unilii/core/src/config.rs`
- Modify: `unilii/core/src/lib.rs`
- Create: `unilii/core/tests/key_engine_tests.rs`
- Create: `unilii/core/tests/sxhkd_import_tests.rs`

- Create: `unilii/lib/src/calendar/mod.rs`
- Create: `unilii/lib/src/calendar/provider.rs`
- Create: `unilii/lib/src/calendar/caldav.rs`
- Create: `unilii/lib/src/calendar/cache.rs`
- Create: `unilii/lib/src/i3/mod.rs`
- Create: `unilii/lib/src/i3/client.rs`
- Create: `unilii/lib/src/tmux/mod.rs`
- Create: `unilii/lib/src/tmux/client.rs`
- Create: `unilii/lib/tests/calendar_cache_tests.rs`
- Create: `unilii/lib/tests/i3_client_tests.rs`
- Create: `unilii/lib/tests/tmux_client_tests.rs`
- Modify: `unilii/lib/src/lib.rs`

- Modify: `unilii/core/Cargo.toml`
- Modify: `unilii/bin/Cargo.toml`
- Modify: `unilii/lib/Cargo.toml`
- Modify: `README.md`
- Create: `unilii/docs/plans/2026-04-16-menus-keybinding-rollout-checklist.md`

## 2. Milestone Plan

### Milestone M0: Foundation (Shared Infrastructure)

**Outcome:** Shared menu state model and action runner are in place, without changing user-visible behavior yet.

- [x] Task M0.1: Add shared menu lifecycle/state types (`Closed/Opening/Ready/Busy/Error/Stale`) in `menus/types.rs`.
- [x] Task M0.2: Add `MenuController` and `SnapshotProvider` traits in `menus/common.rs`.
- [x] Task M0.3: Add async `ActionRunner` with timeout/cancel/structured result in `action_runner.rs`.
- [x] Task M0.4: Integrate action runner into existing tray code paths (network toggle/refresh) without behavior regressions.
- [x] Task M0.5: Add logging schema for action audit (`menu`, `action`, `duration_ms`, `exit_code`, `error_class`).
- [x] Task M0.6: Add unit tests for timeout, cancellation, and stderr capture.
- [x] Task M0.7: Add shared low-level interfaces `FilterableMenu` and `QuickjumpMenu`.
- [x] Task M0.8: Add deterministic quickjump label generator (single-char, then two-char labels).

**Verification:**

- `cargo test -p unilii-bin action_runner`
- `cargo test -p unilii-bin enhanced_tray`
- `cargo check`

**Exit criteria:** Existing enhanced tray behavior remains functional; action runner used by at least one existing action path.

### Milestone M8: Custom Menu + Includeable TOML Sources

**Outcome:** Users can define script/launcher menus with icon metadata and include multiple TOML files.

- [x] Task M8.1: Add `[menus.custom]` config model with inline `items`, `include`, and `sources`.
- [x] Task M8.2: Add include/source merge resolver with path/glob support and cycle suppression.
- [x] Task M8.3: Add custom menu snapshot/model in `unilii/bin/src/menus/custom.rs`.
- [x] Task M8.4: Add launcher and shell action command generation paths.
- [x] Task M8.5: Wire custom menu runtime activation by app-id/icon pattern matching.
- [ ] Task M8.6: Render item-specific SVG/PNG assets in list rows (fallback to icon text/theme icon).
- [ ] Task M8.7: Add docs/examples for xrandr profile launcher bundles via include files.
- [ ] Task M8.8: Add per-menu filter text input widget contract (`f`/`/` focus, incremental query, clear semantics) for all `FilterableMenu` implementers.
- [ ] Task M8.9: Add `choose_from_stdin` action variant (`producer -> chooser -> consumer`) with cancellation-aware behavior.
- [ ] Task M8.10: Add optional Unix socket transport adapter for chooser daemons with mandatory one-shot fallback.

**Verification:**

- `cargo test -p unilii-core config`
- `cargo test -p unilii-bin custom`
- `cargo test -p unilii-bin menus::common`
- Manual: open tray icon bound to `[menus.custom]`, filter items, trigger script/launcher commands.

**Exit criteria:** includeable custom menus load deterministically and execute expected launcher/script actions.

### Milestone M1: Key Engine Semantics

**Outcome:** Deterministic press/release/modrelease behavior with conflict resolution and diagnostics.

- [x] Task M1.1: Introduce key trigger enums and config parsing (`press`, `release`, `modrelease`, optional `repeat`).
- [x] Task M1.2: Implement `KeyEngine` state machine in `unilii/core/src/key_engine.rs`.
- [x] Task M1.3: Port daemon runtime in `keys.rs` to consume `KeyEngine` decisions.
- [x] Task M1.4: Add priority + most-specific conflict resolution and consume policy.
- [x] Task M1.5: Add hold threshold (`hold_ms`) and cooldown/debounce support.
- [x] Task M1.6: Add debug trace hooks for trigger reasoning (`matched`, `suppressed`, `invalidated`).
- [x] Task M1.7: Add compatibility parser/importer from `sxhkdrc` to TOML in `key_import_sxhkd.rs`.
- [x] Task M1.8: Add dry-run CLI mode for key simulation and conflict reports.

**Verification:**

- `cargo test -p unilii-core key_engine_tests`
- `cargo test -p unilii-core sxhkd_import_tests`
- `cargo test -p unilii-core keys`

**Exit criteria:** test matrix covers overlap/tap-hold/release invalidation cases; parser handles common `sxhkd` patterns with explicit warnings for unsupported patterns.

### Milestone M2: WiFi Menu Full Spec Implementation

**Outcome:** Full WiFi control from menu with robust error states and keyboard navigation.

- [x] Task M2.1: Extract existing network snapshot logic into `menus/wifi.rs` provider/controller.
- [x] Task M2.2: Implement full section model (status header, controls, available networks, known networks, diagnostics).
- [ ] Task M2.3: Add secure network credential prompt flow (modal event + backend command invocation).
- [ ] Task M2.4: Add forget-network action and confirmation guard.
- [ ] Task M2.5: Add per-row error anchoring for failed connect/disconnect.
- [ ] Task M2.6: Add keyboard model (`Tab`, `Enter`, arrows, `j/k`, `f`, `r`, numeric actions).
- [x] Task M2.7: Add config parsing under `[menus.wifi]` and defaults.
- [x] Task M2.8: Add tests for parse, sort order, throttling, and fallback states.

**Verification:**

- `cargo test -p unilii-bin wifi`
- `cargo test -p unilii-bin enhanced_tray`
- Manual: open network menu, toggle wifi, connect/disconnect, refresh, settings launch.

**Exit criteria:** all spec-required WiFi actions available and stable in degraded backend states.

### Milestone M3: Mount/SSHFS/Loop/VCVolume Menu

**Outcome:** Unified storage menu supports local devices, sshfs profiles, loop images, and encrypted vcvolume profiles.

- [x] Task M3.1: Implement mount domain model and state transitions (`unmounted/mounting/mounted/error/stale`).
- [x] Task M3.2: Implement local device snapshot provider (lsblk + mount table reconciliation).
- [ ] Task M3.3: Implement SSHFS profile model and mount/unmount orchestration.
- [ ] Task M3.4: Implement loop attach/detach orchestration and read-only default.
- [ ] Task M3.5: Implement vcvolume backend command templates and profile handling.
- [ ] Task M3.6: Add busy-unmount diagnostics and retry guidance.
- [x] Task M3.7: Add menu rendering sections and keyboard handling parity.
- [x] Task M3.8: Add config parsing under `[menus.mount]`, `sshfs_profiles`, `vcvolume_profiles`.
- [ ] Task M3.9: Add integration tests for snapshot parsing and orchestration failure classes.

**Verification:**

- `cargo test -p unilii-bin mount`
- Manual: mount/unmount removable disk, sshfs profile connect/disconnect, loop image attach/detach, vcvolume unlock/lock.

**Exit criteria:** deterministic state reconciliation after partial failures; no stuck "mounting" states.

### Milestone M4: Calendar/CalDAV Menu (Read-first)

**Outcome:** Cached agenda + reminders from CalDAV accounts with secure credentials and offline fallback.

- [x] Task M4.1: Add calendar module skeleton in `unilii/lib/src/calendar` with provider trait.
- [ ] Task M4.2: Implement cache schema and sync token state in SQLite-backed cache layer.
- [ ] Task M4.3: Implement CalDAV read sync for events in configurable window.
- [ ] Task M4.4: Add reminder scheduler with dedup key and snooze/dismiss persistence.
- [ ] Task M4.5: Implement menu controller/rendering for today/next + agenda sections.
- [ ] Task M4.6: Add account config parsing under `[menus.calendar.accounts]` with keyring auth.
- [ ] Task M4.7: Add stale/offline/reauth UI states.
- [ ] Task M4.8: Add tests for cache conflict behavior and reminder dedup.

**Verification:**

- `cargo test -p unilii-lib calendar`
- `cargo test -p unilii-bin calendar`
- Manual: account bootstrap, sync, offline display, reminder fire + snooze.

**Exit criteria:** read path and reminders are reliable across app restarts and transient network errors.

### Milestone M5: Calendar Write Flows + Migration Tooling + Rollout

**Outcome:** Event create/edit/delete support and practical migration away from `sxhkd`.

- [ ] Task M5.1: Add create/edit/delete event operations in provider API.
- [ ] Task M5.2: Add write modal in menu and recurrence-exception baseline handling.
- [ ] Task M5.3: Add conflict presentation policy (server/local preserve + user decision).
- [ ] Task M5.4: Finalize `sxhkd` import CLI UX (`import`, `lint`, `dry-run`, `apply`).
- [ ] Task M5.5: Add shadow mode for daemon parity checks before cutover.
- [ ] Task M5.6: Add migration report and rollback command docs.
- [ ] Task M5.7: Produce rollout checklist document and operator playbook.

**Verification:**

- `cargo test`
- Manual: write operations against test CalDAV server; keybinding shadow mode soak test.

**Exit criteria:** users can migrate keybindings with rollback; calendar writes are safe and auditable.

### Milestone M6: i3 Menu + Internal Actions

**Outcome:** First-class i3 workspace/window switching via menu and keybinding internal actions.

- [ ] Task M6.1: Add i3 snapshot provider (`get_workspaces`, `get_tree`, `get_outputs`) in `unilii/lib/src/i3/client.rs`.
- [ ] Task M6.2: Add i3 menu controller/rendering in `unilii/bin/src/menus/i3.rs`.
- [ ] Task M6.3: Add i3 actions (`workspace switch`, `focus con_id`, `move container`) with optimistic UI + reconciliation.
- [ ] Task M6.4: Add internal key actions for i3 in keybinding command handling (`bar:i3:*` namespace).
- [ ] Task M6.5: Add config parsing under `[menus.i3]` and defaults.
- [ ] Task M6.6: Add tests for i3 JSON parsing and action command orchestration.

**Verification:**

- `cargo test -p unilii-lib i3_client_tests`
- `cargo test -p unilii-bin i3`
- Manual: workspace switch, focused window switch, move focused container.

**Exit criteria:** i3 actions are stable, deterministic, and recover from IPC failures.

### Milestone M7: tmux Menu + Remote Control Actions

**Outcome:** First-class tmux session/window/pane control from menu and internal actions.

- [ ] Task M7.1: Add tmux snapshot adapter in `unilii/lib/src/tmux/client.rs` (CLI-first).
- [ ] Task M7.2: Add tmux menu controller/rendering in `unilii/bin/src/menus/tmux.rs`.
- [ ] Task M7.3: Add tmux actions (`switch-client`, `select-window`, `select-pane`, `send-keys`) with target validation.
- [ ] Task M7.4: Add destructive action confirmations (`kill-session`, `kill-window`).
- [ ] Task M7.5: Add keybinding internal actions for tmux (`bar:tmux:*` namespace).
- [ ] Task M7.6: Add config parsing under `[menus.tmux]` including multi-socket profiles.
- [ ] Task M7.7: Add tests for snapshot parsing and action orchestration.

**Verification:**

- `cargo test -p unilii-lib tmux_client_tests`
- `cargo test -p unilii-bin tmux`
- Manual: session/window/pane switch + send command + kill confirmation.

**Exit criteria:** tmux control paths are reliable across socket/profile failures with clean rollback.

## 3. Testing Strategy by Layer

- Unit: key state machine, parsers, config defaults, reminder dedup, action runner edge cases.
- Integration: command output fixtures for nmcli/lsblk/mount/caldav, import translation tests.
- E2E/manual: connectivity actions, mount lifecycle, reminder UX, keybinding migration cutover.
- E2E/manual: i3 workspace/window switching and tmux remote-control flows.
- Soak: 24h key event stream + periodic menu refresh + command failures.

## 4. Risk Register and Mitigations

- Risk: backend CLI output variance across distros.
  - Mitigation: tolerant parsers + fixture matrix + explicit backend version checks.
- Risk: stuck keyboard state under device hotplug or stream errors.
  - Mitigation: heartbeat/reset path in `KeyEngine`, event-sequence invariants tests.
- Risk: privilege boundary confusion in mount/vcvolume flows.
  - Mitigation: explicit command template contracts and user-visible permission errors.
- Risk: CalDAV conflict complexity.
  - Mitigation: read-first milestone, conservative write rollout, preserve-both conflict model.
- Risk: i3/tmux backend version/format drift.
  - Mitigation: fixture-driven parsers and backend capability checks on startup.

## 5. Observability Requirements

- Add structured events for menu open latency and action outcomes.
- Add counters for key trigger attempts/success/failure by trigger type.
- Add stale-cache age and sync duration metrics for calendar.
- Add migration diagnostics summary for `sxhkd` import.
- Add i3/tmux action success/failure counters by command category.

## 6. Suggested Commit Slicing

- Commit group 1: M0 foundation only
- Commit group 2: M1 key engine + tests
- Commit group 3: M2 WiFi menu
- Commit group 4: M3 mount stack
- Commit group 5: M4 calendar read/reminders
- Commit group 6: M5 calendar writes + migration + docs
- Commit group 7: M6 i3 menu + actions
- Commit group 8: M7 tmux menu + actions

Each group should compile and pass relevant tests independently.

## 7. Rollout Checklist (to be filled during implementation)

Create `unilii/docs/plans/2026-04-16-menus-keybinding-rollout-checklist.md` with:

- feature flags and defaults by release
- migration steps for existing users
- rollback instructions
- support diagnostics commands
