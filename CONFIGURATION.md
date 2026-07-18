# Configuration

This document describes the configuration options for **DeskHalloumi**, including multi-panel support, module configuration, and keybinding system.

## Configuration File Location

The configuration file is located at `~/.config/deskhalloumi/deskhalloumi.toml`.

## Multi-Panel Configuration

Unilii supports multiple independent panels that can be positioned anywhere on your screen. Each panel is defined in the `[[panels]]` array.

### Panel Configuration

Each panel in the `[[panels]]` array supports the following options:

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `name` | String | No | Panel identifier (used for logging and future features) |
| `width` | Integer | Yes | Panel width in pixels |
| `height` | Integer | Yes | Panel height in pixels |
| `position_x` | Integer | Yes | X coordinate of panel position |
| `position_y` | Integer | Yes | Y coordinate of panel position |
| `background_color` | String | No | Background color in hex format (e.g., "#1e1e1e") |
| `text_color` | String | No | Default text color in hex format (e.g., "#ffffff") |

### Example: Multiple Panels

```toml
# Top bar panel
[[panels]]
name = "top_bar"
width = 1920
height = 24
position_x = 0
position_y = 0
background_color = "#1e1e1e"
text_color = "#ffffff"

# Bottom bar panel
[[panels]]
name = "bottom_bar"
width = 1920
height = 24
position_x = 0
position_y = 1076
background_color = "#282828"
text_color = "#ffffff"

# Side panel (left)
[[panels]]
name = "side_panel"
width = 200
height = 600
position_x = 0
position_y = 100
background_color = "#1e1e1e"
text_color = "#ebdbb2"
```

## Module Configuration

Modules are defined in the `[[modules]]` array. Currently, modules are shared across all panels (future versions may support per-panel modules).

### Module Options

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `name` | String | Yes | Module identifier (e.g., "clock", "battery", "tmux") |
| `enabled` | Boolean | Yes | Whether the module is enabled |
| `position` | String | Yes | Module position (left, center, right) |
| `update_interval_ms` | Integer | No | Update interval in milliseconds |

### Available Modules

- **clock**: Displays current time
- **battery**: Displays battery status
- **tmux**: Displays tmux pane information with navigation (requires tmux feature)

### Example: Module Configuration

```toml
[[modules]]
name = "clock"
enabled = true
position = "right"
update_interval_ms = 1000

[[modules]]
name = "battery"
enabled = true
position = "right"
update_interval_ms = 5000

[[modules]]
name = "tmux"
enabled = true
position = "right"
update_interval_ms = 2000
```

## Keybinding Configuration

Keybindings are defined in the `[[keybindings]]` array. Each keybinding can execute shell commands, bar actions, tray actions, or widget actions.

### Keybinding Options

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `name` | String | Yes | Descriptive name for the keybinding |
| `keysym` | String | Yes | Key combination in X11 keysym format |
| `type` | String | No | Action type (shell, bar, tray, widget) - defaults to "shell" |
| `command` | String | Yes | Command to execute (depends on type) |
| `release` | Boolean | No | If true, execute on key release (release-to-confirm) |

### Keybinding Types

#### Shell Commands (type: "shell")

Execute shell commands directly:

```toml
[[keybindings]]
name = "launch_terminal"
keysym = "Super+Enter"
type = "shell"
command = "alacritty"

[[keybindings]]
name = "volume_up"
keysym = "Super+Up"
command = "amixer set Master 5%+"

[[keybindings]]
name = "lock_screen"
keysym = "Super+l"
command = "xscreensaver-command -lock"
```

#### Bar Actions (type: "bar")

Internal bar actions:

- `toggle-module <name>`: Toggle a module on/off
- `reload-config`: Reload the configuration file
- `quit`: Quit the application

```toml
[[keybindings]]
name = "toggle_clock"
keysym = "Super+c"
type = "bar"
command = "toggle-module clock"

[[keybindings]]
name = "reload_config"
keysym = "Super+r"
type = "bar"
command = "reload-config"

[[keybindings]]
name = "quit"
keysym = "Super+q"
type = "bar"
command = "quit"
```

#### Tray Actions (type: "tray")

Internal tray actions:

- `open-menu <index>`: Open tray menu for icon at index (0-9)
- `close-menu`: Close all open tray menus
- `show-aggregated`: Show aggregated tray view
- `show-favorites`: Show favorites tray view

```toml
[[keybindings]]
name = "open_tray_menu_1"
keysym = "Super+1"
type = "tray"
command = "open-menu 0"

[[keybindings]]
name = "close_tray_menus"
keysym = "Escape"
type = "tray"
command = "close-menu"

[[keybindings]]
name = "show_aggregated_view"
keysym = "Super+t"
type = "tray"
command = "show-aggregated"
```

### Release-to-Confirm

The `release` field enables a "release-to-confirm" pattern for dangerous or irreversible actions:

```toml
[[keybindings]]
name = "quit_with_confirm"
keysym = "Alt+Delete"
type = "bar"
command = "quit"
release = true
```

When `release = true`:
1. The key press marks the action as "ready" (shows confirmation UI if available)
2. The action executes when the key is released
3. If another key is pressed before release, the confirmation is cancelled

This pattern is useful for actions like quitting the application or closing all windows, preventing accidental execution.

### Keysym Format

Key combinations use X11 keysym format:
- Modifier keys: `Super`, `Ctrl`, `Alt`, `Shift`
- Key names: `Enter`, `Escape`, `Delete`, `Print`, or character keys like `c`, `f`, etc.

Examples:
- `Super+Enter`
- `Ctrl+Alt+t`
- `Super+Shift+e`
- `Alt+F4`

## Complete Example Configuration

```toml
# Multi-panel configuration
[[panels]]
name = "top_bar"
width = 1920
height = 24
position_x = 0
position_y = 0
background_color = "#1e1e1e"
text_color = "#ffffff"

# Modules
[[modules]]
name = "clock"
enabled = true
position = "right"
update_interval_ms = 1000

[[modules]]
name = "battery"
enabled = true
position = "right"
update_interval_ms = 5000

[[modules]]
name = "tmux"
enabled = true
position = "right"
update_interval_ms = 2000

# Keybindings
[[keybindings]]
name = "launch_terminal"
keysym = "Super+Enter"
type = "shell"
command = "alacritty"

[[keybindings]]
name = "toggle_clock"
keysym = "Super+c"
type = "bar"
command = "toggle-module clock"

[[keybindings]]
name = "reload_config"
keysym = "Super+r"
type = "bar"
command = "reload-config"

[[keybindings]]
name = "quit_with_confirm"
keysym = "Alt+Delete"
type = "bar"
command = "quit"
release = true

[[keybindings]]
name = "open_tray_menu_1"
keysym = "Super+1"
type = "tray"
command = "open-menu 0"

[[keybindings]]
name = "close_tray_menus"
keysym = "Escape"
type = "tray"
command = "close-menu"
```

## Migration from Single Panel

If you're migrating from a single-panel configuration, update your config file:

**Old format:**
```toml
[window]
width = 800
height = 24
position_x = 0
position_y = 0
```

**New format:**
```toml
[[panels]]
width = 800
height = 24
position_x = 0
position_y = 0
```

The old `[window]` section is replaced with the `[[panels]]` array. You can add additional panels to the array to create multiple panels.


## Managed Menu Actions

External DeskHalloumi menus should use `type = "menu"` instead of raw shell commands.
The shared runtime enforces one process per menu and supports show, hide, and
toggle operations from either the bar-embedded daemon or `deskhalloumi-hotkeyd`.

```toml
[[keybindings]]
name = "i3_tree_menu"
keysym = "Super+i"
type = "menu"
command = "toggle:i3-vis"
priority = 100
consume = true

[[keybindings]]
name = "window_filter_menu"
keysym = "Super+u"
type = "menu"
command = "toggle:filter-tab"
priority = 100
consume = true

[[keybindings]]
name = "clipboard_menu"
keysym = "Super+c"
type = "menu"
command = "toggle:copyq"
priority = 100
consume = true
```

When standalone hotkeyd owns shortcuts, launch the bar with
`deskhalloumi run --no-hotkeyd`. A singleton guard prevents accidental dual listeners.
See `docs/hotkeyd.md` for status, validation, migration, and systemd usage.


## Standalone hotkey service

See [hotkeyd architecture](docs/hotkeyd-architecture.md) and [hotkeyd operations](docs/hotkeyd-operations.md). Standalone configurations may contain `shell` and `menu` actions; `bar`, `tray`, and `widget` actions require embedded mode.


## Clickable system menubar

The main `unilii` bar can replace the legacy inline Wi-Fi, video, power, and statistics widgets with configurable popup buttons. The system menu includes Wi-Fi management, xrandr presets, native system statistics, a clickable shortcut table, session/power actions with confirmation, and arbitrary extra actions.

All popup menus share one presentation and interaction policy:

```toml
[menus.ui]
max_visible_rows = 12
max_label_chars = 72
max_subtitle_chars = 140
scroll_height = 360
show_breadcrumbs = true
show_item_counts = true
show_keyboard_hints = true
show_all_favorites_controls = true
```

The same row order drives rendering, mouse activation, keyboard selection, and quick-jump. Section/status rows are visible but skipped by keyboard navigation. Escape leaves a submenu before closing the popup, and favorites are scoped by application plus item ID.

### Wi-Fi, storage, and calendar

```toml
[menus.wifi]
refresh_ms = 4000
max_network_rows = 12
show_known_networks = true
allow_forget = true
settings_command = "nm-connection-editor"
scan_on_open = true
connect_timeout_ms = 20000

[menus.mount]
refresh_ms = 5000
show_loop_devices = true
max_local_rows = 24
max_sshfs_rows = 12
max_loop_rows = 12
max_vcvolume_rows = 12
disks_command = "gnome-disks"
show_device_details = true

[menus.calendar]
refresh_ms = 300000
agenda_days = 7
max_account_rows = 8
max_event_rows = 24
application_command = "gnome-calendar"
show_locations = true
```

Storage profiles are configured with `[[menus.mount.sshfs_profiles]]` and `[[menus.mount.vcvolume_profiles]]`. VCVolume command templates must contain both `{volume}` and `{mountpoint}`. Calendar accounts use `[[menus.calendar.accounts]]`; account IDs must be unique.

### Custom application menus

```toml
[menus.custom]
enabled = true
max_rows = 40
show_subtitles = true
app_ids = ["my-launcher"]
quickjump_alphabet = "asdfjkl;ghqwertyuiopzxcvbnm"

[[menus.custom.items]]
id = "logs"
title = "Follow journal"
subtitle = "Open a terminal with live system logs"
action = "launcher"
command = "x-terminal-emulator"
args = ["-e", "journalctl", "-f"]
filter_fields = ["title", "subtitle", "tags"]
tags = ["system", "logs"]
working_dir = "~/"
confirm = false
visible_if = "command:journalctl"

[[menus.custom.items.env]]
key = "SYSTEMD_COLORS"
value = "1"
```

An item may configure one icon source through `icon.theme_icon`, `icon.svg_path`, or `icon.image_path`. `visible_if` accepts `env:NAME`, `path:...`, `command:program`, and recursive `not:` conditions. Setting `confirm = true` creates a review submenu with explicit Run and Cancel actions.

```toml
[menus.system]
enabled = true
replace_legacy_widgets = true
sections = ["wifi", "displays", "stats", "shortcuts", "power"]
xrandr_presets_yaml = "~/.config/deskhalloumi/xrandr-presets.yml"
confirm_destructive = true

[[menus.system.buttons]]
id = "wifi"
section = "wifi"
enabled = true

[[menus.system.buttons]]
id = "power"
section = "power"
label = "⏻"
enabled = true
```

See the [shared menu design and runtime guide](docs/menu-design-system.md), [system menubar architecture and operations guide](docs/system-menubar.md), and [complete example](examples/system-menubar/unilii.toml) for every command, button, domain menu, shortcut behavior, X11/Wayland override, and safety rule.
