# Keybinding System

The unilii keybinding system allows you to bind keyboard shortcuts to shell commands and internal unilii actions (bar, tray, widget).

## Configuration

Keybindings are configured in `~/.config/unilii.toml`. The configuration file uses TOML format and supports multiple keybinding types.

## Syntax

### Basic Structure

Each keybinding entry has the following fields:

```toml
[[keybindings]]
name = "binding_name"          # Descriptive name (required)
keysym = "Modifier+Key"       # Key combination (required)
type = "shell|bar|tray|widget" # Action type (optional, defaults to "shell")
command = "command_to_execute" # Command or action (required)
```

### Key Combinations (keysym)

Key combinations use X11 keysym format:

- **Modifiers**: `Super` (Windows key), `Ctrl` (Control), `Shift`, `Alt`
- **Keys**: Any key name (e.g., `a`, `Enter`, `Space`, `F1`, `Escape`)
- **Format**: Combine modifiers and keys with `+`

Examples:
- `Super+Enter` - Super + Enter
- `Ctrl+Alt+t` - Ctrl + Alt + T
- `Shift+F1` - Shift + F1
- `Super+Shift+q` - Super + Shift + Q

## Command Types

### 1. Shell Commands (default)

Execute shell commands. This is the default type if `type` is not specified.

```toml
[[keybindings]]
name = "launch_terminal"
keysym = "Super+Enter"
command = "alacritty"
```

```toml
[[keybindings]]
name = "volume_up"
keysym = "Super+Up"
command = "amixer set Master 5%+"
```

You can also explicitly specify `type = "shell"`:

```toml
[[keybindings]]
name = "launch_browser"
keysym = "Super+b"
type = "shell"
command = "firefox"
```

### 2. Bar Actions

Control unilii bar behavior.

```toml
[[keybindings]]
name = "toggle_clock"
keysym = "Super+c"
type = "bar"
command = "toggle-module clock"
```

**Available bar commands:**

- `toggle-module <name>` - Toggle visibility of a module (e.g., "clock", "battery")
- `reload-config` - Reload configuration from file
- `quit` - Quit unilii application

Examples:

```toml
# Toggle clock module
[[keybindings]]
name = "toggle_clock"
keysym = "Super+c"
type = "bar"
command = "toggle-module clock"

# Toggle battery module
[[keybindings]]
name = "toggle_battery"
keysym = "Super+b"
type = "bar"
command = "toggle-module battery"

# Reload configuration
[[keybindings]]
name = "reload_config"
keysym = "Super+r"
type = "bar"
command = "reload-config"

# Quit application
[[keybindings]]
name = "quit"
keysym = "Super+q"
type = "bar"
command = "quit"
```

### 3. Tray Actions

Control the tray system.

```toml
[[keybindings]]
name = "open_tray_menu_1"
keysym = "Super+1"
type = "tray"
command = "open-menu 0"
```

**Available tray commands:**

- `open-menu <index>` - Open tray menu for icon at index (0-9)
- `close-menu` - Close all tray menus
- `show-aggregated` - Switch to aggregated view (all tray items)
- `show-favorites` - Switch to favorites view

Examples:

```toml
# Open menu for first tray icon
[[keybindings]]
name = "open_tray_menu_1"
keysym = "Super+1"
type = "tray"
command = "open-menu 0"

# Open menu for second tray icon
[[keybindings]]
name = "open_tray_menu_2"
keysym = "Super+2"
type = "tray"
command = "open-menu 1"

# Close all menus
[[keybindings]]
name = "close_menus"
keysym = "Escape"
type = "tray"
command = "close-menu"

# Show aggregated view
[[keybindings]]
name = "show_all_tray_items"
keysym = "Super+t"
type = "tray"
command = "show-aggregated"

# Show favorites view
[[keybindings]]
name = "show_favorites"
keysym = "Super+y"
type = "tray"
command = "show-favorites"
```

### 4. Widget Actions (Future)

Widget actions are planned for future versions.

```toml
[[keybindings]]
name = "widget_action"
keysym = "Super+w"
type = "widget"
command = "widget:show <id>"
```

## Complete Example

Here's a complete example configuration:

```toml
[window]
width = 800
height = 24
position_x = 0
position_y = 0
background_color = "#1e1e1e"
text_color = "#ffffff"

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

# Shell commands (default type)
[[keybindings]]
name = "launch_terminal"
keysym = "Super+Enter"
command = "alacritty"

[[keybindings]]
name = "launch_browser"
keysym = "Super+b"
command = "firefox"

[[keybindings]]
name = "volume_up"
keysym = "Super+Up"
command = "amixer set Master 5%+"

# Bar actions
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

# Tray actions
[[keybindings]]
name = "open_tray_menu_1"
keysym = "Super+1"
type = "tray"
command = "open-menu 0"

[[keybindings]]
name = "close_menus"
keysym = "Escape"
type = "tray"
command = "close-menu"
```

## Configuration File Location

The default configuration file location is `~/.config/unilii.toml`. If this file doesn't exist, unilii will create a default configuration file with sensible defaults.

## Troubleshooting

### Keybindings not working

1. Check that the keysym is correct (use `xev` to find the exact keysym name)
2. Ensure the command is valid and executable
3. Check the unilii logs for error messages
4. Make sure the keybinding doesn't conflict with other applications

### Shell commands not executing

1. Ensure the command is in your PATH or provide the full path
2. Test the command in a terminal first to verify it works
3. Check for typos in the command string

### Module names for `toggle-module`

The module name must match the `name` field in your modules configuration:

```toml
[[modules]]
name = "clock"  # Use "clock" in toggle-module command
enabled = true
position = "right"
```

## Tips and Best Practices

1. **Use descriptive names** for your keybindings to make the config file easier to read
2. **Test shell commands** in a terminal before adding them to the config
3. **Avoid conflicts** with common application shortcuts (Ctrl+C, Ctrl+V, etc.)
4. **Group related keybindings** in the config file with comments
5. **Use modifier keys** to avoid accidental triggers (e.g., Super, Ctrl+Alt)
6. **Backup your config** before making major changes

## Additional Resources

- See `examples/unilii.toml` for a complete example configuration
- Use `man xev` to find correct keysym names for your keyboard
- Check the main README.md for general unilii configuration options
