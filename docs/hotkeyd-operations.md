# Operating deskhalloumi-hotkeyd

## Choose one input owner

DeskHalloumi supports three deliberate topologies:

| Topology | Best for | Ownership |
|---|---|---|
| Generated i3 include | Ordinary press/release bindings | i3 passive grabs |
| Standalone `--backend x11` | Hold, modifier-release, repeat, cooldown, priority, consume | DeskHalloumi selective X11 grabs |
| Bar-embedded evdev | Legacy embedded setup and physical-key observation | Bar process |

Do not let i3, sxhkd, and native DeskHalloumi own the same chord concurrently.
The recursive scanner detects i3 collisions, but it cannot stop sxhkd for you.

## Build and install

```sh
cargo build --release -p deskhalloumi-bin \
  --bin deskhalloumi \
  --bin deskhalloumi-hotkeyd \
  --bin deskhalloumi-i3-vis \
  --bin deskhalloumi-filter-tab \
  --bin deskhalloumi-copyq \
  --bin unilii-hotkeyd

install -Dm755 target/release/deskhalloumi ~/.local/bin/deskhalloumi
install -Dm755 target/release/deskhalloumi-hotkeyd ~/.local/bin/deskhalloumi-hotkeyd
install -Dm755 target/release/deskhalloumi-i3-vis ~/.local/bin/deskhalloumi-i3-vis
install -Dm755 target/release/deskhalloumi-filter-tab ~/.local/bin/deskhalloumi-filter-tab
install -Dm755 target/release/deskhalloumi-copyq ~/.local/bin/deskhalloumi-copyq
```

The `unilii-*` binaries are small compatibility launchers. Install them beside
the corresponding `deskhalloumi-*` binaries only while old scripts still need
them.

## Configuration paths

Primary paths:

```text
~/.config/deskhalloumi/deskhalloumi.toml
~/.config/deskhalloumi/hotkeys.toml
~/.config/deskhalloumi/bar.toml
```

If the new main configuration does not exist, DeskHalloumi reads the legacy
`~/.config/unilii/unilii.toml`. It does not move or delete legacy files.

New environment variables win over old aliases:

```text
DESKHALLOUMI_RUNTIME_DIR > UNILII_RUNTIME_DIR
DESKHALLOUMI_BAR_CONFIG  > UNILII_BAR_CONFIG
DESKHALLOUMI_XRANDR_PRESETS_YAML > UNILII_XRANDR_PRESETS_YAML
```

## Starter hotkeys

```sh
mkdir -p ~/.config/deskhalloumi
deskhalloumi-hotkeyd --print-defaults \
  > ~/.config/deskhalloumi/hotkeys.toml
```

The built-in menu defaults use `Super+i`, `Super+u`, and `Super+c`. On the
workstation audited on 2026-07-18 all three collide with active i3 bindings, so
do not enable them unchanged. See `i3-active-audit-2026-07-18.md`.

A native-X11 configuration may use every action type:

```toml
[[keybindings]]
name = "terminal"
keysym = "Super+Return"
type = "shell"
command = "exec kitty"
priority = 50
consume = true

[[keybindings]]
name = "tray_toggle"
keysym = "Super+Shift+t"
type = "tray"
command = "toggle-menu"
priority = 80
consume = true

[[keybindings]]
name = "display_menu"
keysym = "Super+Shift"
type = "widget"
command = "video:toggle_menu"
trigger = "modrelease"
hold_ms = 120
cooldown_ms = 300
priority = 100
consume = true
```

Configuration is trusted input. Shell commands execute through `sh -c`.

## Validate and audit

Basic validation:

```sh
deskhalloumi-hotkeyd \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --dry-run
```

Validate against the complete active i3 configuration:

```sh
deskhalloumi-hotkeyd \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --audit-i3-config ~/.config/i3/config \
  --dry-run \
  --strict
```

Strict mode exits with code `3` for invalid bindings, migration loss, duplicate
or shadowed bindings, active i3 collisions, or an incomplete include audit.

The scanner resolves allowed includes, variables, modes, `bindsym`, and
`bindcode`, and reports source file and line. An unresolved dynamic include is
reported as incomplete rather than silently ignored.

## sxhkd migration

```sh
deskhalloumi-hotkeyd \
  --sxhkd ~/.config/sxhkd/sxhkdrc \
  --audit-i3-config ~/.config/i3/config \
  --dry-run \
  --strict
```

Simple comma-separated brace alternatives are expanded pairwise. Numeric
ranges, nested/malformed expansions, modes/chains, and replay semantics remain
explicit diagnostics when they cannot be represented exactly.

Known old and new menu commands are promoted to managed-menu actions:

```text
unilii-i3-vis / deskhalloumi-i3-vis
unilii-filter-tab / deskhalloumi-filter-tab
unilii-copyq / deskhalloumi-copyq
```

## Generated i3 backend

Use this for standard press/release actions:

```sh
deskhalloumi-hotkeyd \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --audit-i3-config ~/.config/i3/config \
  --write-i3-bindings ~/.config/deskhalloumi/i3-bindings.conf \
  --strict
```

Add once to the i3 config:

```text
include ~/.config/deskhalloumi/i3-bindings.conf
```

After that, regenerate and reload atomically:

```sh
deskhalloumi-hotkeyd \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --audit-i3-config ~/.config/i3/config \
  --write-i3-bindings ~/.config/deskhalloumi/i3-bindings.conf \
  --reload-i3 \
  --strict
```

A strict failure occurs before replacement, preserving the last-known-good
include.

## Native selective X11 backend

Run the bar without an embedded listener:

```sh
deskhalloumi run --no-hotkeyd
```

Then start the standalone service:

```sh
deskhalloumi-hotkeyd \
  --backend x11 \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --watch
```

The backend grabs only configured trigger chords and lock-state variants.
Matching triggers do not reach the focused client; unmatched keys remain
untouched. Grab conflicts fail readiness with the binding, chord, keycode, and
modifier mask. Transactional reload restores the previous binding generation if
the candidate cannot acquire its grabs.

`--grab` is not used with `--backend x11`; passive grabs are already selective.

## Action bus

Standalone shell and managed-menu actions execute in hotkeyd. Bar, tray, and
widget actions go to:

```text
$XDG_RUNTIME_DIR/deskhalloumi/action.sock
```

The bar creates the socket with mode `0600`. Requests are versioned and bounded.
If the bar is absent, only the invoked internal action fails after a short
bounded timeout; shell/menu bindings and the daemon remain active.

Supported widget targets:

```text
wifi:<action>
audio:<action>
video:<action>
display:<action>
power:<action>
sysmonitor:refresh
```

Some mutable bar module operations still produce explicit diagnostics; see
`hotkey-action-matrix.md`.

## systemd user service

```sh
install -Dm644 contrib/systemd/user/deskhalloumi-hotkeyd.service \
  ~/.config/systemd/user/deskhalloumi-hotkeyd.service
systemctl --user daemon-reload
systemctl --user disable --now unilii-hotkeyd.service 2>/dev/null || true
systemctl --user enable --now deskhalloumi-hotkeyd.service
```

Do not enable both units; they conflict intentionally.

Operations:

```sh
systemctl --user status deskhalloumi-hotkeyd.service
journalctl --user -u deskhalloumi-hotkeyd.service -f
systemctl --user reload deskhalloumi-hotkeyd.service
systemctl --user restart deskhalloumi-hotkeyd.service
systemctl --user stop deskhalloumi-hotkeyd.service
```

Rollback instructions are in `project-renaming.md`.

## Control commands

```sh
deskhalloumi-hotkeyd --ping
deskhalloumi-hotkeyd --status
deskhalloumi-hotkeyd --status --json
deskhalloumi-hotkeyd --reload
deskhalloumi-hotkeyd --shutdown
```

Managed menu lifecycle:

```sh
deskhalloumi-hotkeyd --menu-action show:i3-vis
deskhalloumi-hotkeyd --menu-action hide:i3-vis
deskhalloumi-hotkeyd --menu-action toggle:i3-vis
```

Reload reuses the original source paths. Restart to change the source set or
upgrade the supervisor executable.

## Runtime paths

Default:

```text
$XDG_RUNTIME_DIR/deskhalloumi/hotkeyd.sock
$XDG_RUNTIME_DIR/deskhalloumi/hotkeyd.instance.json
$XDG_RUNTIME_DIR/deskhalloumi/action.sock
$XDG_RUNTIME_DIR/deskhalloumi/menus/*.json
```

Override the complete root:

```sh
DESKHALLOUMI_RUNTIME_DIR=/run/user/$UID/my-deskhalloumi \
  deskhalloumi-hotkeyd --backend x11 ...
```

The runtime root is mode `0700`; sockets are mode `0600`.

## Evdev observe and unsafe grab modes

The default evdev backend observes physical events and therefore cannot prevent
a matching key from reaching other clients. It commonly requires `/dev/input`
permissions.

The whole-device grab escape hatch:

```sh
deskhalloumi-hotkeyd \
  --backend evdev \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --grab \
  --allow-unsafe-evdev-grab
```

is development-only. It suppresses the entire keyboard because unmatched events
are not reinjected.

## Troubleshooting

### A hotkey runs twice

Check:

```sh
pgrep -a sxhkd
deskhalloumi-hotkeyd --status
deskhalloumi-hotkeyd \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --audit-i3-config ~/.config/i3/config
```

The singleton prevents two DeskHalloumi listeners, not duplicate ownership by
i3 or sxhkd.

### X11 grab conflict

Remove or change the existing owner, then reload. With i3, use the audit to find
the exact source line. Avoid forcing a new generation while the old owner is
still active.

### Action receiver unavailable

Start the bar with `deskhalloumi run --no-hotkeyd`. Check that
`$XDG_RUNTIME_DIR/deskhalloumi/action.sock` exists and is owned by the current
user.

### Reload rejected or rolled back

```sh
deskhalloumi-hotkeyd --status --json | jq .status.last_reload_error
deskhalloumi-hotkeyd \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --dry-run --strict
```

The previous generation remains active when restoration succeeds.

## Verification

```sh
cargo fmt --all -- --check
python3 scripts/check_release_metadata.py
scripts/test_safe.sh
scripts/test_i3_hotkeys.sh
CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
```

The isolated hotkey script starts Xvfb, real i3, xdotool, and xev without
connecting to or reloading the live desktop session.
