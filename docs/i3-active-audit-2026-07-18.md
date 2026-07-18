# Active i3 keybinding audit — 2026-07-18

## Scope

The DeskHalloumi recursive scanner audited the configured entrypoint:

```text
~/.config/i3/config
```

That path resolves to:

```text
/home/user/.dotfiles/config/i3/config
```

The audit also resolved the included generated color file under
`~/.cache/wal/`. It did not modify or reload the live i3 session.

## Result

```yaml
files_scanned: 2
bindings_scanned: 118
unresolved_includes: 0
audit_incomplete: false
existing_internal_collisions: 0
deskhalloumi_default_collisions: 3
```

The built-in DeskHalloumi menu defaults conflict with these active bindings:

| DeskHalloumi binding | Chord | Existing location | Existing action |
|---|---|---|---|
| `menu_i3_vis` | `Mod4+i` | config line 54 | `filter_tab_menu --tab "S-i"` |
| `menu_filter_tab` | `Mod4+u` | config line 53 | `filter_tab_menu --tab "S-u"` |
| `menu_copyq` | `Mod4+c` | config line 167 | `focus child` |

## Cutover requirement

Do not enable the built-in menu defaults unchanged. Choose one of these paths:

1. keep the current chords and assign different DeskHalloumi chords;
2. replace the current actions intentionally and remove their i3 definitions;
3. omit `--menu-defaults` and declare only the desired bindings in the canonical
   DeskHalloumi TOML file.

The scanner can reproduce the audit with:

```sh
deskhalloumi-hotkeyd \
  --menu-defaults \
  --audit-i3-config ~/.config/i3/config
```

Use `--strict` in automation; exact collisions or unresolved includes then
produce a non-zero result.
