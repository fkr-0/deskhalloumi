# Hotkey action types and integration matrix

This document defines where each `[[keybindings]]` action type executes and what
behavior is currently implemented. It is the authoritative boundary between the
standalone `deskhalloumi-hotkeyd` topology and the bar-embedded topology. The
legacy `unilii-hotkeyd` command invokes the same implementation.

## Topology matrix

| Action type | Standalone hotkeyd | Bar-embedded daemon | Execution location |
|---|---:|---:|---|
| `shell` | Yes | Yes | Child process started by `KeybindingDaemon` |
| `menu` | Yes | Yes | Shared `MenuProcessManager` |
| `tray` | Yes, while the bar action receiver is running | Yes | Versioned action bus or embedded channel |
| `bar` | Yes, while the bar action receiver is running | Yes | Versioned action bus or embedded channel |
| `widget` | Yes, while the bar action receiver is running | Yes | Versioned action bus or embedded channel |

Standalone mode sends `bar`, `tray`, and `widget` actions to the private
user-scoped action socket. A missing receiver produces a bounded per-action
error; it does not invalidate unrelated shell or menu bindings.

The action protocol is line-delimited JSON with an explicit protocol version,
request ID, validated action kind, bounded frame size, and a matching response.
The socket is created with mode `0600` below the private runtime directory.

## Single input path in embedded mode

Embedded mode uses exactly one evdev reader:

```text
evdev -> KeybindingDaemon -> KeyEngine
                         |-> matching binding execution
                         `-> RawKeyEvent -> Iced action subscription
```

The bar no longer opens a separate raw evdev subscription. The embedded daemon
forwards every press, repeat, and release event through `KeybindingResult::RawKeyEvent`.
The Iced update loop uses those events for global tray navigation and shift-digit
tray shortcuts.

When the standalone daemon owns input, the bar still installs the typed action
receiver but does not open a second global keyboard listener.

## `shell`

```toml
[[keybindings]]
name = "terminal"
keysym = "Super+Return"
type = "shell"
command = "exec foot"
```

The command runs through:

```text
sh -c <command>
```

Children are reaped asynchronously. Configuration is trusted input; no shell
escaping or sandbox is applied.

## `menu`

```toml
[[keybindings]]
name = "i3_vis"
keysym = "Super+i"
type = "menu"
command = "toggle:i3-vis"
```

Supported verbs:

```text
show:<name>
hide:<name>
toggle:<name>
```

Known names:

```text
i3-vis
filter-tab
copyq
```

Menu actions use the cross-process PID registry, per-menu operation lock, stale
record verification, and child reaper. They are safe from either topology and
from the control CLI.

## `tray`

Tray actions require a running bar receiver because they modify Iced state. They
may originate from either the embedded daemon or standalone hotkeyd.

### Window lifecycle

| Command | Aliases | Behavior |
|---|---|---|
| `open-menu` | `menu:open`, `tray:open` | Reopen existing tray state or open the first tray icon |
| `close-menu` | `menu:close`, `tray:close` | Hide state and close the tray window |
| `toggle-menu` | `menu:toggle`, `tray:toggle` | Toggle the tray window |

### Views

| Command | Aliases | Behavior |
|---|---|---|
| `show-aggregated` | `aggregated`, `tray:aggregated` | Build/show aggregated state and open the tray window |
| `show-favorites` | `favorites`, `tray:favorites` | Build/show favorites state and open the tray window |

When no enhanced tray state exists, the bar initializes one from currently
known tray icons. Menu item data may still be empty until each application's
menu has been fetched.

### Navigation

| Command | Aliases | Behavior |
|---|---|---|
| `focus-next` | `next`, `tray:next` | Select next visible item |
| `focus-previous` | `previous`, `prev`, `tray:previous` | Select previous visible item |
| `activate-selected` | `select`, `tray:activate` | Trigger selected item |
| `open-index:N` | `tray:index:N` | Open zero-based tray icon index `N` |

Example:

```toml
[[keybindings]]
name = "tray_toggle"
keysym = "Super+Shift+t"
type = "tray"
command = "toggle-menu"
priority = 90
consume = true

[[keybindings]]
name = "tray_next"
keysym = "Super+Shift+j"
type = "tray"
command = "focus-next"
priority = 90
consume = true
```

### Refresh

`refresh-status` (`refresh`, `tray:refresh`) refreshes the current Network,
Mount, or Calendar special view. Generic DBus-menu and aggregated views do not
yet have one common refresh operation; the bar logs a warning instead of
silently doing nothing.

Unknown tray commands are represented as `Raw` and logged as unsupported.

## `bar`

The action channel, protocol, and parser are wired, but the current module runtime cannot
safely mutate loaded modules or reconstruct the complete bar in place.

Recognized commands:

```text
reload-config
config:reload
bar:reload

toggle-module:<name>
bar:toggle:<name>

focus-module:<name>
bar:focus:<name>
```

Current behavior is an explicit warning describing the unavailable operation.
Do not depend on these actions for production workflows yet. Use process/service
restart for configuration reload and normal module configuration for visibility.

The commands cross process boundaries correctly; unsupported mutable runtime
operations still return explicit diagnostics.

## `widget`

Widget actions use `<widget>:<action>`. Supported targets are `wifi`, `audio`,
`video`/`display`, `power`, and `sysmonitor:refresh`. Unknown targets are
diagnosed explicitly.

## Raw event forwarding

`RawKeyEvent` is an internal `KeybindingResult` variant, not a TOML action type.
It forwards:

```text
code: normalized engine key name, e.g. KEY_LEFTSHIFT
value: 0 release, 1 press, 2 repeat
```

The bar uses it to:

- maintain global shift state;
- navigate an open enhanced tray with arrows, Tab, Enter, and Escape;
- activate shift-digit tray icon shortcuts.

Raw events are sent before the key engine processes matching bindings, ensuring
state such as modifier release is observed in event order.

## Priority and consumption

All action types use the same `KeyEngine` conflict rules:

1. higher `priority` first;
2. more-specific chord first;
3. configuration order as final tie-break;
4. `consume = true` suppresses remaining lower candidates for the event.

Consumption controls DeskHalloumi binding dispatch. Evdev observe mode cannot
suppress physical input. The selective X11 backend uses passive grabs, so the
matching trigger key is withheld from the focused client while unmatched input
remains untouched.

## Recommended division

Use standalone X11 mode for desktop-global actions with selective suppression:

```text
shell + menu + tray + bar + widget
```

Use embedded mode when global keys must manipulate the bar's own tray state:

```text
shell + menu + tray
```

Use the generated i3 backend for ordinary press/release actions and the native
X11 backend for hold, modifier-release, explicit repeat, cooldown, priority, and
consumption semantics.
