# Changelog

All notable user-visible changes to the active DeskHalloumi workspace are
recorded in this file. The project follows Semantic Versioning as described in
[`docs/versioning.md`](docs/versioning.md).

Version `0.3.0` is the latest remotely published release and is identified by
the annotated `v0.3.0` tag. Versions through `0.4.0` are represented by local
annotated tags, but no later tag or branch push, remote asset upload, or crate
publication is implied.

## [Unreleased]

### Changed

- Recorded the local annotated `v0.4.0` tag and its still-unpublished native
  AArch64 workflow condition in active release documentation.

## [0.4.0] - 2026-08-02

### Added

- A shared provider operation and runner path covering bounded refresh admission,
  timeout, cancellation, refresh and instance generations, explicit disabled
  state, last-known-good stale state, bounded error retention, and bounded
  backend shutdown.
- A process-wide provider status registry that rejects replaced instances and
  exposes health, generations, refresh policy, last-update age, and bounded
  errors over the existing action bus.
- `deskhalloumi provider-status [--json]` and a compact panel badge showing the
  worst live provider.
- Provider refresh failure/timeout and shutdown failure/timeout counters in live
  runtime metrics.
- Executable fixed backends for audio, network, and system providers, completing
  the hardware- and service-free backend set for all production providers.
- A 128-generation provider replacement soak regression proving stale status and
  refresh permits do not leak.
- A native `ubuntu-24.04-arm` AArch64 release lane with archive installation,
  command smoke, checksum verification, and clean removal.
- ADR 0001 recording that 0.4.x intentionally publishes no musl artifact.

### Changed

- Clock, battery, Tmux, audio, network, and system providers now use the same
  lifecycle, admission, timeout, generation, status, and shutdown contracts.
- Built-in audio, network, and system provider ownership moved from the Iced
  composition root to `unilii/bin/src/provider_runtime.rs`.
- Module subscription supervision now passes the process cancellation token and
  shared refresh registry into lifecycle-managed provider workers.
- Battery-less hosts and unavailable tmux servers publish explicit disabled
  states instead of failing startup.
- GitHub Release publication is deferred until both native x86-64 and AArch64
  GNU/Linux archives pass their release lanes.

### Fixed

- A stopped provider from an older instance can no longer overwrite the status
  of its active replacement.
- Provider errors are retained across shutdown/stopped transitions and truncated
  on a UTF-8 boundary before entering diagnostics.
- Built-in provider refresh and selection paths no longer duplicate timeout and
  generation logic in `main.rs`.

## [0.3.3] - 2026-08-02

### Added

- A source-tree-independent release archive smoke test covering checksum
  verification, safe extraction, temporary-prefix installation, all twelve
  primary and compatibility command help paths, the headless runtime contract,
  and clean removal.
- Exact protocol-boundary regressions for IPC frames at the 64 KiB limit and one
  byte over it in synchronous and asynchronous readers.
- Executable fixture sources for clock, battery, and Tmux module tests.
- A documented generated-file durability contract distinguishing failures before
  destination replacement from failures after rename but before directory sync.
- A package-by-package implementation-state review and coordinated patch, minor,
  intermediate, and stable-major roadmap.

### Changed

- Transactional i3 include installation restores the previous include when the
  candidate destination was replaced but parent-directory durability could not
  be confirmed.
- Active release work now lives in a concise root `tasks.yml`; the detailed
  implementation ledger through 0.3.2 is archived under `docs/history/`.
- The tag, branch, GitHub Release, binary asset, and crates.io publication states
  are documented separately.

### Removed

- Tracked archival `battery.rs.bak` and `main.rs.bak` source files.

### Fixed

- Durable generated-file errors no longer imply that the old destination remains
  visible when rename succeeded but parent-directory synchronization failed.
- Battery and Tmux tests no longer require live hardware or a tmux server, and
  clock tests can control the reported time.

## [0.3.2] - 2026-07-27

### Added

- Runtime bar controls for reloading menu configuration, hiding or showing
  modules, and focusing a module with visible status feedback.
- Strict last-known-good configuration reload validation and runtime status
  metrics for rejected hotkey actions and dropped updates.
- Regression coverage for saturated hotkey queues, stalled control clients,
  conditional tray animation ticks, strict configuration conversion, and i3
  reload rollback.
- Recovery bar actions for restoring all hidden modules, clearing module focus,
  and orderly application exit, including the documented space-separated
  module-action aliases.
- A release-space preflight that fails early with a non-destructive diagnostic
  before a full Rust/Iced build exhausts the filesystem.

### Changed

- Hotkey execution now uses a bounded queue, bounded concurrency, supervised
  Tokio processes, timeouts, process-group cleanup, and bounded output instead
  of spawning one unmanaged thread per command.
- Bar, tray, and embedded hotkey channels are bounded and report overload rather
  than growing without limit.
- Hotkey control connections are served concurrently while state-changing
  requests remain serialized through the supervisor.
- Tray animation ticks run only while an animation is active, and module render
  paths no longer emit per-frame info logs.
- Action-bus, hotkey-control, and selective X11 event paths now enforce bounded
  frames, connection counts, queues, and read/write timeouts at the point of
  admission instead of checking after unbounded allocation.
- Runtime diagnostics now expose active, queued, completed, failed, cancelled,
  timed-out, and rejected actions with cancellation-safe gauges.
- Generated i3 includes are written with unique temporary files, retained
  permissions, file and directory synchronization, and cleanup on failure.
- Reconciled the provider, menu, and runtime documentation with the contracts
  already shipped in 0.3.0, making provider-specific and tray-specific types
  explicit migration adapters rather than parallel canonical models.
- Added a detailed 0.4.0–0.8.0 roadmap covering provider hardening, native
  Linux AArch64 release gates, a non-promissory musl investigation, menu/action
  convergence, input semantics, packaging maturity, and experimental
  Sway/Wayland portability.

### Fixed

- A failed live i3 reload now restores the previous generated include and
  reloads the restored configuration once, avoiding a half-applied binding set.
- Invalid live bar configuration no longer replaces working state with defaults.
- Slow or incomplete control-socket clients no longer block hotkey supervision.
- Concurrent bar reload results are generation-checked, so a slow older reload
  cannot overwrite a newer accepted configuration.
- Same-length atomic hotkey-config replacements are detected through inode,
  device, modification-time, and change-time fingerprints.
- Oversized or unterminated IPC requests and responses are rejected while being
  read, preventing memory growth before the 64 KiB protocol limit is applied.
- Restored compatibility for documented `toggle-module <name>`,
  `focus-module <name>`, and `quit` bar commands.

## [0.3.0] - 2026-07-19

### Added

- Durable GitHub Release publication for validated Linux archives and SHA-256
  checksums, in addition to temporary Actions artifacts.
- A current internal maintainer roadmap, documentation index, complete binary
  installation/upgrade/rollback guide, and async runtime policy.
- Asynchronous action execution through `tokio::process`, with generic action
  timeouts, working-directory and environment support, bounded retained output,
  output byte/truncation metadata, and Unix descendant-process termination.
- Structured module-subscription task monitoring with `JoinSet`, including
  explicit normal-completion, panic, and cancellation diagnostics.
- A shared `deskhalloumi-core` runtime boundary containing bounded action
  execution, owned task supervision, cancellation tokens, keyed provider
  refresh admission, latest-value module channels, and process-wide counters.
- Runtime metrics for active tasks, task outcomes, action durations and
  timeouts, output truncation/discarded bytes, provider coalescing/saturation,
  and dropped or overwritten updates.
- A canonical one-shot quick-select contract with ordered home-row-first key
  bindings, visible overlays, typed activation, and abort-on-any-other-key behavior.
- Typed provider lifecycle snapshots for clock, battery, network, audio, system,
  and Tmux, including health, generations, refresh policy, test backends, stale
  value retention, last-update age, and explicit shutdown states.
- A renderer-neutral menu model shared by tray, widget, custom, filter-tab, and
  system surfaces, plus bounded visible action history with failure details.
- CLI introspection for modules, menus, actions, and hotkeys, and typed local
  action invocation through the action bus.
- Live `runtime-metrics` diagnostics over the action bus, with structured CLI
  output for task, action, timeout, truncation, provider-pressure, and update counters.

### Changed

- Release checksums now contain the archive basename, so `sha256sum -c` works
  directly in the download directory.
- Release retries update the existing GitHub Release and replace its assets
  without moving the immutable source tag.
- Clock, battery, and Tmux subscription producers now return owned worker
  futures to the application supervisor instead of detaching themselves.
- Audio, Wi-Fi, power, video, CopyQ, filter-tab previews, i3 visualizer actions,
  tray networking, mount discovery, Tmux, and CalDAV command paths now execute
  asynchronously with explicit duration and output limits.
- Repeated provider refreshes are coalesced by key and globally bounded; closing
  the main bar cancels and joins its runtime tree within a fixed shutdown window.
- Module subscriptions now publish typed Tokio watch snapshots instead of using
  a fixed clock/battery registry; stale generations cannot overwrite newer data.
- Provider replacements receive unique instance generations, so queued snapshots
  from a pre-reload provider cannot update the active replacement.
- Action-bus routing and CLI inventories were extracted from the transitional
  `main.rs`, and duplicate menu/update paths and broad dead-code allowances were reduced.
- `deskhalloumi-bar` is now explicitly defined as a synchronous headless reference
  runtime; the supported interactive and supervised runtime remains `deskhalloumi`.

### Fixed

- Managed menu process records now use Linux process start-time identity rather
  than relying on an immediately stable `/proc/<pid>/cmdline`, removing a clean-runner spawn/exec race while retaining PID-reuse protection.
- GitHub Actions now installs the `libudev` development headers required by the
  evdev/udev crates on clean Ubuntu runners.
- The release workflow can be dispatched manually for an immutable annotated
  tag, allowing packaging to be retried without moving or replacing the tag.
- Hardware-neutral CI no longer requires an accessible `/dev/input` keyboard;
  keyboard discovery is validated safely even when the device set is empty.
- Calendar formatting tests derive their expectation from the runner's local
  timezone, and module-loading tests no longer require physical battery hardware.
- Release retries use the Rust 1.94.1 toolchain that validated `v0.2.0`, while
  branch CI remains on current stable and the codebase is kept clean under new lints.
- CI toolchain installation explicitly includes `rustfmt` and `clippy`, avoiding
  missing-component failures when using rustup's minimal profile.
- Removed unused alternate tray/update coordinators and stale standalone Wi-Fi
  tests that still contained detached-task and live-command implementations.

## [0.2.0] - 2026-07-18

### Added

- Standalone `deskhalloumi-hotkeyd` supervision with a user-scoped control socket,
  status/ping/reload/shutdown commands, file watching, and transactional worker
  reload with rollback.
- Deterministic press, release, modifier-release, repeat, cooldown, priority,
  and consume semantics in the keybinding engine.
- Shadow-mode and strict migration diagnostics for invalid, duplicate, and
  shadowed keybindings.
- sxhkd configuration import with release-prefix support, same-class
  alphanumeric ranges, underscore empty elements, escaped braces, and pairwise
  chord/command expansion.
- Safe i3/X11 keybinding export through `--print-i3-bindings` and
  `--write-i3-bindings`, with optional `--reload-i3`.
- Atomic generated i3 include replacement and strict fail-closed validation.
- Managed cross-process menu actions for the i3 visualizer, filter-tab, and
  CopyQ frontends.
- Naming migration plan, i3/sxhkd feasibility review, focused `todo.yml`, and a
  documented Semantic Versioning/release policy.
- Automated release-metadata validation for the workspace version and
  changelog structure.
- Recursive i3 configuration auditing with include, variable, mode, `bindsym`,
  and `bindcode` handling plus source-located collision reports.
- A selective native X11 passive-grab backend for modifier-release/hold,
  repeat, cooldown, priority, and consume semantics.
- A versioned, bounded, user-scoped action bus connecting standalone hotkeys to
  bar, tray, and widget actions.
- An isolated Xvfb+i3 integration test that verifies generated press/release
  bindings, atomic rollback, advanced X11 semantics, and trigger suppression.
- A tag-gated release workflow that validates annotated tags, reruns all gates,
  and produces a deterministic Linux binary archive with a SHA-256 checksum.
- An in-app CopyQ shortcut guide available from the header or with `F1`.
- Dynamic evdev keyboard hot-plug handling that adds newly connected keyboards,
  retires removed streams independently, and suppresses stale path generations.
- Canonical GitHub repository and package metadata, an MIT license file, public
  release notes, and license inclusion in deterministic release archives.

### Changed

- Unsupported sxhkd mixed-class ranges, malformed/nested expansions, and
  mode/chord chains are skipped with explicit diagnostics instead of being
  imported as literal nonfunctional chords.
- sxhkd replay bindings are imported only with an explicit warning that replay
  semantics are not preserved.
- sxhkd synchronous command prefixes are stripped before shell execution and
  produce an explicit asynchronous-semantics warning instead of a broken shell
  command beginning with `;`.
- Normal i3 deployments can delegate standard passive key grabs to i3 rather
  than requiring access to raw `/dev/input` devices.
- The project and Cargo packages are now named DeskHalloumi/`deskhalloumi-*`.
  Small `unilii-*` launcher aliases, legacy environment variables, and legacy
  config-path fallback remain available for the pre-1.0 transition.
- New configuration and runtime state default to
  `~/.config/deskhalloumi` and `$XDG_RUNTIME_DIR/deskhalloumi`; old locations
  are read without destructive migration.
- Primary CLI help, version output, logs, and window titles now use
  DeskHalloumi branding while legacy application IDs remain stable for
  existing window-manager rules.
- CopyQ renders a selection-following result window and merges exactly the rows
  currently shown instead of using a hard-coded first-12 limit.

### Fixed

- Managed-menu hide/toggle now treats terminated zombie children as stopped,
  removing a timing-dependent false `TerminationRequested` outcome.
- Restored the workspace-wide `clippy -D warnings` release gate after recent
  bar, menu, and filter-tab additions.
- Replaced heavyweight duplicate compatibility binaries with small exec
  launchers, avoiding redundant GUI links while preserving old command names.
- Made the X11 event worker cancellable and explicitly release passive grabs,
  allowing failed reload candidates to restore the previous generation.
- Fixed CopyQ keyboard navigation losing its visible selection when filtered
  history exceeded the configured rendered-row limit.
- Release metadata validation now verifies every first-party Cargo.lock version,
  clean candidate worktrees, annotated tag objects, and tag-to-HEAD agreement.
- Fixed the Tmux plugin rejecting real pane IDs such as `%17`; pane discovery now
  covers all windows, reports command failures, and selects panes by stable ID.
- Fixed the tokio-udev listener being monitor-only: add/remove/change events now
  update active keyboard streams without restarting the hotkey daemon.

### Security

- Exclusive raw evdev grabbing remains disabled unless the unsafe behavior is
  explicitly acknowledged, because unmatched keyboard events are not yet
  re-injected.
- The hotkey control socket is restricted to the current user and stale socket
  ownership is validated before replacement.
- The bar action socket is created below the private runtime directory with
  mode `0600`; requests are versioned, size-bounded, and timeout-bounded.
- Native X11 mode grabs only configured trigger chords, leaving unmatched input
  untouched and reporting grab conflicts before committing a new generation.
- sxhkd Cartesian brace expansion is capped at 4096 generated values to prevent
  accidental configuration blow-ups during migration.

[Unreleased]: https://github.com/fkr-0/deskhalloumi/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/fkr-0/deskhalloumi/compare/v0.3.3...v0.4.0
[0.3.3]: https://github.com/fkr-0/deskhalloumi/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/fkr-0/deskhalloumi/compare/v0.3.0...v0.3.2
[0.3.0]: https://github.com/fkr-0/deskhalloumi/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/fkr-0/deskhalloumi/tree/v0.2.0
