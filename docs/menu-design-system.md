# Menu design system and runtime architecture

This document describes the release menu architecture shared by the unilii tray, built-in system menus, Wi-Fi, storage, calendar, and configured custom launchers.

The goal is not merely consistent styling. Every menu now follows the same interaction contract, data ordering, failure model, command-safety rules, and configuration boundaries.

## Design principles

1. **One ordered model.** The same ordered `TrayMenuItem` rows drive rendering, mouse activation, keyboard selection, quick-jump labels, item counts, and action lookup. A menu must not render one order and activate another.
2. **Actions and information are distinct.** Section headings, status cards, separators, loading rows, and empty states are visible but never keyboard-selectable.
3. **Progressive disclosure.** Primary actions remain at the top. Secondary details are grouped below headings. Destructive or explicitly confirmed custom actions open a review submenu before execution.
4. **Keyboard and pointer parity.** Anything clickable is reachable by keyboard. Keyboard activation uses the same message/action path as pointer activation.
5. **Stable failure states.** Loading, partial data, empty data, stale data, and recoverable errors remain visible in the popup rather than collapsing the menu or silently doing nothing.
6. **Configuration fails locally.** Invalid menu presentation or domain configuration falls back only that menu slice. Panel, modules, keybindings, and unrelated menu settings remain intact.
7. **Untrusted dynamic text is bounded and quoted.** SSIDs, device labels, profile names, event titles, DBus labels, paths, and configured environment values are bounded for layout. Values interpolated into shell commands use single-quote escaping.

## Component map

```text
Config
  └─ menus
      ├─ ui          shared presentation policy
      ├─ wifi        NetworkManager behavior
      ├─ mount       local/SSHFS/loop/VCVolume behavior
      ├─ calendar    agenda and launcher behavior
      ├─ custom      configured launcher behavior
      └─ system      system-menubar sections and commands

Providers / snapshots
  ├─ StatusNotifier + DBusMenu
  ├─ nmcli
  ├─ lsblk, /proc/mounts, losetup
  ├─ CalDAV/cache helpers
  └─ configured custom items

Canonical menu models
  ├─ menus/presentation.rs
  ├─ menus/wifi.rs
  ├─ menus/mount.rs
  ├─ menus/calendar.rs
  ├─ menus/custom.rs
  └─ menus/system.rs

EnhancedTrayState
  ├─ current view
  ├─ selected visible-row index
  ├─ submenu path
  ├─ filter text
  ├─ favorites
  └─ animation state

Iced popup renderer
  ├─ common header and mode toolbar
  ├─ breadcrumb
  ├─ semantic row renderer
  ├─ search / empty / status views
  └─ bounded scroll body
```

## Menu row semantics

The existing `TrayMenuItem` structure remains the common transport type. Shared constructors in `menus/presentation.rs` encode semantics without introducing a second incompatible UI tree.

| Row | Purpose | Selectable | Typical rendering |
|---|---|---:|---|
| Action | Runs a command or DBus action | Yes, when enabled | Full-width row with title, optional subtitle, icon, shortcut, and selection marker |
| Submenu | Opens a nested collection | Yes | Action row with trailing chevron |
| Checkable | Represents a toggle/radio state | Yes, when enabled | Action row with checked/unchecked marker |
| Text input | Accepts provider-owned text | Input focus | Label and full-width input |
| Section | Groups a set of rows | No | Low-emphasis heading with optional count |
| Status | Loading, error, current state, or explanation | No | Bounded information card |
| Separator | Visual grouping only | No | Thin divider |

Section IDs use the `section:` prefix and status IDs use `status:`. Selection and quick-jump helpers use semantic predicates rather than assuming every visible row is actionable.

## Shared popup anatomy

A release popup is composed in this order:

1. Header: provider icon, title, status/subtitle, optional item count, previous/next app controls.
2. Breadcrumb: displayed for nested DBus, system, or confirmation submenus when enabled.
3. Mode toolbar: All actions and Favorites views.
4. Optional search field or quick-jump banner.
5. Semantic body rows, scrollable after `menus.ui.max_visible_rows`.
6. Optional compact keyboard hint footer.

The popup window height follows the configured visible-row and scroll-height policy rather than a fixed historical row cap. Labels and subtitles are Unicode-safe and truncated with an ellipsis.

DBus mnemonic markers are normalized for display: `_Open` is rendered as `Open`, while escaped `__` remains a literal underscore.

## Interaction contract

| Input | Result |
|---|---|
| Click | Activate the clicked action or enter its submenu |
| Up / Down | Move across actionable rows only, wrapping at the ends |
| Tab | Move to the next actionable row |
| Shift+Tab | Reserved for reverse focus traversal where supplied by the active input path; Up always moves backward |
| Enter | Activate the selected row |
| Right | Enter the selected submenu; otherwise move to the next tray application |
| Left | Leave the current submenu; otherwise move to the previous tray application |
| Escape | Cancel quick-jump, then leave one submenu level, then close the popup at the root |
| `g` | Toggle quick-jump mode |
| `f` | Toggle the selected item as a favorite when the current view supports it |
| `a` | Open the aggregated All actions view |
| `v` | Open Favorites |

Focused Iced events and the embedded/global evdev path implement the same semantics.

### Selection invariants

- Opening a menu or submenu selects its first enabled, visible, non-separator row.
- Changing an aggregated filter recomputes selection against the filtered results.
- Invalid submenu paths leave the current view unchanged.
- Entering a submenu never starts closing the popup.
- Informational rows never receive quick-jump labels.

## Quick-jump

Quick-jump labels are generated only for actionable rows. Labels use the configured custom-menu alphabet for matching custom providers and the standard home-row-oriented alphabet elsewhere.

The displayed hint, keyboard lookup, and action index are derived from one selectable-index vector. This prevents headings or disabled rows from shifting activation targets.

## Search and aggregated actions

The All actions view flattens visible, enabled actions from all tray providers and retains their application ID and full path. Search matches labels and paths. Results use two sibling controls rather than invalid nested buttons:

- the main action button;
- an optional favorite toggle.

`menus.ui.show_all_favorites_controls = false` hides per-row favorite stars in All actions while keeping favorite removal controls visible inside Favorites.

## Favorites

Favorites are keyed by both application and item ID. This avoids collisions when two DBus providers both expose common IDs such as `open`, `settings`, or `quit`.

The storage key is application-scoped. Legacy unscoped favorite IDs are still recognized and are migrated/removed when first toggled, so existing in-memory events remain compatible.

Favorites are intentionally runtime state at present; persistence across process restarts remains a separate bounded enhancement.

## Confirmation flow

Custom items with `confirm = true` and destructive system actions use review submenus.

A confirmation submenu contains:

1. a non-selectable warning/status card;
2. **Run action**, selected first and activated through the normal action path;
3. **Cancel**, which returns to the parent/root without running the command.

Escape and Left also leave the confirmation submenu. The popup remains open throughout review.

## Domain menus

### Wi-Fi

The canonical Wi-Fi row order is:

1. Enable/disable radio.
2. Rescan.
3. Open configured settings application.
4. Current interface/connection status.
5. Available networks heading and rows.
6. Saved connections heading and rows.
7. Optional forget actions.

Connected networks sort first, followed by signal strength and SSID. Signal is shown as a compact glyph and percentage. Network and profile names are shell-quoted before building `nmcli` commands.

### Storage

The storage menu combines:

- local block devices;
- SSHFS profiles;
- loop devices/images;
- VCVolume profiles;
- refresh and configured disk-utility actions.

`lsblk -P` parsing preserves quoted spaces and escapes. Device paths, mountpoints, users, hosts, remote paths, image paths, and profile values are shell-quoted. Each category has an independent configurable row limit.

### Calendar

The calendar menu presents an agenda rather than raw transport data:

- refresh and configured calendar launcher;
- account summary and account-specific failures;
- events grouped by local day;
- human-readable local time;
- optional locations;
- explicit no-account and no-event states;
- partial/stale status when some accounts fail.

### Custom menus

Configured items support:

- title and optional subtitle;
- shell or launcher action;
- theme, SVG, or image icon;
- filter fields and tags;
- working directory;
- environment variables;
- visibility predicates;
- confirmation;
- row limit and quick-jump alphabet.

`visible_if` accepts:

```text
env:NAME
path:/absolute/or/~/path
command:program
not:<condition>
```

Environment keys must be valid shell identifiers. Working directories and environment values are shell-quoted. At most one icon source may be configured per item.

## Loading, errors, and command completion

Specialized views retain their current snapshot while a refresh or action is running where possible. Busy state, failure text, and successful completion are reflected in the menu. State-changing Wi-Fi and storage commands trigger snapshot refreshes so labels do not remain stale.

The system menu uses the bounded asynchronous `ActionRunner`. Legacy specialized tray commands currently use the tray spawn path and then refresh their provider snapshot. Both paths surface errors instead of silently swallowing them.

## Configuration validation

Validation covers:

- shared row, label, subtitle, and scroll bounds;
- Wi-Fi refresh, row, and timeout bounds;
- storage row bounds, duplicate profiles, required fields, empty SSHFS options, and VCVolume placeholders;
- calendar refresh/agenda/row bounds, required account fields, and duplicate account IDs;
- custom row bounds, quick-jump alphabet, source specs, duplicate IDs, command presence, visibility syntax, filter fields, icon exclusivity, and environment keys;
- system-menu sections, buttons, timeouts, command presence, and duplicate IDs.

When loading the main configuration, each invalid slice falls back independently:

```text
menus.ui       -> MenuUiConfig::default()
menus.wifi     -> WifiMenuConfig::default()
menus.mount    -> MountMenuConfig::default()
menus.calendar -> CalendarMenuConfig::default()
menus.custom   -> CustomMenuConfig::default()
menus.system   -> SystemMenuConfig::default()
```

## Platform boundaries

- Wi-Fi assumes NetworkManager/`nmcli` unless commands/providers are replaced in a future backend.
- Display presets and the default idle controls are X11/xrandr/xset-oriented; override system-menu commands for Wayland/Sway or another session stack.
- Storage actions depend on tools such as `udisksctl`, `sshfs`, `fusermount`, `losetup`, and the configured VCVolume command.
- Calendar launching is command-configurable; CalDAV credential resolution remains outside the renderer.
- Generic DBus menu quality depends partly on what the provider exports.

## Release verification

Recommended gates after menu changes:

```sh
cargo fmt --all -- --check
cargo test -p deskhalloumi-core
cargo test -p deskhalloumi-bin --bin deskhalloumi
cargo check --workspace
scripts/test_safe.sh
CARGO_INCREMENTAL=0 cargo clippy -p deskhalloumi-core -p deskhalloumi-bin --all-targets -- -D warnings
```

Do not live-test suspend, logout, reboot, shutdown, display-layout mutation, Wi-Fi mutation, or destructive storage actions as part of an ordinary automated test run. Test command construction and state transitions with pure fixtures, then perform explicit operator-controlled smoke tests when appropriate.
