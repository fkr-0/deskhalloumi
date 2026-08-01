# DeskHalloumi release roadmap

This is the human-readable release plan for the coordinated Cargo workspace.
The machine-oriented source of truth is [`roadmap.yml`](../roadmap.yml), and the
0.3.2 implementation assessment is
[`implementation-state-0.3.2.md`](implementation-state-0.3.2.md).
Active work is tracked in [`tasks.yml`](../tasks.yml); detailed implementation evidence through 0.3.2 is archived in [`history/tasks-through-0.3.2.yml`](history/tasks-through-0.3.2.yml).

The roadmap defines acceptance boundaries, not dates. A capability belongs to a
release only after its exit criteria pass. Every first-party crate inherits one
workspace version and is represented by one annotated workspace tag.

## Tagged baseline: 0.3.2

The local annotated tag `v0.3.2` points to
`84e9ca856a25a7988373d3622d4be9cb51646974`. The safe workspace tests, isolated
i3/X11 integration, strict Clippy, release metadata validation, artifact smoke
tests, and an independent review all passed before tagging. The tag and branch
have not been pushed and no remote publication is implied.

The baseline already includes:

- supervised Tokio runtime ownership and bounded shutdown;
- bounded action execution, IPC frames, connections, queues, output, and
  timeouts;
- typed provider contracts and snapshots with refresh and instance generations;
- last-known-good provider retention and health metadata;
- renderer-neutral menu, typed action, quick-select, history, and introspection
  foundations;
- generated i3 bindings, recursive conflict audit, transactional rollback, and
  selective native X11 advanced hotkeys;
- dynamic evdev keyboard hot-plug;
- deterministic Linux x86-64 release archives and checksums.

Later releases complete migration and prove these foundations under churn. They
must not create parallel runtimes, provider stacks, menu models, or action buses.

## Workspace-wide responsibility map

| member | next patch: 0.3.3 | next minor: 0.4.0 | stable-major requirement: 1.0.0 |
| --- | --- | --- | --- |
| `deskhalloumi` | clarify facade documentation and legacy naming | decide whether it is a supported facade or internal convenience crate | expose an intentional, tested public facade or mark it non-publishable/internal |
| `deskhalloumi-core` | boundary regressions and API inventory | complete renderer-neutral provider lifecycle and diagnostics contracts | stabilize selected config, action, menu, hotkey, IPC, and plugin-facing APIs with compatibility policy |
| `deskhalloumi-lib` | remove stale docs and improve hardware-free seams | provide injected Linux/CalDAV backends and deterministic fixtures | define stable OS-adapter contracts, error semantics, and supported feature combinations |
| `deskhalloumi-bin` | release-state docs, regression closure, archival cleanup | extract provider/runtime ownership and expose live health end to end | become a thin composition root with one authoritative menu/action/provider state path |
| clock plugin | injectable time seam tests | publish through the shared provider adapter without direct lifecycle shortcuts | conform to the supported compile-time plugin API and compatibility tests |
| battery plugin | constructor/subscription fixture seams | remove mandatory live-sysfs discovery from ordinary construction and tests | stable provider behavior on battery, desktop, and battery-less hosts |
| Tmux plugin | command fixture and failure-path coverage | inject command backend and expose stale/error/disabled state consistently | stable bounded action/provider behavior without requiring a live tmux server in tests |

Dynamic plugin loading is not a roadmap promise. The current plugin model is
compile-time optional crates; 1.0 may stabilize that model without introducing a
runtime ABI.

## 0.3.3 — Release follow-up and regression closure

This patch remains backwards compatible and deliberately narrow. It may not
consume the provider-convergence feature scope assigned to 0.4.0.

### Primary work

- Reconcile repository documentation with the locally tagged 0.3.2 state while
  keeping push and publication state explicit.
- Verify the exact assembled 0.3.2 archive can be checksum-verified, extracted,
  installed into a temporary prefix, smoke-tested, and removed without using the
  source tree.
- Add exact boundary regressions for maximum-size IPC frames and the intended
  durable-write failure contract after rename and parent-directory sync.
- Replace contract-name-only plugin tests with small injectable seams where this
  can be done without changing public behavior.
- Remove tracked archival `.bak` source files after proving that no build,
  fixture, or documentation path references them.
- Correct stale `unilii` prose in active crate-level documentation while
  retaining intentional compatibility names.
- Split current historical tasks from active tasks so new reviews do not treat
  old line counts and completed migration notes as current state.

### Exit criteria

- No public CLI, config, IPC, service, application-ID, or path compatibility
  change.
- All seven packages inherit `0.3.3` from the workspace and the lockfile agrees.
- Safe tests, isolated i3/X11 integration, strict Clippy, and release metadata
  validation pass.
- Plugin unit tests require neither battery hardware nor a tmux server and can
  control clock time where time-dependent behavior is tested.
- No tracked `*.bak` file remains in the active source tree.
- Release notes distinguish local tagging, remote push, artifact publication,
  and crates.io publication.

### Non-goals

- No Wayland shortcut parity.
- No dynamic plugin ABI.
- No removal of `unilii-*` compatibility launchers.
- No broad menu or provider rewrite.

## 0.4.0 — Provider hardening and target expansion

This minor completes the provider lifecycle migration introduced in 0.3.0 and
hardened in 0.3.2.

### Shared provider contract

Clock, battery, network, audio, system, and optional Tmux providers must use one
end-to-end contract for:

- startup, loading, fresh, stale, error, disabled, shutting-down, and stopped
  states;
- refresh interval, timeout, staleness threshold, and startup-refresh policy;
- keyed bounded refresh admission and coalescing;
- refresh-generation and provider-instance-generation acceptance;
- last-known-good retention;
- bounded graceful shutdown;
- a real fixture or in-memory backend selected without live hardware or service
  access.

`ProviderContract` declarations must correspond to executable behavior. A string
naming a hypothetical test backend is not sufficient acceptance evidence.

### Component work

- **Core:** separate provider data/lifecycle contracts from Iced rendering and
  define one adapter boundary from provider snapshots to module/UI updates.
- **Library:** make sysfs, evdev/udev, process, and CalDAV access injectable or
  fixture-driven at the provider boundary, with structured errors rather than
  silent environmental assumptions.
- **Binary:** extract provider creation, replacement, cancellation, and health
  aggregation from `main.rs`; remove fixed or provider-specific state paths.
- **Clock:** inject a clock source for deterministic behavior and missed-tick
  tests.
- **Battery:** support battery-less hosts as an explicit disabled/unavailable
  state and test live-device replacement without physical hardware.
- **Tmux:** inject command execution, distinguish no server from malformed
  output, and retain last-known-good pane state on transient failure.

### Operator visibility

Expose provider id, lifecycle state, active instance generation, refresh
generation, last successful update, last-update age, last bounded error, and
refresh pressure through live action-bus introspection and at least one panel
surface.

### Soak and target validation

- Repeated provider replacement and overlapping refresh attempts must not leak
  tasks, apply stale results, grow logs without bound, or leave nonzero gauges.
- Record reference idle CPU, resident memory, task, file-descriptor, and log
  budgets for provider churn.
- Add native Linux AArch64 build, archive, installation, runtime-smoke, and
  removal validation. Publish AArch64 assets only after the native lane passes.
- Complete a musl feasibility ADR that may explicitly reject publication.

### Exit criteria

- Every named production provider uses the shared lifecycle and snapshot path.
- Ordinary tests require no battery, input device, NetworkManager, audio daemon,
  tmux server, compositor, or desktop session.
- Stale refresh and stale provider-instance results are rejected by tests across
  all provider classes.
- Live diagnostics expose provider health and age without revealing secrets.
- `main.rs` no longer owns provider-specific lifecycle policy.
- AArch64 publication remains conditional on native artifact testing.

### Non-goals

- Cross-compilation alone is not publication evidence.
- A static musl artifact is not promised.
- Sway/Wayland global-hotkey parity remains outside this release.

## Intermediate minors toward 1.0

The following milestones remain ordered prerequisites unless their exit criteria
are completed earlier and recorded explicitly.

### 0.5.0 — Menu and action convergence

- Make `MenuModel` authoritative for tray, widget, custom, filter-tab, and system
  surfaces.
- Standardize closed/loading/busy/fresh/stale/disabled/error presentation.
- Prefer live daemon state in CLI inventories, clearly labeling configured-only
  fallback.
- Return structured action outcomes or history ids from typed invocation.
- Split `main.rs` into bootstrap, runtime ownership, action routing, update, and
  Iced adapter boundaries.
- Retire broad dead-code allowances as canonical paths replace transitional
  implementations.

### 0.6.0 — Input semantics and migration completion

- Define explicit logical-key versus physical-key configuration.
- Add en-US and de-DE layout-sensitive fixtures and tests.
- Report exact, approximate, layout-dependent, and unsupported migration
  outcomes before writing configuration.
- Extend sxhkd migration only where semantics are representable.
- Add long-running hot-plug, reload, managed-menu, and action-bus soak tests.

### 0.7.0 — Packaging and operational maturity

- Package-quality systemd user integration and reproducible Arch packaging.
- SBOM, dependency/license inventory, provenance, and attestations.
- Post-publication checksum, installation, command-smoke, and removal tests.
- Upgrade and rollback tests between consecutive releases.
- Bounded redacted diagnostics and preview-first configuration migration with
  backup and restore.

### 0.8.0 — Experimental compositor portability

- Keep Sway/Wayland work feature-gated and experimental.
- Separate compositor-neutral actions from i3/X11 implementations.
- Prototype layer-shell, Sway IPC, output control, and shortcut registration.
- Publish a capability matrix based on isolated compositor tests.

Version 0.8.0 is not a mandatory prerequisite for 1.0 if DeskHalloumi retains
i3/X11 as its explicit stable support boundary. Experimental portability must
not delay or weaken the stable i3/X11 contract.

## 1.0.0 — Stable desktop-control contract

Version 1.0 stabilizes an intentionally selected public surface. It does not
mean every currently public Rust item or experimental backend becomes stable.

### Compatibility inventory

Before the first release candidate, publish a machine-readable inventory of:

- primary and compatibility binary names and CLI options;
- configuration schemas, defaults, path precedence, and migration rules;
- environment variables;
- systemd unit names and lifecycle behavior;
- Unix socket locations, protocol versions, messages, limits, and negotiation;
- DBus names and desktop application IDs;
- generated i3 configuration semantics;
- persistent cache/state formats;
- supported Rust crates, features, and plugin-facing APIs.

Every inventoried surface must be classified as stable, deprecated with a
removal horizon, experimental, or internal.

### Monorepo architecture requirements

- `deskhalloumi-bin` is a thin composition root and renderer adapter, not the
  owner of core provider, menu, action, or hotkey semantics.
- `deskhalloumi-core` has explicit public/internal module boundaries and no
  accidental stabilization of implementation details.
- Renderer-neutral provider and menu contracts do not require Iced types.
- `deskhalloumi-lib` has documented Linux adapter and error contracts with
  deterministic fixtures.
- Clock, battery, and Tmux satisfy one compile-time plugin/module contract with
  compatibility tests and no mandatory live-service dependency in unit tests.
- The role of the top-level `deskhalloumi` crate is explicit: supported facade,
  internal package, or non-published compatibility crate.
- The crates.io publication decision is recorded per crate; publication is not
  required merely because the packages exist.

### Reliability and operations requirements

- Config and IPC schemas are versioned and have upgrade/downgrade or rejection
  tests.
- Release artifacts have reproducible build evidence, checksums, SBOM,
  provenance, installation, upgrade, rollback, and removal verification.
- Supported providers and hotkey paths meet documented soak and resource
  budgets.
- Parsers for config, IPC, i3, sxhkd, DBus menu data, and external command output
  have property, fuzz, corpus, or adversarial coverage appropriate to risk.
- Diagnostics are bounded and redacted by default.
- All release-critical behavior is testable without mutating the operator's
  desktop session.

### Compatibility and platform decision

The `unilii` compatibility layer must have an explicit decision before 1.0:
retain it for the 1.x series, deprecate it with a documented removal major, or
provide a measured migration release. It must not disappear implicitly.

The stable platform matrix may name only i3/X11. Wayland parity is optional; an
unsupported or experimental Wayland row is acceptable when documented clearly.

### 1.0 exit criteria

- All selected stable surfaces have compatibility tests and migration docs.
- No broad transitional `dead_code` or architecture-debt allowance remains in a
  production module.
- No tracked archival source file is part of the canonical build tree.
- Installation, upgrade, rollback, and removal pass from assembled artifacts.
- A release candidate has completed a soak period with no unresolved P0/P1
  correctness, security, data-loss, or compatibility defect.
- The annotated 1.0.0 tag, revision vector, artifact hashes, SBOM, provenance,
  and verification evidence are reproducible and archived.

## Dependency order

1. Tag and verify 0.3.2 locally — complete.
2. Close narrow regressions and repository-state debt in 0.3.3.
3. Complete provider lifecycle and health convergence in 0.4.0.
4. Complete menu/action convergence and thin the binary composition root.
5. Make input semantics and migration behavior explicit.
6. Reach package-quality installation, diagnostics, provenance, and rollback.
7. Decide the stable public-surface inventory and compatibility policy.
8. Cut 1.0 release candidates; experimental Wayland work may proceed in
   parallel but is not a hidden stability requirement.

## Release-wide definition of done

Every release requires formatting, safe hardware-neutral tests, strict Clippy,
release metadata validation, relevant isolated desktop integration tests,
annotated immutable tags, release notes, deterministic archives, checksums, and
explicit publication state.

Every newly published architecture additionally requires a native build,
installation test, runtime smoke test of every packaged primary command, and
clean removal from the assembled artifact.
