# DeskHalloumi hotkey architecture

## Purpose and invariants

DeskHalloumi provides one deterministic key engine that can run inside the bar
or in `deskhalloumi-hotkeyd`. The legacy `unilii-hotkeyd` executable is a
compatibility wrapper.

The architecture preserves these invariants:

1. one logical global-input owner per user session;
2. one process per managed external menu;
3. configuration reload commits only after the replacement listener is ready;
4. failed replacement restores the previous binding set where possible;
5. control and action IPC are user-scoped, versioned, and bounded;
6. unmatched input is untouched by the selective X11 backend.

## Component map

```text
TOML / sxhkdrc / menu defaults
              |
              v
      validation + i3 audit
              |
       +------+-------+
       |              |
       v              v
 generated i3     deskhalloumi-hotkeyd
 include          supervisor/control socket
       |              |
       v              v
 i3 passive       KeybindingDaemon
 grabs            +-----------------------+
                  | KeyEngine             |
                  | evdev or X11 backend  |
                  | action dispatcher     |
                  +-----+-----------+-----+
                        |           |
             shell/menu direct      | versioned action bus
                                    v
                              DeskHalloumi bar
                              tray/bar/widget state
```

## Core modules

### `key_engine.rs`

Renderer- and backend-independent state machine. It consumes normalized events:

```text
0 = release
1 = initial press
2 = repeat
```

It implements press, release, modifier-release with hold threshold, explicit
repeat, cooldown, priority ordering, specificity ordering, and consuming
bindings.

### `keys.rs`

Runtime adapter that parses human chords, selects a backend, feeds the engine,
and dispatches matches. Shell and managed-menu actions execute locally. Bar,
tray, and widget actions use the embedded channel when present or the action bus
when running standalone.

### `x11_hotkeys.rs`

Selective native X11 backend:

- derives trigger keycodes from the active X11 keyboard mapping;
- acquires passive grabs only for configured chords;
- covers CapsLock and discovered NumLock variants;
- normalizes left/right modifiers;
- reconstructs modifier state for the shared engine;
- reports binding/chord/keycode/modifier details for grab conflicts;
- emits repeat events from actual X11 key repetition.

A matching trigger is withheld from the focused client. Nonmatching input is not
grabbed.

### `i3_keybindings.rs` and `i3_config.rs`

The exporter renders ordinary press/release bindings as `bindsym` and
`bindsym --release`. The scanner recursively resolves allowed includes, expands
variables, tracks modes, parses `bindsym`/`bindcode`, and reports exact or
modifier-equivalent collisions. An unresolved include marks the audit
incomplete rather than silently declaring it clean.

### `action_bus.rs`

Versioned line-delimited JSON protocol for `shell`, `menu`, `bar`, `tray`, and
`widget` action identities. Standalone hotkeyd currently sends bar/tray/widget
actions to the bar; shell/menu execute locally. Requests and responses carry the
protocol version and request ID. Frames, command sizes, and read/write timeouts
are bounded.

Default socket:

```text
$XDG_RUNTIME_DIR/deskhalloumi/action.sock
```

The bar creates it with mode `0600` below a mode-`0700` runtime directory.

### `hotkey_control.rs`

Independent protocol version for daemon lifecycle operations:

```json
{"command":"ping"}
{"command":"status"}
{"command":"reload"}
{"command":"shutdown"}
{"command":"menu","action":"toggle:i3-vis"}
```

Status includes backend, generation, binding counts, source paths, managed menu
state, and the last reload error.

### `menu_process.rs`

Cross-process menu lifecycle manager with per-menu locks, PID/executable
verification, stale-record cleanup, child reaping, and idempotent show/hide/
toggle behavior. Zombie children are treated as stopped.

## Input ownership modes

### Generated i3 mode

Use for ordinary press/release actions. i3 owns the passive grabs. DeskHalloumi
generates and atomically replaces the included configuration after validation.

### Native X11 mode

Use for advanced engine semantics:

```sh
deskhalloumi-hotkeyd --backend x11 --config ~/.config/deskhalloumi/hotkeys.toml --watch
```

The supervisor owns the singleton and the worker owns the X11 connection/grabs.
On reload the old worker is stopped, the candidate attempts its grabs, and the
previous binding generation is restarted if readiness fails.

### Evdev observe mode

Useful for physical-key diagnostics and development. It sees raw device events
but cannot selectively prevent a matching key from reaching other clients.

### Unsafe evdev whole-device grab

Available only behind explicit acknowledgement. It suppresses the entire input
device because unmatched events are not reinjected. It is not a supported normal
session configuration.

## Embedded and standalone action flow

The bar always installs one typed action receiver. If it owns the embedded key
listener, the daemon sends directly through an in-process channel. If standalone
hotkeyd owns input, it connects to the private action socket. Missing or rejected
actions generate bounded errors for that invocation while unrelated bindings
remain active.

```text
standalone hotkeyd                 embedded daemon
        |                                 |
        | Unix action socket              | Tokio channel
        +---------------+-----------------+
                        v
             Message::KeybindingAction
                        |
                        v
                 Iced update loop
```

Widget commands use `<widget>:<action>` and currently support Wi-Fi, audio,
video/display, power, and system-monitor refresh targets. Some mutable bar module
operations remain explicit diagnostics; see `hotkey-action-matrix.md`.

## Runtime filesystem

Precedence:

```text
DESKHALLOUMI_RUNTIME_DIR
UNILII_RUNTIME_DIR                  # compatibility fallback
$XDG_RUNTIME_DIR/deskhalloumi
/tmp/deskhalloumi-<uid>
```

Typical layout:

```text
deskhalloumi/
├── hotkeyd.instance.json
├── hotkeyd.sock                    # lifecycle control, 0600
├── action.sock                     # bar action receiver, 0600
└── menus/
    ├── i3-vis.json
    ├── filter-tab.json
    └── copyq.json
```

## Reload sequence

1. Read and parse all configured sources.
2. Import sxhkd syntax and promote known menu commands.
3. Validate chords/actions and report duplicates, shadowing, and migration loss.
4. Reject invalid input without touching the current worker.
5. Stop the current worker while the supervisor retains singleton ownership.
6. Start the candidate and wait up to five seconds for readiness.
7. Commit only after device access or X11 grabs succeed.
8. On failure, stop the candidate and restore the previous bindings.
9. Record rollback details in status.

## Security boundary

Configuration is trusted: shell actions execute through `sh -c`. The Unix
sockets are not remote authentication protocols; their protection is private
filesystem ownership and permissions. Raw evdev access can observe all keyboard
input and should be granted narrowly. The X11 backend inherits the security
boundary of the user's X11 session.

## Tested boundary

`scripts/test_i3_hotkeys.sh` uses Xvfb plus a real i3 instance to verify generated
press/release bindings, atomic fail-closed export, X11 press/modifier-release,
repeat, cooldown, priority/consume, and focused-client trigger suppression. It
never touches the developer's live display.

## Remaining limitations

- Sway/Wayland requires a separately tested adapter; no parity claim is made.
- sxhkd line continuations, modes/chains, replay, synchronous execution, and
  nested expansions remain explicit exact/approximate/unsupported migration
  diagnostics.
- Some bar module mutation actions still require a runtime refactor.
- Managed-menu `show` is process-idempotent but does not force the window manager
  to raise an already visible window that lost focus.
