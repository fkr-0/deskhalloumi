# System menubar example

`unilii.toml` demonstrates the configurable clickable system-menu row and the shared release menu policy. Copy it to the normal unilii configuration location or merge the `[menus.ui]`, `[menus.wifi]`, `[menus.mount]`, `[menus.calendar]`, `[menus.custom]`, `[menus.system]`, button, extra-item, and keybinding sections into an existing configuration.

The SSHFS profile is intentionally illustrative; replace its host, user, paths, and mountpoint before enabling it in daily use. See [`docs/menu-design-system.md`](../../docs/menu-design-system.md) for interaction, validation, confirmation, favorites, and command-safety details.

Copy `xrandr-presets.yml` to the path configured by `menus.system.xrandr_presets_yaml`, then adjust output names after inspecting `xrandr --query`.

Validate by running the normal bar test suite or starting the bar with the explicit configuration path. Destructive actions display a confirmation view when `confirm_destructive = true`.
