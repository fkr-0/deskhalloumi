# Changelog

All notable user-visible changes to the active DeskHalloumi workspace are
recorded in
this file. The project follows Semantic Versioning as described in
[`docs/versioning.md`](docs/versioning.md).

The workspace declares version `0.2.0`. The release preparation commit does not
create the corresponding annotated `v0.2.0` tag; tagging remains a deliberate
maintainer action after review.

## [Unreleased]

## [0.2.0] - 2026-07-18

### Added

- Standalone `deskhalloumi-hotkeyd` supervision with a user-scoped control socket,
  status/ping/reload/shutdown commands, file watching, and transactional worker
  reload with rollback.
- Deterministic press, release, modifier-release, repeat, cooldown, priority,
  and consume semantics in the keybinding engine.
- Shadow-mode and strict migration diagnostics for invalid, duplicate, and
  shadowed keybindings.
- sxhkd configuration import with release-prefix support and pairwise expansion
  of simple comma-separated chord/command braces.
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

### Changed

- Unsupported sxhkd ranges, malformed/nested expansions, and mode/chord chains
  are now skipped with explicit diagnostics instead of being imported as
  literal nonfunctional chords.
- sxhkd replay bindings are imported only with an explicit warning that replay
  semantics are not preserved.
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
