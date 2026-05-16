# unilii

`unilii` is a Rust status-bar and tray workspace built around Iced, Tokio, DBus/StatusNotifier integration, configurable menus, keybindings, and small plugin crates. The current tree is no longer the early single-crate X11 rewrite described by older docs; it is a Cargo workspace with reusable core/library crates, a GUI binary, and plugins.

## Workspace layout

```text
.
├── Cargo.toml                  # workspace manifest
├── CONFIGURATION.md            # menu, module, and app configuration notes
├── KEYBINDINGS.md              # keybinding and sxhkd import notes
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

The root directory also contains review artifacts and external/reference material in the current dirty working tree. Treat directories such as `iced_examples/` and `wiUp/` as non-canonical unless a task explicitly says otherwise.

## What currently works

The active implementation includes:

- Iced-based bar/window wiring with X11 and Wayland features enabled through the workspace Iced dependency.
- StatusNotifier/tray parsing, DBus menu conversion, enhanced tray state, favorites, filtering, submenu navigation, and extracted update helpers.
- Configurable menu models for network, mount, calendar, and custom launcher/menu entries.
- Widgets for audio, video/display presets, WiFi, power, and system monitoring.
- Core keybinding support, dry-run keybinding tests, and sxhkd import helpers.
- Plugin crates for clock, battery, and tmux modules.
- CalDAV/calendar cache helpers in `unilii/lib`.

Some architecture is still intentionally transitional. In particular, `unilii/bin/src/main.rs` still owns too much wiring and is being split into tested modules under tasks tracked in `tasks.yml`.

## Building and running

Install a recent Rust toolchain, then build the workspace from the repository root:

```sh
cargo build --workspace
```

Build the GUI binary in release mode with:

```sh
cargo build -p unilii-bin --release
```

The binary is produced under Cargo’s workspace target directory. Run it with Cargo during development:

```sh
cargo run -p unilii-bin -- --help
cargo run -p unilii-bin -- run
```

See `CONFIGURATION.md` and `KEYBINDINGS.md` for configuration and input examples.

## Test and lint commands

The current verified baseline is:

```sh
cargo check --workspace
cargo test --workspace
CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
```

`CARGO_INCREMENTAL=0` is currently recommended for clippy because one incremental Clippy/rustc internal compiler error was observed during recent verification, while the non-incremental clippy gate passed.

Focused examples:

```sh
cargo test -p unilii-bin update::enhanced_tray_events::tests::enhanced_tray_event_updates_existing_tree_and_menu -- --nocapture
cargo test -p unilii-bin tray::tests::icon_label_uses_known_icon_keywords -- --nocapture
cargo test -p unilii-lib calendar::caldav::tests::normalizes_ics_datetime_values_to_utc_shape -- --nocapture
```

## Development workflow

Use `tasks.yml` as the source of truth for current priorities. The current workflow is test-first:

1. Reproduce the bug or missing behavior with a focused failing test or lint/doc check.
2. Make the smallest implementation change that turns the test green.
3. Run the focused test, `cargo check --workspace`, non-incremental clippy, and `cargo test --workspace` when the slice touches production code.
4. Update `tasks.yml` with completion evidence, new subtasks, and refinements to remaining work.

For new behavior, prefer pure state/model tests over live DBus, live NetworkManager, or GUI-window tests. Add live smoke tests only after the pure behavior boundary is stable.

## Current limitations

- `main.rs` is still too large and is being split incrementally.
- Some transitional modules carry local `FIXME(T6)` dead-code allowances until the tray/menu architecture is fully consolidated.
- Enhanced tray events now have module-level update tests, but a full Iced daemon/update-path integration test is still pending.
- The root contains untracked/reference material that should not be treated as canonical source without checking `tasks.yml`.

## License

The workspace manifests currently declare `MIT` for the active workspace packages. Check the package manifests and any imported/reference material before redistributing artifacts from the dirty working tree.
