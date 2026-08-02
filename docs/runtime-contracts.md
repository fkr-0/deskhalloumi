# Runtime interaction contracts

This document defines the renderer-neutral contracts shared by DeskHalloumi's Iced frontends, command-line introspection, providers, menus, and action bus.

## Quick-select

Quick-select is a one-shot overlay over an ordered action set.

- Actions are bound in order to `asdfhjklqwertyuiopzxcvbnm1234567890`.
- The overlay shows every assigned key next to its option.
- A plain mapped key executes exactly one bound action and terminates the session.
- Any unmapped, modified, named, or otherwise non-action key aborts the session and is consumed; it must not trigger the underlying view.
- There are no multi-key prefixes or partially bound overlays. Creating a session fails when there are more actions than shortcuts.
- The same `QuickSelectSession` model is used by tray/custom menus and filter-tab.

## Provider lifecycle

Clock, battery, network, audio, system, and optional Tmux providers expose one lifecycle through `deskhalloumi_core::runtime`.

Version 0.4.0 completes the production migration to this contract. Clock,
battery, network, audio, system, and optional Tmux refreshes now use the shared
admission, timeout, generation, cancellation, status-registry, and bounded
shutdown helpers instead of provider-specific lifecycle implementations.

Each provider declares a `ProviderContract` containing:

- stable id and display name;
- refresh interval, timeout, stale threshold, and startup-refresh policy;
- graceful shutdown timeout;
- an executable hardware- or service-free backend used by ordinary tests.

Every published `ProviderSnapshot<T>` contains both a provider-instance generation and a monotonic refresh generation, plus lifecycle state, refresh start time, last successful update time, health, error detail, and calculable last-update age.

Lifecycle states are:

- `startup`: registered but not refreshed;
- `loading`: refresh in progress, optionally retaining the previous value;
- `fresh`: latest generation completed successfully;
- `stale`: refresh failed or exceeded policy while retaining the last known good value;
- `error`: no usable value exists;
- `disabled`: intentionally unavailable by configuration or capability policy;
- `shutting_down` and `stopped`: explicit terminal transitions.

Refreshes use generation tokens. A result is accepted only when its refresh generation equals the current generation, preventing late results within one provider instance from replacing newer state. Every newly constructed provider channel also receives a unique instance generation. Iced subscription identity includes that value, and the application rejects any already-queued snapshot whose instance generation no longer matches the active provider after replacement or reload. Provider refresh admission is keyed and bounded, so duplicate refreshes coalesce and unrelated providers cannot create unbounded work.

Tests use executable fixture or in-memory backends. Ordinary tests do not
require a physical battery, evdev access, NetworkManager, PulseAudio/PipeWire,
tmux, i3, X11, or a live desktop session. Battery-less hosts publish an explicit
disabled state instead of failing module construction; an unavailable tmux
server is likewise distinguished from malformed output or a transient command
failure.

Provider health is not only a rendering hint. The process-wide
`ProviderStatusRegistry` rejects stale provider instances and exposes provider
id, lifecycle/health state, active instance and refresh generations, refresh
policy, last successful update, calculable last-update age, and a UTF-8-safe
bounded error without forcing a refresh. `deskhalloumi provider-status` queries
that registry over the existing bounded action bus. The panel renders the worst
live provider as a compact status badge.

## Menu model

`deskhalloumi_core::menu::MenuModel` is the canonical renderer-neutral representation for:

- tray and DBus menus;
- widget menus;
- custom launchers;
- filter-tab;
- system, mount, calendar, and action-history views.

A menu carries a stable id, title, source, generation, last-update timestamp, lifecycle, and recursive items. Menu lifecycle is standardized as closed, loading, busy, fresh, stale, disabled, or error. Generation checks reject stale menu publications.

Items expose typed actions rather than renderer callbacks. Iced adapters translate those actions at the edge; CLI introspection serializes the same model.

`TrayMenuItem` and other provider/renderer-specific row structures are edge
adapters during migration. They must not become a second canonical lifecycle,
selection, quick-select, or action-routing model. Version 0.5.0 completes this
convergence and makes live runtime introspection authoritative where available.

## Typed actions and history

The CLI exposes:

```text
deskhalloumi list-modules [--json]
deskhalloumi list-menus [--json]
deskhalloumi list-actions [--json]
deskhalloumi list-hotkeys [--json]
deskhalloumi invoke-action <bar|tray|widget> <payload> [--socket PATH]
deskhalloumi runtime-metrics [--json] [--socket PATH]
deskhalloumi provider-status [--json] [--socket PATH]
```

Typed bar, tray, and widget invocations travel through the versioned local action bus. Shell and managed-menu execution remain owned by `deskhalloumi-hotkeyd` and are rejected by the bar action router.

`runtime-metrics` and `provider-status` are synchronous diagnostic action-bus
requests. The running daemon answers in the same bounded response frame; neither
request is queued as a UI action. Runtime metrics include provider refresh
failure/timeout and shutdown failure/timeout counters. Provider status contains
only bounded structured lifecycle data and never command output. Active and
queued gauges are guard-owned, so cancellation or queue teardown cannot leave
stale non-zero values.

The bounded action history records sequence, action id, source, running/succeeded/failed/timed-out/cancelled status, duration, and failure detail. The system menu renders recent failures visibly.

## Architectural boundaries

- `deskhalloumi-core` owns quick-select, provider, menu, action-history, and typed-action semantics.
- `unilii/bin/src/action_routing.rs` owns pure action-bus-to-update routing.
- `unilii/bin/src/introspection.rs` owns CLI inventories and typed invocation.
- `unilii/bin/src/provider_runtime.rs` owns the built-in audio, network, and system provider backends and handles.
- `unilii/bin/src/subscription_manager.rs` adapts provider watch channels into Iced subscriptions and passes the shared cancellation token and refresh registry to provider workers.
- `main.rs` remains the transitional composition root and should not regain semantics extracted into these modules.

## `deskhalloumi-bar` runtime decision

The separate `deskhalloumi-bar` binary remains intentionally synchronous and headless. It is a reference and diagnostic runtime for configuration validation, fixture-backed providers, scheduler/reload behavior, and text render-model inspection. It is not the supported interactive panel and will not be migrated to the shared Tokio/Iced runtime.

The production interactive runtime is `deskhalloumi`. New long-lived desktop providers, tray/DBus ownership, global hotkeys, supervised async work, and graphical panel behavior belong there. `deskhalloumi-bar --runtime-contract` prints this decision as machine-readable JSON so packaging and tests can verify the boundary.
