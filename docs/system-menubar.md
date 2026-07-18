# Configurable system menubar

The system menubar is the release-oriented replacement for the older inline Wi-Fi, display, power, and system-monitor widgets. It renders one configurable row of clickable buttons and opens each domain through the same popup/menu state machine used by the enhanced tray.

## Goals

The implementation is designed around these invariants:

1. **One popup engine.** Wi-Fi, displays, statistics, shortcuts, power/session actions, and user-defined actions use one popup window and one keyboard-navigation model.
2. **No destructive unit-test side effects.** Tests never invoke suspend, reboot, shutdown, or radio changes.
3. **Configuration controls presentation and commands.** Button order, labels, visible sections, xrandr presets, session commands, idle-policy commands, confirmations, timeouts, and extra actions are configurable.
4. **Information and actions are distinct.** Informational rows and separators are not keyboard-selectable and cannot be activated accidentally.
5. **Destructive actions are confirmation-aware.** Logout, reboot, shutdown, and confirmed custom actions enter a confirmation view before command execution.
6. **Commands are asynchronous and bounded.** Shell actions use the shared `ActionRunner`, capture output/errors, and respect `command_timeout_ms`.
7. **Untrusted network names are shell-quoted.** SSIDs and saved profile names cannot become shell syntax when passed to `nmcli`.

## Architecture

```text
Config
  ├─ menus.system
  ├─ menus.wifi
  ├─ keybindings
  └─ xrandr preset YAML
          │
          ▼
menubar button row
          │ click
          ▼
SystemMenuRuntime + build_system_menu()
          │
          ▼
EnhancedTrayState / popup window
          │
          ├─ internal typed action
          ├─ NetworkManager snapshot/action
          ├─ managed-menu or hotkey action
          └─ bounded shell command via ActionRunner
```

### Main components

| File | Responsibility |
| --- | --- |
| `unilii/core/src/config.rs` | Serializable configuration, defaults, validation, and fallback behavior. |
| `unilii/bin/src/menus/system.rs` | Pure system-menu model, section construction, confirmation model, shortcut rows, and typed internal action parsing. |
| `unilii/bin/src/main.rs` | Popup lifecycle, Iced message routing, command execution, Wi-Fi snapshot refresh, and clickable rendering. |
| `unilii/bin/src/widgets/sysmonitor.rs` | Native CPU, memory, load, uptime, and root-filesystem snapshot collection. |
| `unilii/bin/src/widgets/wifi.rs` | Compact Wi-Fi state and escaped `nmcli -t` parsing. |
| `unilii/bin/src/widgets/video.rs` | xrandr output state and named preset loading. |
| `unilii/bin/src/widgets/power.rs` | X11 idle-policy state detection used by the compact power section. |
| `unilii/bin/src/action_runner.rs` | Timeout-bounded asynchronous process execution with stdout/stderr and exit metadata. |

## Popup and state lifecycle

A configured button opens either the root menu or one named section. Clicking the same open button closes the popup. Clicking another system button replaces the current system-menu view without spawning a second popup.

The synthetic system menu is registered inside `EnhancedTrayState` with the stable application ID `unilii-system-menu`. It is intentionally not a DBus StatusNotifier item. Tray refreshes preserve this synthetic state rather than treating it as a vanished application.

Keyboard selection uses visible row indices but skips:

- disabled information rows;
- separators;
- actions disabled by an empty configured command.

Mouse and keyboard activation follow the same `TrayMenuTriggered` route. Submenu actions remain open; real tray actions may close according to their existing action semantics.

## Sections

### Wi-Fi

The compact button shows the connected SSID, disabled state, or disconnected state. The section provides:

- current radio/connection state;
- a detailed available/known-network view;
- enable or disable Wi-Fi;
- rescan;
- configurable settings command.

The detailed view uses `menus.wifi`:

- `max_network_rows` limits available and known rows;
- `show_known_networks` controls saved profile visibility;
- `allow_forget` exposes profile deletion;
- `settings_command` controls the settings launcher;
- `scan_on_open` and `connect_timeout_ms` remain available to the NetworkManager path.

Network names are passed through single-quote shell escaping. Available-network clicks run `nmcli device wifi connect`; known-profile clicks run `nmcli connection up id`; forget controls run `nmcli connection delete`.

### Displays / xrandr

The button and section show currently detected outputs, modes, and primary status. Named presets are loaded from `menus.system.xrandr_presets_yaml`. Each preset may be represented as a shell command or an xrandr argument list.

Preset argument lists are shell-quoted when converted to the bounded action path. Applying a preset refreshes display state and reports command failure in the popup.

### System statistics

Statistics are collected without shell pipelines where practical:

- CPU utilization from deltas in `/proc/stat`;
- memory utilization from `MemTotal` and `MemAvailable`;
- load averages from `/proc/loadavg`;
- uptime from `/proc/uptime`;
- root-filesystem utilization from `df -P /`.

The compact label shows CPU and memory percentages. The section shows all fields and can launch `stats_command`, normally a terminal system monitor.

### Shortcut table

The shortcut table is generated from the active `keybindings` array. Rows are sorted by chord and name and show:

- binding name;
- command type (`shell`, `menu`, `tray`, `bar`, or `widget`);
- key chord;
- non-default trigger semantics such as release, modifier release, or repeat.

Rows are clickable and dispatch the same action semantics as the original binding:

- shell bindings run through the bounded action runner;
- managed menu bindings use `MenuProcessManager`;
- tray and bar actions use the embedded action bus;
- unsupported widget actions are visible but disabled.

The table is capped by `shortcut_limit`.

### Session and power

The section includes:

- enable or disable X11 screen blanking/DPMS after inactivity;
- lock session;
- suspend;
- log out;
- restart;
- shut down.

The default idle-enable command sets explicit timeouts instead of merely toggling an existing zero timeout:

```sh
xset s 600 600 +dpms dpms 0 0 900
```

The default disable command is:

```sh
xset s off -dpms
```

Logout, restart, and shutdown use the confirmation view when `confirm_destructive = true`. An empty configured command renders the corresponding action disabled.

### Extra actions

`[[menus.system.extra_items]]` adds arbitrary clickable actions to the `extra` section. IDs must be unique; title and command must be non-empty. Each item may have a shortcut hint and its own confirmation requirement.

## Configuration reference

### `[menus.system]`

| Key | Default | Meaning |
| --- | --- | --- |
| `enabled` | `true` | Render the system menu row. |
| `replace_legacy_widgets` | `true` | Hide old inline Wi-Fi/video/power/stats widgets. |
| `buttons` | five direct buttons | Ordered button definitions. |
| `sections` | all built-in sections | Root-menu section order and availability. |
| `xrandr_presets_yaml` | unset | YAML file containing named display presets. |
| `stats_command` | `x-terminal-emulator -e htop` | Detailed monitor launcher. |
| `lock_command` | `loginctl lock-session` | Session lock action. |
| `logout_command` | `i3-msg exit` | Session logout action. |
| `suspend_command` | `systemctl suspend` | Suspend action. |
| `reboot_command` | `systemctl reboot` | Restart action. |
| `poweroff_command` | `systemctl poweroff` | Shutdown action. |
| `idle_status_command` | `xset q` | Reserved/configured status command; native X11 state detection currently reads `xset q`. |
| `idle_enable_command` | explicit X11 timeouts | Enable inactivity blanking/DPMS. |
| `idle_disable_command` | `xset s off -dpms` | Disable inactivity blanking/DPMS. |
| `confirm_destructive` | `true` | Confirm logout/restart/shutdown. |
| `command_timeout_ms` | `30000` | Shell action timeout, 100–300000 ms. |
| `shortcut_limit` | `40` | Maximum shortcut rows, 1–500. |

### `[[menus.system.buttons]]`

| Key | Meaning |
| --- | --- |
| `id` | Unique button identity. |
| `section` | `root`, `wifi`, `displays`, `stats`, `shortcuts`, `power`, or `extra`. |
| `label` | Optional static label; omitted labels may be dynamic. |
| `enabled` | Whether the button is rendered. |

An enabled non-root button must reference a section listed in `menus.system.sections`. Duplicate IDs, duplicate sections, unknown sections, and invalid limits are rejected. On load, an invalid system-menu slice is replaced with system-menu defaults without discarding unrelated configuration.

## X11, Wayland, and session utilities

The shipped defaults target an i3/X11 workstation:

- `xrandr` for display state and presets;
- `xset` for idle blanking and DPMS;
- `i3-msg exit` for logout;
- `loginctl` and `systemctl` for lock/power actions.

On Wayland, override the commands for the compositor/session. Examples include:

```toml
[menus.system]
logout_command = "swaymsg exit"
lock_command = "swaylock"
idle_enable_command = "systemctl --user start swayidle.service"
idle_disable_command = "systemctl --user stop swayidle.service"
```

Display presets currently use xrandr semantics. A Wayland display backend is not inferred automatically; configure extra actions for `wlr-randr`, `kscreen-doctor`, or compositor-specific tools until a typed Wayland display provider is added.

For session helper suites, commands may point to scripts or utilities such as `sessionctl`, provided they return a meaningful exit status and complete before `command_timeout_ms`.

## Security and failure behavior

- Menu commands execute as the current user, never through privilege escalation automatically.
- Power operations may be permitted or rejected by logind/polkit; rejection is shown as an action error.
- SSIDs and saved connection names are shell-quoted before command construction.
- Destructive commands are not run by unit tests.
- Commands receive null stdin, captured stdout/stderr, and a timeout.
- Timeout expiration kills the child process and reports a timeout status.
- An invalid extra item or button layout does not silently create a dead action; validation reports the problem and restores the default system-menu configuration.

## Example

See:

- [`../examples/system-menubar/unilii.toml`](../examples/system-menubar/unilii.toml)
- [`../examples/system-menubar/xrandr-presets.yml`](../examples/system-menubar/xrandr-presets.yml)
- [`../examples/system-menubar/README.md`](../examples/system-menubar/README.md)

## Known limitations

- X11 idle state is currently detected with `xset q`; `idle_status_command` is retained for backend evolution but arbitrary status output is not yet interpreted.
- Wi-Fi password prompting is delegated to NetworkManager/nmcli or the configured settings application.
- The shortcut table reflects the loaded configuration; standalone hotkeyd bindings loaded from a separate file are not mirrored into the bar process automatically.
- xrandr preset application is X11-specific.
- There is no full GUI screenshot assertion; behavior is covered at the menu model, parser, command builder, navigation, and bar regression levels.
