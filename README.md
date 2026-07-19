# DeskHalloumi

[![CI](https://github.com/fkr-0/deskhalloumi/actions/workflows/ci.yml/badge.svg)](https://github.com/fkr-0/deskhalloumi/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/fkr-0/deskhalloumi?label=release)](https://github.com/fkr-0/deskhalloumi/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

DeskHalloumi is a Rust desktop-control workspace built around Iced, Tokio,
DBus/StatusNotifier integration, configurable menus, a panel, global hotkeys,
and small plugin crates. It was previously named `unilii`; compatibility
commands and config fallbacks remain available during the pre-1.0 migration.

Canonical repository: <https://github.com/fkr-0/deskhalloumi>

## Release 0.3.0

`v0.3.0`, released on July 19, 2026, introduces the supervised asynchronous
runtime, generation-safe typed provider state, renderer-neutral menus and
quick-select, live runtime diagnostics, CLI introspection, and bounded visible
action history. The existing i3/X11, configuration-path, application-ID, and
`unilii-*` compatibility contracts remain available.

- [Release notes](docs/releases/0.3.0.md)
- [Complete changelog](CHANGELOG.md)
- [Versioning and release policy](docs/versioning.md)
- [Binary release and source tag `v0.3.0`](https://github.com/fkr-0/deskhalloumi/releases/tag/v0.3.0)
- [Installation and upgrade guide](docs/installation.md)

Clone the repository with:

```sh
git clone git@github.com:fkr-0/deskhalloumi.git
cd deskhalloumi
```

Pushing an annotated version tag runs the release workflow, which validates the
tag, builds the primary and compatibility binaries, creates a deterministic
Linux archive and checksum, and attaches both files to a durable GitHub Release.
The same files are retained temporarily as a GitHub Actions artifact. Crates are
not published automatically.

## Workspace layout

```text
.
├── Cargo.toml                  # workspace manifest
├── CONFIGURATION.md            # menu, module, and app configuration notes
├── KEYBINDINGS.md              # keybinding and sxhkd import notes
├── CHANGELOG.md                # user-visible unreleased and released changes
├── roadmap.yml                 # internal maintainer roadmap and release horizons
├── todo.yml                    # focused remaining hotkey/rename/release work
├── tasks.yml                   # current review/implementation task state
└── unilii/
    ├── Cargo.toml              # small top-level library package
    ├── bin/                    # Iced status bar binary and app wiring
    │   └── src/
    │       ├── main.rs         # current bootstrap/wiring; being split incrementally
    │       ├── app.rs          # app messages/state types
    │       ├── update/         # extracted update helpers
    │       ├── tray.rs         # tray parsing and legacy tray helpers
    │       ├── enhanced_tray/  # StatusNotifier/DBus menu model, state, and rendering
    │       ├── menus/          # network, mount, calendar, custom menu models
    │       └── widgets/        # audio, video, WiFi, power, system widgets
    ├── core/                   # config, keybinding, key engine, sxhkd import
    ├── lib/                    # shared utilities such as CalDAV/cache/input/process helpers
    └── plugins/
        ├── Clock/
        ├── Battery/
        └── Tmux/
```

Ignored local review/reference directories such as `iced_examples/` and `wiUp/`
may exist in developer checkouts. They are not part of the canonical repository.


## Hotkey service and managed menus

The global hotkey subsystem can run inside the bar or as the standalone
`deskhalloumi-hotkeyd` supervisor. The standalone topology provides a private Unix
control socket, transactional reload, file watching, structured status, and
cross-process single-instance menu lifecycle management.

Documentation:

- [Documentation index](docs/README.md)
- [Installation, upgrade, and rollback](docs/installation.md)
- [Async runtime and subprocess policy](docs/async-runtime.md)
- [Hotkey architecture](docs/hotkeyd-architecture.md)
- [Hotkey operation and troubleshooting](docs/hotkeyd-operations.md)
- [i3/sxhkd replacement feasibility and migration](docs/hotkeyd-i3-feasibility.md)
- [Action type/topology matrix](docs/hotkey-action-matrix.md)
- [Runnable configurations](examples/hotkeyd/)

Menu documentation:

- [Shared menu design system and runtime architecture](docs/menu-design-system.md)
- [Clickable system-menubar architecture and operation](docs/system-menubar.md)
- [Complete menu configuration example](examples/system-menubar/unilii.toml)

Recommended topology:

```sh
deskhalloumi run --no-hotkeyd
deskhalloumi-hotkeyd --config ~/.config/deskhalloumi/hotkeys.toml --watch
```

For ordinary i3/X11 press/release bindings, prefer generating an i3 include so
i3 owns selective passive grabs without raw input-device permissions:

```sh
deskhalloumi-hotkeyd \
  --sxhkd ~/.config/sxhkd/sxhkdrc \
  --write-i3-bindings ~/.config/deskhalloumi/i3-bindings.conf \
  --strict
```

## What currently works

The active implementation includes:

- Iced-based bar/window wiring with X11 and Wayland features enabled through the workspace Iced dependency.
- StatusNotifier/tray parsing, DBus menu conversion, enhanced tray state, favorites, filtering, submenu navigation, and extracted update helpers.
- Release-oriented shared menu design system for DBus tray menus, aggregated search, favorites, Wi-Fi, storage, calendar, system controls, and custom launcher entries.
- Widgets for audio, video/display presets, WiFi, power, and system monitoring.
- Core keybinding support, dry-run tests, sxhkd import with simple brace
  and range expansion, recursive active-i3 conflict auditing, safe i3 include
  generation, dynamic evdev keyboard hot-plug, and a selective native X11
  backend for advanced shortcut semantics.
- A versioned user-scoped action bus connecting standalone hotkeys to bar, tray,
  and widget actions.
- Plugin crates for clock, battery, and tmux modules.
- CalDAV/calendar cache helpers in `unilii/lib`.

### Session support boundary

| surface | i3/X11 | Sway/Wayland |
| --- | --- | --- |
| Iced bar and popup rendering | supported | rendering may work through Iced |
| generated global press/release shortcuts | supported through i3 | no parity claim |
| advanced global hold/mod-release/repeat shortcuts | supported through the selective X11 backend | unsupported |
| active shortcut collision audit | supported for recursive i3 configuration | unsupported |
| xrandr/xset-oriented system actions | supported | must be overridden explicitly |

DeskHalloumi does not present X11 hotkey behavior as Sway parity. A separately
tested Wayland adapter is required before global shortcut support can be called
portable.

Some architecture is still intentionally transitional. In particular, `unilii/bin/src/main.rs` still owns too much wiring and is being split into tested modules under tasks tracked in `tasks.yml`.

## Building and running

Prebuilt Linux x86-64 binaries are available from the
[GitHub Releases page](https://github.com/fkr-0/deskhalloumi/releases). Verify
the accompanying SHA-256 file before installation. See
[the installation guide](docs/installation.md) for user-local, system-wide,
systemd, upgrade, rollback, and removal instructions.

Install a recent Rust toolchain, then build the workspace from the repository root:

```sh
cargo build --workspace
```

Build the GUI binary in release mode with:

```sh
cargo build -p deskhalloumi-bin --release
```

The binary is produced under Cargo’s workspace target directory. Run it with Cargo during development:

```sh
cargo run -p deskhalloumi-bin --bin deskhalloumi -- --help
cargo run -p deskhalloumi-bin --bin deskhalloumi -- run
```

See `CONFIGURATION.md` and `KEYBINDINGS.md` for configuration and input examples.

## Test and lint commands

The current verified baseline is:

```sh
cargo check --workspace
scripts/test_safe.sh
scripts/test_i3_hotkeys.sh
CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
```

`scripts/test_safe.sh` is the default full test path for local development and CI because it runs the live-session command audit before `cargo test --workspace`. Direct `cargo test --workspace` remains useful for expert/debug runs, but do not use it as the default handoff gate when desktop/session command paths changed.

`CARGO_INCREMENTAL=0` is currently recommended for clippy because one incremental Clippy/rustc internal compiler error was observed during recent verification, while the non-incremental clippy gate passed.

Focused examples:

```sh
cargo test -p deskhalloumi-bin update::enhanced_tray_events::tests::enhanced_tray_event_updates_existing_tree_and_menu -- --nocapture
cargo test -p deskhalloumi-bin tray::tests::icon_label_uses_known_icon_keywords -- --nocapture
cargo test -p deskhalloumi-lib calendar::caldav::tests::normalizes_ics_datetime_values_to_utc_shape -- --nocapture
```

## Development workflow

Use `roadmap.yml` for release horizons and architecture direction, `todo.yml`
for focused known gaps, and `tasks.yml` for detailed implementation evidence.
The current workflow is test-first:

1. Reproduce the bug or missing behavior with a focused failing test or lint/doc check.
2. Make the smallest implementation change that turns the test green.
3. Run the focused test, `cargo check --workspace`, non-incremental clippy, and `scripts/test_safe.sh` when the slice touches production code.
4. Update `tasks.yml` with completion evidence, new subtasks, and refinements to remaining work.

For new behavior, prefer pure state/model tests over live DBus, live NetworkManager, or GUI-window tests. Add live smoke tests only after the pure behavior boundary is stable. See `CONTRIBUTING.md` for the safe testing policy and audit annotations.

Release metadata is governed by [the versioning policy](docs/versioning.md) and
validated with `python3 scripts/check_release_metadata.py`. A prepared release
commit can be checked with `--candidate`; a tagged release must pass
`--release --require-clean`. The tag-triggered workflow accepts annotated tags
only, reruns every release gate, and publishes a deterministic Linux archive
plus SHA-256 checksum as GitHub Release assets without publishing crates
automatically. DeskHalloumi's
compatibility contract, path precedence, systemd transition, and name-screening
limits are documented in [the rename plan](docs/project-renaming.md).

## Current limitations

- `main.rs` is still too large and is being split incrementally.
- Some transitional modules carry local `FIXME(T6)` dead-code allowances until the tray/menu architecture is fully consolidated.
- Enhanced tray events now have module-level update tests, but a full Iced daemon/update-path integration test is still pending.

## License

DeskHalloumi is distributed under the [MIT License](LICENSE). Third-party
dependencies and any separately obtained local reference material retain their
own licenses.

## System menubar

See [docs/system-menubar.md](docs/system-menubar.md).
