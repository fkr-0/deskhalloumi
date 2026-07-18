# deskhalloumi-hotkeyd

`deskhalloumi-hotkeyd` is the standalone global-key service for unilii. It shares the
same deterministic key engine and managed-menu runtime as the bar while keeping
input ownership, menu processes, and configuration reloads coordinated across
processes.

## Documentation

- [Architecture and invariants](hotkeyd-architecture.md)
- [Action types and topology matrix](hotkey-action-matrix.md)
- [Installation, operation, and troubleshooting](hotkeyd-operations.md)
- [General keybinding reference](../KEYBINDINGS.md)
- [General configuration reference](../CONFIGURATION.md)

## Recommended start

```sh
deskhalloumi-hotkeyd --print-defaults > ~/.config/deskhalloumi/hotkeys.toml
deskhalloumi-hotkeyd --config ~/.config/deskhalloumi/hotkeys.toml --dry-run --strict
deskhalloumi-hotkeyd --config ~/.config/deskhalloumi/hotkeys.toml --watch
```

Run the bar with:

```sh
deskhalloumi run --no-hotkeyd
```

## Daily controls

```sh
deskhalloumi-hotkeyd --ping
deskhalloumi-hotkeyd --status
deskhalloumi-hotkeyd --status --json
deskhalloumi-hotkeyd --reload
deskhalloumi-hotkeyd --shutdown
deskhalloumi-hotkeyd --menu-action toggle:i3-vis
```

Reloads are transactional: invalid candidates are rejected, listener failures
are rolled back, and the committed generation changes only after the new input
worker reports readiness.
