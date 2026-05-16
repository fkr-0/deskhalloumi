# Configuration

This document describes the configuration options for **unilii**, including multi-panel support, module configuration, and keybinding system.

## Configuration File Location

The configuration file is located at `~/.config/unilii/unilii.toml`.

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
