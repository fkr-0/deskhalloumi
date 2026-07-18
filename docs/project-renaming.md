# DeskHalloumi rename and compatibility contract

## Decision

The public project name is **DeskHalloumi**. Machine-facing identifiers use the
lowercase slug `deskhalloumi`.

The previous names `unilii` and the occasional typo `uniliii` were difficult to
read and type reliably. DeskHalloumi is distinctive, pronounceable, and broad
enough for the panel, tray, menus, hotkey service, action bus, and desktop
control tools.

## Name verification performed on 2026-07-18

Exact-name searches were run across general web results, GitHub-indexed results,
crates.io, npm, and PyPI. No exact `DeskHalloumi`/`deskhalloumi` project or
package was found in the indexed results checked during this review.

This is a collision screen, not legal clearance:

- it does not replace a trademark search by jurisdiction and product class;
- package names remain available only until somebody registers them;
- domain availability must be confirmed at registration time;
- visual/logo similarity has not been assessed.

The name is therefore suitable for repository development and local packaging,
but public commercial release still requires the maintainer's normal legal and
registry checks.

## Identifier map

| Surface | Primary identifier | Compatibility identifier |
|---|---|---|
| Repository/project | `deskhalloumi` | `unilii` |
| Main command | `deskhalloumi` | `unilii` |
| Panel command | `deskhalloumi-bar` | `unilii-bar` |
| Hotkey command | `deskhalloumi-hotkeyd` | `unilii-hotkeyd` |
| Clipboard command | `deskhalloumi-copyq` | `unilii-copyq` |
| Filter-tab command | `deskhalloumi-filter-tab` | `unilii-filter-tab` |
| i3 visualizer | `deskhalloumi-i3-vis` | `unilii-i3-vis` |
| Core crate | `deskhalloumi-core` | source remains under `unilii/core` |
| Shared library | `deskhalloumi-lib` | source remains under `unilii/lib` |
| Config directory | `~/.config/deskhalloumi` | read fallback: `~/.config/unilii` |
| Main config | `deskhalloumi.toml` | read fallback: `unilii.toml` |
| Runtime directory | `$XDG_RUNTIME_DIR/deskhalloumi` | override: `UNILII_RUNTIME_DIR` |
| Environment prefix | `DESKHALLOUMI_` | fallback: `UNILII_` |
| systemd unit | `deskhalloumi-hotkeyd.service` | `unilii-hotkeyd.service` |

## Implemented phase-one behavior

### Rust packages and commands

Workspace package names now use `deskhalloumi-*`. Every user-facing primary
binary has a legacy `unilii-*` wrapper built from the same source entrypoint, so
scripts can migrate independently.

### Environment precedence

When both names exist, the new value wins:

```text
DESKHALLOUMI_RUNTIME_DIR > UNILII_RUNTIME_DIR > XDG_RUNTIME_DIR default
DESKHALLOUMI_BAR_CONFIG  > UNILII_BAR_CONFIG  > discovered config paths
DESKHALLOUMI_XRANDR_PRESETS_YAML > UNILII_XRANDR_PRESETS_YAML
```

New code must follow the same rule. Compatibility variables are readers, not a
reason to keep writing new state under the old prefix.

### Configuration paths

DeskHalloumi reads the new location first. If it does not exist, it reads the
legacy location without moving or deleting anything. Creating a default config
always writes the new path.

```text
new:    ~/.config/deskhalloumi/deskhalloumi.toml
legacy: ~/.config/unilii/unilii.toml
```

This is deliberately a fallback, not an automatic migration.

### Runtime and IPC paths

New processes use `$XDG_RUNTIME_DIR/deskhalloumi`. An explicit
`DESKHALLOUMI_RUNTIME_DIR` wins over the legacy override. The hotkey control and
action-bus protocols are versioned independently of branding, so field names do
not change merely because the project name changed.

## systemd migration

Install and enable the new unit:

```sh
install -Dm644 contrib/systemd/user/deskhalloumi-hotkeyd.service \
  ~/.config/systemd/user/deskhalloumi-hotkeyd.service
systemctl --user daemon-reload
systemctl --user disable --now unilii-hotkeyd.service 2>/dev/null || true
systemctl --user enable --now deskhalloumi-hotkeyd.service
```

Do not enable both units: both intentionally contend for the same logical
hotkey-daemon singleton.

Rollback:

```sh
systemctl --user disable --now deskhalloumi-hotkeyd.service
systemctl --user enable --now unilii-hotkeyd.service
```

The old config remains untouched, so rollback does not require data restoration.

## Remaining public-release work

- Reserve desired package names before publication.
- Check relevant trademark registers and product classes.
- Decide whether to rename the physical source directory from `unilii/`; this is
  intentionally deferred because it creates a large low-value Git move.
- Application IDs and DBus identities remain on their legacy values in 0.2.0;
  rename them only with dual matching or compatibility readers.
- Retain command, variable, path, and unit aliases through at least one announced
  release cycle; removal is a breaking change.
