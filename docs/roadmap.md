# DeskHalloumi release roadmap

This is the human-readable release plan for the coordinated Cargo workspace.
The machine-oriented source of truth is [`roadmap.yml`](../roadmap.yml), and the
0.3.2 implementation assessment is
[`implementation-state-0.3.2.md`](implementation-state-0.3.2.md).
Active work is tracked in [`tasks.yml`](../tasks.yml); detailed implementation evidence through 0.3.2 is archived in [`history/tasks-through-0.3.2.yml`](history/tasks-through-0.3.2.yml).

The roadmap defines acceptance boundaries, not dates. A capability belongs to a
release only after its exit criteria pass. Every first-party crate inherits one
workspace version and is represented by one annotated workspace tag.

## Tagged baseline: 0.3.3

The local annotated tag `v0.3.3` points to
`c2daa0a1a09cb1b739c2a588945d11efdb9f10f4`. The complete safe workspace tests,
isolated i3/X11 integration, strict Clippy, deterministic x86-64 archive,
installation/removal smoke, and independent review passed before tagging. The
tag and branch have not been pushed and no remote publication is implied.

The baseline already includes bounded runtime/action/IPC behavior, transactional
i3 generation, selective X11 hotkeys, hardware-free plugin seams, exact frame
limits, durable-file failure semantics, deterministic release archives, and
explicit separation of local tags from remote publication.

## Workspace-wide responsibility map

| member | 0.4.0 completion | next convergence work | stable-major requirement |
| --- | --- | --- | --- |
| `deskhalloumi` | unchanged facade/helpers | decide supported facade versus internal convenience crate | expose an intentional tested facade or mark it non-publishable/internal |
| `deskhalloumi-core` | shared provider runner, status registry, metrics, churn tests | continue API boundary inventory | stabilize selected config, action, menu, hotkey, IPC, and plugin-facing APIs |
| `deskhalloumi-lib` | existing Linux/CalDAV adapters remain provider inputs | make remaining adapter errors and fixtures uniform | define stable OS-adapter contracts and supported feature combinations |
| `deskhalloumi-bin` | built-in provider ownership extracted to `provider_runtime.rs`; live status CLI/panel | continue menu/action and Iced composition-root extraction | become a thin composition root with one authoritative state path |
| clock plugin | shared timed lifecycle with injectable clock | no separate lifecycle stack | conform to the selected stable compile-time plugin API |
| battery plugin | battery-less hosts become explicit disabled state | refine richer charging/state fixtures | stable behavior on battery and battery-less hosts |
| Tmux plugin | shared refresh/shutdown helpers plus bounded selection operations | standardize menu/action presentation | stable bounded behavior without requiring a live tmux server in tests |

Dynamic plugin loading is not promised. The current model remains compile-time
optional crates and may be the stable 1.0 model.

## 0.3.3 — Tagged regression and hygiene baseline

Version 0.3.3 is complete and locally tagged. It added archive installation and
removal smoke tests, exact IPC frame boundaries, explicit generated-file failure
phases, executable Clock/Battery/Tmux fixture sources, historical task-ledger
archiving, and removal of tracked backup sources without changing compatibility
contracts.

## 0.4.0 — Locally tagged provider lifecycle release

The annotated local tag `v0.4.0` points to
`373ce47911869fa63b847433bd9f0272f46ada4f`. The implementation and all local
release gates are complete. It does not introduce a parallel provider subsystem; it finishes the
migration to the typed runtime contracts introduced in 0.3.0.

### Shared lifecycle

Clock, battery, network, audio, system, and optional Tmux providers now share:

- startup, loading, fresh, stale, error, disabled, shutting-down, and stopped
  states;
- keyed bounded refresh admission and coalescing;
- refresh timeout and cancellation;
- refresh-generation and provider-instance-generation acceptance;
- last-known-good retention and UTF-8-safe bounded error detail;
- bounded backend shutdown with failure/timeout metrics;
- executable hardware- or service-free backends for ordinary tests.

Battery-less hosts and unavailable tmux servers publish explicit disabled states
rather than failing startup. A process-wide diagnostic status registry records
only bounded lifecycle metadata and rejects stale provider instances; typed
watch channels remain the authoritative provider values.

### Binary ownership and operator visibility

Built-in audio, network, and system lifecycle ownership moved from `main.rs` to
`unilii/bin/src/provider_runtime.rs`. The Iced layer requests operations and
applies typed snapshots but no longer owns admission, timeout, or generation
policy.

`deskhalloumi provider-status [--json]` reports provider id, health, active
instance generation, refresh generation, refresh policy, last-update age, and
bounded errors over the existing action bus. The panel displays a compact badge
for the worst live provider. Runtime metrics additionally count provider refresh
failures/timeouts and shutdown failures/timeouts.

### Churn and architecture targets

A 128-generation replacement regression verifies that stopped old instances
cannot overwrite active status and every refresh permit is returned.

The release workflow configures a native `ubuntu-24.04-arm` lane that builds the
same twelve binaries, assembles a deterministic AArch64 GNU/Linux archive,
installs it into a temporary prefix, smoke-tests every command, verifies its
checksum, and removes it. GitHub Release publication waits for both x86-64 and
AArch64 jobs. This native lane cannot be executed by the local x86-64 release
host, so local evidence must not claim it passed.

ADR 0001 intentionally rejects a 0.4.x musl artifact. A future reconsideration
requires a named native musl environment and complete build, installation,
runtime-smoke, and removal evidence.

### Release evidence

- All named providers use the shared lifecycle and snapshot path.
- No provider-specific mutable state registry remains; the global status registry
  is diagnostic-only and stale-instance-safe.
- Ordinary tests require no physical battery, input device, NetworkManager,
  audio daemon, tmux server, compositor, or desktop session.
- Live provider health is available through the action bus and panel.
- Repeated replacement does not leak refresh permits or stale status.
- Local formatting, safe tests, isolated i3/X11 integration, strict Clippy,
  deterministic x86-64 archive, and installation/removal smoke pass.
- Remote publication remains blocked until the native AArch64 lane succeeds.
- No musl artifact is promised or published.

### Non-goals

- No Wayland global-shortcut parity claim.
- No dynamic plugin ABI.
- No removal of `unilii-*` compatibility launchers.
- No claim that local x86-64 validation substitutes for native AArch64 evidence.

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
