# Hotkey configuration examples

## `hotkeys.toml`

Standalone-service example using only `shell` and managed `menu` actions:

```sh
unilii-hotkeyd --config examples/hotkeyd/hotkeys.toml --dry-run --strict
unilii-hotkeyd --config examples/hotkeyd/hotkeys.toml --shadow
```

## `embedded-bar-hotkeys.toml`

Bar-embedded example using `tray` actions. Merge these `[[keybindings]]` entries
into the main unilii configuration, start the bar without `--no-hotkeyd`, and do
not run standalone hotkeyd:

```sh
unilii --config /path/to/merged-unilii.toml run
```

See:

- [Architecture](../../docs/hotkeyd-architecture.md)
- [Operations](../../docs/hotkeyd-operations.md)
- [Action matrix](../../docs/hotkey-action-matrix.md)
