# DeskHalloumi release roadmap

This document is the human-readable release plan. The machine-oriented source
of truth is [`roadmap.yml`](../roadmap.yml); detailed implementation evidence
lives in [`tasks.yml`](../tasks.yml).

The roadmap describes intended release boundaries, not promises about dates.
A feature belongs to a release only after its acceptance criteria pass. An
architecture target is not published merely because it cross-compiles.

## Current baseline: 0.3.0

DeskHalloumi 0.3.0 shipped more of the provider and menu foundations than the
older roadmap expected:

- a supervised Tokio runtime with bounded shutdown;
- typed `ProviderContract` and `ProviderSnapshot` state over Tokio watch
  channels;
- refresh- and instance-generation stale-result suppression;
- keyed, bounded provider refresh admission and coalescing;
- hardware-free provider backends and tests;
- renderer-neutral `MenuModel`, typed menu actions, and one-shot quick-select;
- CLI inventories, typed action invocation, live runtime metrics, and bounded
  visible action history.

The next releases therefore focus on completing migration, proving behavior
under churn, improving operational visibility, and removing transitional
adapters. They do not reimplement these foundations.

## 0.4.0 — Provider hardening and target expansion

### Provider lifecycle completion

Clock, battery, network, audio, system, and optional Tmux providers must use one
contract end to end. Every provider declares:

- startup and loading behavior;
- fresh, stale, error, disabled, shutting-down, and stopped states;
- refresh interval, timeout, staleness threshold, and startup-refresh policy;
- bounded graceful shutdown behavior;
- a fixture or in-memory backend usable without its live service or hardware.

The production runtime must not retain a fixed clock/battery registry or a
provider-specific state channel. Refresh admission is keyed and bounded so an
already running refresh cannot overlap with a duplicate request. A result is
accepted only when both its refresh generation and provider-instance generation
still belong to the active provider.

Provider status must expose health, last successful update, calculable
last-update age, active instance generation, and current refresh generation.
This information should be available through live action-bus diagnostics and at
least one panel-facing status surface.

Ordinary tests must run without a physical battery, `/dev/input`,
NetworkManager, an audio daemon, tmux, i3, X11, Wayland, or a live desktop
session. Soak tests repeatedly replace providers, trigger refreshes, cancel
work, and inject late results while checking for stale updates, leaked tasks,
and unbounded logs.

### Linux AArch64

Publish Linux AArch64 binaries only after all of the following pass on native
AArch64 Linux:

1. release build from the annotated tag;
2. deterministic archive assembly and checksum verification;
3. extraction and installation into a clean prefix;
4. `--help` or equivalent non-desktop smoke tests for every packaged primary
   command;
5. runtime smoke tests for the supported non-destructive provider and action
   paths;
6. clean uninstall or prefix removal.

Cross-compilation or emulation alone is useful development evidence but is not
sufficient for publication.

### musl investigation

Investigate musl, but do not promise a static artifact. Iced, WGPU/graphics,
Wayland/X11, DBus, udev, font, and desktop-system dependencies may make a useful
and genuinely portable static build impractical. The deliverable is an
architecture decision record that may recommend not publishing musl binaries.

## 0.5.0 — Menu and action convergence

The 0.3.0 `MenuModel` is the canonical core model. Version 0.5.0 completes the
migration of tray menus, widget menus, custom launchers, filter-tab, and system
actions so renderer- or provider-specific types are adapters rather than
parallel sources of truth.

Primary work:

- derive ordering, selection, enablement, lifecycle, quick-select assignment,
  and typed action lookup from `MenuModel`;
- standardize closed/loading/busy/fresh/stale/disabled/error presentation;
- make CLI introspection prefer live active modules, menus, actions, and
  hotkeys, with configured-only output clearly labelled as such;
- make typed invocation return a structured outcome or action-history id;
- show bounded recent success, failure, timeout, and cancellation detail
  without logging secret-bearing command output by default;
- continue splitting `main.rs` into bootstrap, runtime ownership, action
  routing, and Iced adapters;
- remove transitional duplicate paths and broad dead-code allowances where
  compatibility does not require them.

The release is complete when core menu semantics have no Iced callback or DBus
renderer dependency and all named surfaces use the same lifecycle and action
contract.

## 0.6.0 — Input semantics and migration completion

This release makes the hotkey support contract explicit:

- define logical-key versus physical-key configuration;
- add en-US and de-DE layout-sensitive fixtures and tests;
- report exact, approximate, layout-dependent, and unsupported migration
  outcomes before writing generated configuration;
- extend sxhkd import only where DeskHalloumi can represent the semantics
  exactly;
- keep replay, synchronous execution, unsupported chains, and other lossy
  constructs visibly approximate or unsupported;
- add long-running input hot-plug, configuration reload, managed-menu, and
  action-bus soak tests;
- record idle CPU, resident memory, task, file-descriptor, and log-volume
  budgets on a documented reference environment.

No unsupported source construct should silently become a different action.

## 0.7.0 — Packaging and operational maturity

Primary work:

- package-quality systemd user integration;
- reproducible Arch packaging or repository publication;
- SBOM and dependency/license inventory;
- build provenance and artifact attestations;
- automatic checksum, extraction, installation, and command-smoke verification
  against the exact uploaded release assets;
- upgrade and rollback tests between consecutive releases;
- a bounded, redacted diagnostics bundle for bug reports;
- configuration migration preview, backup, restore, and changed-file reporting.

User services may be installed but must not be enabled implicitly. Migration
tools default to preview and create a restorable backup before writing.

## 0.8.0 — Experimental compositor portability

This release remains explicitly experimental.

Primary work:

- write a Sway/Wayland architecture decision record;
- separate compositor-neutral actions from i3/X11 implementations;
- prototype layer-shell panel behavior, Sway IPC, output control, and portal or
  compositor-specific shortcut registration;
- run isolated compositor tests for startup, shutdown, popup focus, IPC
  reconnect, output churn, shortcut ownership, and failure cleanup;
- publish a capability matrix marking each behavior validated, partial,
  unavailable, or compositor-specific.

DeskHalloumi must not call this parity until global shortcuts, popup focus,
output control, reload, shutdown, and failure behavior are independently
validated. Experimental work must not weaken the supported i3/X11 path.

## Release-wide definition of done

Every release requires formatting, safe hardware-neutral tests, strict Clippy,
release metadata validation, relevant isolated desktop integration tests,
annotated immutable tags, release notes, deterministic archives, checksums, and
successful publication verification.

Every newly published architecture target additionally requires a native build,
installation test, runtime smoke test of every packaged primary command, and
documented target-specific limitations.
