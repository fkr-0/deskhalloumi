# Replacing sxhkd on i3/X11

## Decision

DeskHalloumi now provides two supported X11 ownership paths:

1. **Generated i3 bindings** for ordinary press/release actions. i3 owns the
   passive grabs and DeskHalloumi remains the declarative source of truth.
2. **Native selective X11 backend** for hold/modifier-release, repeat, cooldown,
   priority, and consume semantics that i3 cannot represent.

Raw evdev observation remains useful for diagnostics and physical-key work. The
whole-device `--grab` escape hatch remains unsafe for normal desktop operation.

## Capability matrix

| Capability | Generated i3 backend | Native X11 backend | Evdev observe backend |
|---|---:|---:|---:|
| Press | Exact | Exact | Executes, but key reaches client |
| Release | `bindsym --release` | Exact | Executes, but key reaches client |
| Modifier-release + hold | Not representable | Exact through shared engine | Engine exact, no suppression |
| Explicit repeat trigger | Not equivalent | Exact through X11 repeat events | Exact through evdev repeat events |
| Cooldown | Not representable | Exact | Exact |
| Priority/consume | Not representable | Exact within DeskHalloumi dispatch | Exact within DeskHalloumi dispatch |
| Selective trigger suppression | i3-owned | Native passive grab | No |
| Unmatched input untouched | Yes | Yes | Yes in observe mode |
| CapsLock/NumLock variants | i3-owned | Explicitly grabbed/tested | Not relevant to matching |
| Left/right modifiers | i3-owned | Normalized/tested | Normalized/tested |
| Active-config conflict scan | Implemented | Grab conflicts diagnosed | Not applicable |
| Shell/menu actions | Yes | Yes | Yes |
| Bar/tray/widget actions | Through hotkeyd/action bus | Through action bus | Through action bus |
| Dynamic evdev hotplug | Not applicable | Not applicable | Implemented through tokio-udev with generation deduplication |
| Sway/Wayland parity | Separate future adapter | No | Permission-dependent; not claimed |

## Active i3 conflict audit

The scanner recursively resolves includes within allowed roots, expands i3
variables, tracks modes, parses `bindsym` and `bindcode`, and reports exact or
modifier-equivalent collisions with source file and line. Dynamic/unresolved
includes make the audit explicitly incomplete.

```sh
deskhalloumi-hotkeyd \
  --menu-defaults \
  --audit-i3-config ~/.config/i3/config \
  --strict
```

The 2026-07-18 audit of the current workstation is recorded in
[`i3-active-audit-2026-07-18.md`](i3-active-audit-2026-07-18.md). The live config
was not modified or reloaded.

## Generated i3 cutover

```sh
mkdir -p ~/.config/deskhalloumi

deskhalloumi-hotkeyd \
  --sxhkd ~/.config/sxhkd/sxhkdrc \
  --audit-i3-config ~/.config/i3/config \
  --write-i3-bindings ~/.config/deskhalloumi/i3-bindings.conf \
  --strict
```

Add to i3:

```text
include ~/.config/deskhalloumi/i3-bindings.conf
```

After the include is established:

```sh
deskhalloumi-hotkeyd \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --audit-i3-config ~/.config/i3/config \
  --write-i3-bindings ~/.config/deskhalloumi/i3-bindings.conf \
  --reload-i3 \
  --strict
```

Strict validation happens before atomic replacement. Unsupported semantics,
migration warnings, unresolved includes, or collisions leave the last-known-good
file untouched.

## Native X11 deployment

Use the X11 backend when a binding depends on the advanced engine:

```sh
deskhalloumi-hotkeyd \
  --backend x11 \
  --config ~/.config/deskhalloumi/hotkeys.toml \
  --watch
```

The backend derives keycodes from the active X11 keyboard mapping and grabs only
the configured trigger key plus required modifier/lock variants. A matching
trigger is withheld from the focused client; unrelated keys are not grabbed.
Grab conflicts identify the binding, chord, keycode, and modifier mask.

Reload validates configuration, stops the old X11 listener, attempts the new
generation, and restores the previous binding set if the candidate cannot
acquire its grabs.

## Action bus

Standalone hotkeyd executes shell and managed-menu actions directly. Bar, tray,
and widget actions are sent to the bar over the versioned private Unix socket:

```text
$XDG_RUNTIME_DIR/deskhalloumi/action.sock
```

A missing bar receiver fails only the invoked action with a bounded timeout; the
daemon and unrelated bindings remain active.

## Isolated integration test

`scripts/test_i3_hotkeys.sh` starts Xvfb and a real i3 instance without touching
the developer's session. It verifies:

- generated press and release actions execute exactly once;
- strict-invalid output does not replace a known-good include;
- native X11 press and modifier-release behavior;
- actual X11 repeat delivery;
- cooldown enforcement;
- priority and consume suppression;
- the grabbed trigger key does not reach a focused `xev` client.

The same script runs in CI after installing Xvfb, i3, xdotool, and x11-utils.

## Rollback

1. Stop `deskhalloumi-hotkeyd`.
2. Remove/comment the generated include if using the i3 backend.
3. Reload i3.
4. Re-enable sxhkd and its preserved configuration.
5. The old `unilii-*` commands and config-path fallback remain available during
   the compatibility period.
