# DeskHalloumi repository guidance

## Scope

DeskHalloumi is the active Rust/Iced desktop-control workspace in this
repository. The physical `unilii/` source directory is retained for migration
stability, but public package and command identities use `deskhalloumi-*`.

Do not treat ignored local reference directories such as `iced_examples/` or
`wiUp/` as canonical source.

## Active workspace

```text
Cargo.toml
unilii/
├── Cargo.toml
├── bin/                 # primary GUI/CLI binaries
├── core/                # config, hotkeys, i3/X11 integration, action bus
├── lib/                 # input/system/calendar helpers
└── plugins/             # Clock, Battery, Tmux
```

Primary binaries:

```text
deskhalloumi
deskhalloumi-bar
deskhalloumi-copyq
deskhalloumi-filter-tab
deskhalloumi-i3-vis
deskhalloumi-hotkeyd
```

`unilii-*` binaries are compatibility launchers. Do not add new behavior only
to a compatibility launcher.

## Required workflow

1. Read `README.md`, `todo.yml`, and any relevant document under `docs/`.
2. Add or update focused tests before broad refactors.
3. Keep desktop/session side effects out of ordinary tests.
4. Run the narrowest relevant test first.
5. Before handoff, run the release gates appropriate to the change.

Canonical gates:

```sh
cargo fmt --all -- --check
python3 scripts/check_release_metadata.py
scripts/test_safe.sh
scripts/test_i3_hotkeys.sh
CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
```

For a prepared release commit:

```sh
python3 scripts/check_release_metadata.py --candidate
```

For an annotated release tag checked out at `HEAD`:

```sh
python3 scripts/check_release_metadata.py --release --require-clean
```

## Safety rules

- Never run live session-mutating commands from unit tests.
- Use fixtures or harmless command overrides for i3, Sway, NetworkManager,
  audio, power, logout, reboot, shutdown, or display-layout behavior.
- `scripts/test_safe.sh` runs the live-session command audit before workspace
  tests.
- `scripts/test_i3_hotkeys.sh` is allowed to run i3 commands only because it
  creates a private Xvfb display and replaces `HOME`, `DISPLAY`,
  `XDG_CONFIG_HOME`, and `XDG_RUNTIME_DIR`.
- Raw evdev exclusive grabbing is unsafe unless explicitly acknowledged. Prefer
  generated i3 bindings or the selective X11 backend.
- Shell actions are trusted configuration input; preserve quoting and size/
  timeout boundaries on IPC paths.

## Compatibility contract

Primary paths:

```text
~/.config/deskhalloumi/
$XDG_RUNTIME_DIR/deskhalloumi/
```

Legacy config paths, environment variables, commands, and systemd units remain
readable/usable during the 0.x migration. New values take precedence over
legacy values.

Legacy application IDs such as `unilii-copyq`, `unilii-filter-tab`, and
`unilii-i3-vis` remain stable in 0.2.0 so existing window-manager rules do not
silently break. Visible titles and CLI names use DeskHalloumi branding.

## Platform boundary

- i3/X11 global shortcuts are supported and tested.
- Advanced hold/mod-release/repeat semantics use the selective X11 backend.
- Sway/Wayland global-hotkey parity is not claimed.
- Iced rendering may work on Wayland, but that does not imply hotkey, xrandr,
  xset, or i3 integration parity.

## Code conventions

- Keep shared semantics in `deskhalloumi-core`; keep Iced adapters in
  `unilii/bin`.
- Prefer pure model/state tests over GUI or live-service tests.
- Use structured errors with actionable context.
- Use `tracing` for runtime diagnostics.
- Keep async tasks bounded; avoid blocking external commands on UI paths where
  a non-blocking alternative is practical.
- Preserve the workspace version via `version.workspace = true` for all
  first-party packages.
- Do not add a nested `[workspace]` section to member manifests.

## Release policy

`CHANGELOG.md` follows Keep a Changelog-style sections. Release tags are
annotated and named `vMAJOR.MINOR.PATCH`. The tag-triggered workflow validates
that the tag points at `HEAD`, reruns all gates, and creates a deterministic
Linux archive with a SHA-256 checksum. It does not publish crates or create a
public release automatically.
