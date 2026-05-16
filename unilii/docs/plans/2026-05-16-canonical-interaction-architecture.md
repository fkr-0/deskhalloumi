# Canonical interaction architecture

## Status

Date: 2026-05-16

This document records the current direction for unilii's interaction architecture. It is intentionally small and operational: it names the canonical owners, the transitional/deprecated paths, and the extraction order used by the current `tasks.yml` work.

## Goals

- Keep unilii's Iced application as the host architecture.
- Move state transitions out of `main.rs` into renderer-independent update helpers.
- Import useful modrelease-2 interaction semantics as tests and model transitions, not as Cairo/X11 renderer code.
- Keep each extraction testable with focused unit tests plus workspace check/clippy/test gates.

## Canonical owners

| Concern | Canonical owner | Notes |
| --- | --- | --- |
| Runtime bootstrap and daemon wiring | `unilii/bin/src/main.rs` for now | This file should shrink toward wiring only. Startup/bootstrap error mapping is a future extraction slice. |
| App message and bar state types | `unilii/bin/src/app.rs` | `Message` and `UniliiBar` are the current message/state boundary. |
| Message/state transition helpers | `unilii/bin/src/update/*` | New behavior should land here or in a domain module called from here, not directly in `main.rs`. |
| Enhanced tray event transitions | `unilii/bin/src/update/enhanced_tray_events.rs` | Owns `apply_enhanced_tray_event` and its focused behavior tests. |
| Tray data model/state/rendering | `unilii/bin/src/enhanced_tray/*` | Preferred model for StatusNotifier/DBus menu behavior. |
| Legacy tray parsing and compatibility helpers | `unilii/bin/src/tray.rs` | Keep tested parsing helpers; migrate update/state logic into canonical owners as slices are extracted. |
| Menu domain models | `unilii/bin/src/menus/*` | Network, mount, calendar, and custom menu models remain domain-specific until shared lifecycle helpers are introduced. |
| Keybinding model and import | `unilii/core/src/keys.rs`, `unilii/core/src/key_engine.rs`, `unilii/core/src/key_import_sxhkd.rs` | Renderer-free and suitable for modrelease-2 parity imports. |
| Side-effect command execution | `unilii/bin/src/action_runner.rs` | Shell-backed actions should return explicit outcomes, not panic. |

## Deprecated or transitional paths

- `unilii/bin/src/main.rs` as a god module is deprecated. It remains the current bootstrap/update/view host while slices move out.
- `unilii/bin/src/enhanced_tray_backup.rs`, `main.rs.bak`, and other backup/archive files are not canonical source and should be handled by repo-hygiene work.
- Local `FIXME(T6)` dead-code allowances mark transitional APIs. They should be retired by wiring, deleting, or moving code into test/support modules.
- Direct feature additions to `main.rs` are discouraged. Add behavior behind focused tests in `update/*`, `menus/*`, `enhanced_tray/*`, `core/*`, or `lib/*` as appropriate.

## Update extraction rules

1. Characterize the current behavior with a focused RED test at the intended new module boundary.
2. Move the smallest helper or branch into the new owner.
3. Keep `main.rs` as a thin caller for that slice.
4. Remove duplicate old tests once the new module owns the behavior.
5. Run:

    cargo check --workspace
    CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

6. Update `tasks.yml` with completion evidence and new follow-up tasks.

## Current extracted update modules

- `update/enhanced_tray_events.rs`
  - Owns `apply_enhanced_tray_event`.
  - Handles `IconsUpdated`, `MenuUpdated`, `DbusMenuReceived`, `FavoritesChanged`, and `NavigationChanged`.
  - Has module-level tests for tree/menu updates and favorites state creation.

## Next extraction order

1. `main.rs` startup/bootstrap error mapping.
2. Tray navigation branches such as `TrayNavigateLeft` and `TrayNavigateRight`.
3. Favorite toggling such as `TrayToggleFavorite`.
4. Network, mount, and calendar snapshot update branches.
5. Menu lifecycle helpers informed by modrelease-2: escape dismissal, click-outside dismissal, focus-loss dismissal, keyboard navigation, and action emission.

## Modrelease-2 import policy

Port behavior, not renderer stack:

- Import model and state transition semantics as failing unilii tests first.
- Adapt X11/keycode behavior through unilii's key engine and input abstractions.
- Reject Cairo render code and direct X11 focus/window assumptions unless they are isolated behind a backend boundary.
- Prefer renderer-free fixtures and pure state tests before live DBus, NetworkManager, i3/sway, or GUI-window tests.

## Completion signal for T1

T1 is not complete until:

- `main.rs` contains only bootstrap/wiring and minimal daemon setup.
- New menu/tray/keybinding behavior has a single documented update path.
- Transitional/dead-code allowances have been retired or explicitly moved to test/support modules.
- README, CONFIGURATION.md, and relevant docs agree on the current architecture.
