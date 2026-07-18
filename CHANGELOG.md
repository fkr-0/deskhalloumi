# Changelog

All notable user-visible changes to the active DeskHalloumi workspace are
recorded in this file. The project follows Semantic Versioning as described in
[`docs/versioning.md`](docs/versioning.md).

Version `0.2.0` is identified by the annotated `v0.2.0` release tag. Changes
after that tag belong under `[Unreleased]`.

## [Unreleased]

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

### Fixed

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

[Unreleased]: https://github.com/fkr-0/deskhalloumi/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/fkr-0/deskhalloumi/tree/v0.2.0
