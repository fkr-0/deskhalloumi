# i3-vis Menu Specification

Status: experimental implementation spec.

## Idea

`i3-vis` is a mod-release menu for understanding and navigating the current i3 workspace tree. i3 workspaces are trees: split containers contain windows or other split containers. The menu visualizes that tree as a compact graph and highlights the window that was focused when the program started.

Example shape:

```text
workspace 2
└─ 2 vertical
   ├─ emacs
   └─ 2 horizontal
      ├─ firefox
      └─ xterm
```

## Launch model

The intended usage mirrors the other mod-release menus:

1. i3 binds `mod+key` to launch `deskhalloumi-i3-vis`.
2. The program reads `i3-msg -t get_tree` once at startup.
3. It selects the focused window from that startup snapshot.
4. While the modifier is held, the user can move selection.
5. The first modifier release confirms the selected window and exits.

Default suggested binding:

```i3
for_window [app_id="unilii-i3-vis"] floating enable, border pixel 0, move position center, sticky enable
bindsym $mod+i exec --no-startup-id /usr/local/bin/deskhalloumi-i3-vis
```

Optional i3 rule for a more HUD-like feel:

```i3
for_window [app_id="unilii-i3-vis"] opacity 0.90
```

Avoid `no_focus` for the default mode: the application must receive the modifier release event to confirm. A future global-key-engine variant can support true `no_focus` overlays.

## Visualization

The menu renders only the workspace that contains the startup-focused window. If no focused node is found, it falls back to the first workspace in the tree.

Rows:

- workspace/root rows are structural and not selectable.
- split containers are structural and not selectable.
- window rows are selectable.
- the startup-focused window keeps a `★` marker.
- the current selection has a `▶` marker.

Layout labels:

- `2 vertical` for a `splitv` container with two children
- `2 horizontal` for a `splith` container with two children
- `N tabbed` / `N stacked` for those i3 layouts

## Interaction

- Modifier release: confirm selected window and exit.
- Enter: confirm selected window and exit.
- Escape: cancel, restore focus to the window focused at menu launch, and exit.
- `h` / `j` / `k` / `l`: run i3 `focus left/down/up/right` immediately and refresh the tree.
- `Shift+H` / `Shift+J` / `Shift+K` / `Shift+L`: run i3 `move left/down/up/right` immediately and refresh the tree.
- `ArrowDown` / `Tab`: next selectable window.
- `ArrowUp` / `Shift+Tab`: previous selectable window.
- `g` / `Home`: first selectable window.
- `G` / `End`: last selectable window.
- `r` / `Ctrl+R`: refresh the tree from i3.

## Non-obfuscation / overlay constraints

The popup should feel like a lightweight HUD:

- borderless
- transparent background
- always on top
- not represented in its own tree graph
- compatibility app id: `unilii-i3-vis`
- default title: `DeskHalloumi i3 Visualizer`

The legacy app id remains stable during the 0.2 command-name migration so
existing i3 rules continue to match. The executable name, CLI help, and visible
title use DeskHalloumi branding.

Because focus is needed for mod-release confirmation in the current standalone implementation, the first version is WM-managed and focusable. It filters itself out of the displayed i3 tree.

## Esc restore semantics

`i3-vis` captures the window focused at program start and keeps that id for the whole menu lifetime. If live passthrough commands such as `h/j/k/l` change focus while the menu is open, pressing `Esc` runs:

```sh
i3-msg '[con_id=<startup-focused>] focus'
```

Then the HUD exits. This makes Escape a true cancel/restore operation instead of leaving the last passthrough focus change active.

## Confirmation

Confirmation runs:

```sh
i3-msg '[con_id=<selected>] focus'
```

`--no-exec` keeps the popup dry-run for testing.


## Headless e2e mode

`deskhalloumi-i3-vis` supports deterministic non-GUI execution for integration tests:

```sh
deskhalloumi-i3-vis --i3-msg ./fake-i3-msg --dump-text
deskhalloumi-i3-vis --i3-msg ./fake-i3-msg --dump-text --e2e-actions j,release
```

`--dump-text` loads the same i3 tree, builds the same `I3VisState`, and renders deterministic text from that model instead of launching Iced. This is the default e2e path because it does not require `DISPLAY` or a live i3 session.

`--e2e-actions` accepts a comma-separated script:

- `next`
- `previous` / `prev`
- `h` / `j` / `k` / `l` for i3 `focus left/down/up/right`
- `H` / `J` / `K` / `L` for i3 `move left/down/up/right`
- `focus-left` / `focus-down` / `focus-up` / `focus-right`
- `move-left` / `move-down` / `move-up` / `move-right`
- `first`
- `G` / `last`
- `release` / `modrelease` / `enter` / `confirm`
- `esc` / `escape` / `cancel` restores startup focus and exits
- `r` / `refresh`

The e2e fixture test uses a fake `i3-msg` executable. For `-t get_tree` it prints a fixture JSON file; for focus commands it logs the exact argv. This validates both the rendered graph and the final focus command, for example `[con_id=24] focus` after `next,release`. It also validates immediate i3 passthrough commands such as `h,j,k,l,H,J,K,L`, which log `focus left/down/up/right` and `move left/down/up/right`.

## Screenshot e2e mode

Screenshot testing is possible but intentionally optional and ignored by default. It requires a graphical session, `xdotool`, and ImageMagick `import`:

```sh
RUN_I3_VIS_SCREENSHOT_E2E=1 \
  cargo test -p deskhalloumi-bin --test i3_vis_e2e -- --ignored --nocapture
```

The ignored smoke test launches:

```sh
deskhalloumi-i3-vis --mock --no-exec
```

Then it finds the `deskhalloumi-i3-vis` window and writes:

```text
target/i3-vis-smoke.png
```

This is useful for local visual regression checks, while the default CI-safe e2e suite remains fully headless.

## Open follow-ups

- Global key-engine backed true no-focus mode.
- Canvas/SVG layout rendering instead of the current compact tree rows.
- Direct geometric mini-map from i3 rect coordinates.
- Optional window thumbnails.
