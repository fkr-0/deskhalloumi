# DeskHalloumi documentation

This index groups the documentation by task and audience. The repository-level
`README.md` remains the product overview; this page is the entry point for
operators, contributors, and maintainers who need more detail.

## Install and operate

- [Installation, upgrade, rollback, and removal](installation.md)
- [Configuration reference and examples](../CONFIGURATION.md)
- [Keybindings and sxhkd migration](../KEYBINDINGS.md)
- [Hotkey daemon operation and troubleshooting](hotkeyd-operations.md)
- [System menubar](system-menubar.md)
- [i3 visualizer menu](i3-vis-menu.md)

## Architecture

- [Async runtime and subprocess policy](async-runtime.md)
- [Quick-select, provider, menu, and typed-action contracts](runtime-contracts.md)
- [Bar architecture](bar-architecture.md)
- [Hotkey daemon architecture](hotkeyd-architecture.md)
- [Shared menu design system](menu-design-system.md)
- [Action and topology matrix](hotkey-action-matrix.md)
- [CopyQ frontend API](copyq-frontend-apispec.md)

## Migration and compatibility

- [Project rename and compatibility contract](project-renaming.md)
- [i3/sxhkd replacement feasibility](hotkeyd-i3-feasibility.md)
- [Active i3 audit from July 18, 2026](i3-active-audit-2026-07-18.md)

## Releases and maintenance

- [DeskHalloumi 0.3.0 release notes](releases/0.3.0.md)
- [DeskHalloumi 0.2.0 release notes](releases/0.2.0.md)
- [Versioning and release policy](versioning.md)
- [`roadmap.yml`](../roadmap.yml), the internal maintainer roadmap
- [`todo.yml`](../todo.yml), focused known gaps
- [`tasks.yml`](../tasks.yml), implementation evidence and review state

## Support boundary

DeskHalloumi currently supports i3/X11 for global shortcuts and active
configuration auditing. Iced rendering may work on Wayland, but Sway/Wayland
global-hotkey, output-control, and layer-shell parity are not claimed.
