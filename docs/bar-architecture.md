# deskhalloumi-bar architecture decisions

This document records the first implementation decisions for the Polybar replacement roadmap tracked in `tasks.yml` under `feature_implementation_plan.id = "polybar_makepad"`.

## Current repository fit

The active workspace layout is:

```text
unilii/core      shared types, config validation, registries, key handling
unilii/bin       runnable binaries and the current Iced status bar
unilii/lib       host/system data utilities
unilii/plugins   existing linked modules
```

The first implementation therefore uses `unilii/core` for bar config and provider metadata, and adds the runnable `deskhalloumi-bar` binary inside the existing `unilii/bin` package. This avoids creating a parallel `crates/*` tree that does not match the current repository.

## Accepted decisions

- First runnable target: `deskhalloumi-bar`, a headless/testable scaffold binary in `unilii/bin`.
- First shared API: typed bar config and provider metadata in `unilii_core::bar`.
- First extension model: built-in providers plus script modules. Dynamic plugins remain deferred.
- First renderer boundary: text/view-model-ready module metadata and config validation. A native Makepad renderer remains a later rendering backend task.
- First WM backends: i3 and sway are the intended priority, with generic shell/X11 fallback after the runtime contract is stable.
- Screen reservation: best effort only until the concrete renderer/windowing layer is selected. WM rules, layer-shell/struts, or always-on-top fallback must be documented per backend.

## Explicit deferrals

- Makepad UI rendering and native window placement are deferred until the config, runtime, and module contracts compile and are tested.
- Event-socket i3/sway integrations are deferred behind a `WmBackend` trait.
- Dynamic ABI/plugin loading is deferred behind provider metadata and script modules.
- Full notification daemon and tray implementation are out of the first scaffold; a notification indicator module is tracked separately.

## MVP checklist

- `deskhalloumi-bar --print-default-config` prints a valid starter TOML config.
- `deskhalloumi-bar --check-config <path>` validates config and reports actionable errors.
- `unilii_core::bar` validates duplicate ids, unknown module types, missing script commands, invalid intervals, and bad layout references.
- Built-in provider metadata exists for workspaces, window_title, clock, system, network, vpn, audio, battery, script, and notifications.
- The later renderer can consume the same config/provider contracts without moving them out of core.
