# deskhalloumi-bar architecture decisions

This document records the role of the separate `deskhalloumi-bar` runtime and the final decision about its execution model.

## Current repository fit

The active workspace layout is:

```text
unilii/core      shared types, config validation, registries, key handling
unilii/bin       runnable binaries and the current Iced status bar
unilii/lib       host/system data utilities
unilii/plugins   existing linked modules
```

The implementation uses `unilii/core` for bar config and provider metadata and keeps the runnable `deskhalloumi-bar` binary inside the existing `unilii/bin` package. This avoids a parallel `crates/*` tree that does not match the current repository.

## Accepted decisions

- `deskhalloumi-bar` is a headless, synchronous reference runtime in `unilii/bin`.
- `deskhalloumi` is the supported interactive desktop runtime and owns Tokio task supervision, Iced rendering, tray/DBus integration, hotkeys, and live provider channels.
- `deskhalloumi-bar` will not be migrated to the shared Tokio runtime. Its blocking execution is intentional because its supported uses are finite diagnostics, fixture-backed provider checks, config validation, scheduler tests, reload reference behavior, and text render-model inspection.
- First shared API: typed bar config and provider metadata in `unilii_core::bar`.
- First extension model: built-in providers plus script modules. Dynamic plugins remain deferred.
- Renderer boundary: text/view-model-ready module metadata and config validation. Interactive rendering belongs to `deskhalloumi`; a separate Makepad experiment must not turn the reference runtime into a second production daemon.
- First WM backends: i3 and sway are the intended priority, with generic shell/X11 fallback after the runtime contract is stable.
- Screen reservation: best effort only until the concrete renderer/windowing layer is selected. WM rules, layer-shell/struts, or always-on-top fallback must be documented per backend.

## Excluded responsibilities

- Interactive panel rendering and native window placement.
- Long-lived Tokio task supervision.
- Tray, DBus, global-hotkey, or desktop-session ownership.
- Production provider refresh loops.
- Dynamic ABI/plugin loading.

New production features in these categories must be implemented in `deskhalloumi`, not duplicated here.

## MVP checklist

- `deskhalloumi-bar --print-default-config` prints a valid starter TOML config.
- `deskhalloumi-bar --check-config <path>` validates config and reports actionable errors.
- `deskhalloumi-bar --runtime-contract` prints the machine-readable execution-model decision.
- `unilii_core::bar` validates duplicate ids, unknown module types, missing script commands, invalid intervals, and bad layout references.
- Built-in provider metadata exists for workspaces, window_title, clock, system, network, vpn, audio, battery, script, and notifications.
- Other renderers can consume the same config/provider contracts without moving them out of core.
