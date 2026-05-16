# Unilii Feature Specification: WiFi, Storage/Mount, Calendar, and Keybinding Ergonomics

Date: 2026-04-16  
Status: Draft v1 (implementation-ready baseline)

## 1. Purpose and Scope

This specification defines a production-grade interaction model and implementation contract for:

1. WiFi menu (full network control)
2. Mount menu (local volumes, loop devices, SSHFS, and removable media)
3. Calendar menu (CalDAV-backed personal/remote calendars)
4. Keyboard ergonomics and mechanics for press/release/mod-release keybinding behavior
5. A roadmap to replace external hotkey daemons (for example `sxhkd`) with native unilii key management

The design targets Linux desktop environments where unilii acts as a top bar and command orchestrator.

## 2. Product Goals

- Provide feature-complete, low-latency system menus without requiring users to leave the bar.
- Unify all menus under one interaction pattern (mouse + keyboard + global hotkeys).
- Make global keybindings reliable enough for daily driver use, including release-trigger workflows.
- Reduce dependency on external hotkey daemons by progressively matching and exceeding their capability.
- Keep behavior deterministic, observable, and recoverable under permission, backend, and device failures.

## 3. Non-Goals

- Implementing a full desktop environment shell.
- Building custom network or filesystem stacks; rely on stable system tools/services.
- Replacing enterprise groupware features beyond event read/write and reminders in v1.
- Replacing all tiling-WM scripting immediately; migration is phased and reversible.

## 4. Cross-Menu UX and Mechanics Contract

### 4.1 Shared Menu Model

Every menu implements the same lifecycle:

- `Closed`: icon/summary only
- `Opening`: fetch minimal snapshot, show skeleton rows
- `Ready`: interactive content visible
- `Busy(action_id)`: one action in progress, optimistic UI where safe
- `Error(scope, message, recoverable)`: bounded error pane with retry action
- `Stale`: data displayed, background refresh failed

### 4.2 Shared Interaction Contract

- Left click: open focused menu.
- Right click: open quick actions pane.
- `Esc`: close menu.
- `Tab`/`Shift+Tab`: move focus between actionable rows.
- `Enter`: activate focused row.
- `j/k` and arrow keys: move list selection.
- `f`: focus filter/search field (if present).
- `/`: focus filter/search field and clear previous query (if present).
- `r`: manual refresh.
- Numeric shortcuts (`1..9`): activate first nine visible actions in current section.

### 4.3 Rendering and Performance Targets

- First menu paint: under 60 ms after open (cached snapshot path).
- Fresh snapshot fetch started within 16 ms after open.
- Actions provide visible feedback within 80 ms.
- Background refresh default interval:
  - WiFi: 3-5 s
  - Mount: 2-5 s
  - Calendar: 30-60 s foreground, 5-15 min background

### 4.4 Backend Process Isolation

All shell/system interactions run through a single async command executor with:

- per-command timeout
- stderr capture
- command audit metadata (`menu`, `action`, `duration`, `exit_code`)
- cancellation support when menu closes

## 5. WiFi Menu Specification

### 5.1 User Outcomes

- See current link state, SSID, signal quality, interface, and internet reachability.
- Toggle WiFi radio on/off.
- Scan and connect to visible networks.
- Disconnect current connection.
- Access known networks and forget networks.
- Launch advanced settings in native tool when needed.

### 5.2 Data Sources and Tooling

Primary backend: `nmcli` via NetworkManager.  
Secondary detection: DBus status notifier updates when available.

Required command classes:

- radio status (`nmcli radio wifi`)
- active connection (`nmcli -t -f ... connection show --active`)
- scan list (`nmcli -t -f ... device wifi list`)
- connect/disconnect (`nmcli device wifi connect ...`, `nmcli device disconnect ...`)
- known profiles (`nmcli connection show`)

### 5.3 UI Structure

Sections:

1. Status header (radio state, current SSID, signal icon, internet status)
2. Controls row (`Toggle`, `Disconnect`, `Refresh`, `Settings`)
3. Available networks list (sorted by active first, then signal)
4. Known networks sub-list (priority, autoconnect, forget)
5. Diagnostics footer (interface name, last scan age, backend errors)

### 5.4 Actions and State Rules

- Toggle radio updates header optimistically; rollback if command fails.
- Connect action supports:
  - open networks (immediate)
  - secure networks (credential prompt modal)
- Failed connect keeps menu open and anchors error to target SSID row.
- Manual refresh throttled to one request per 2 s.

### 5.5 Config Contract (TOML)

```toml
[menus.wifi]
enabled = true
backend = "nmcli"
refresh_ms = 4000
max_network_rows = 20
show_known_networks = true
allow_forget = true
settings_command = "nm-connection-editor"
scan_on_open = true
connect_timeout_ms = 20000
```

### 5.6 Failure and Recovery

- Missing `nmcli`: show actionable error with install hint.
- Permission or polkit denial: keep controls disabled until next successful status fetch.
- Interface absent: show "No WiFi adapter" state, keep Settings action available.

## 6. Mount / SSHFS / Loop / VCVolume Menu Specification

### 6.1 User Outcomes

- Mount/unmount removable drives quickly.
- Attach and mount loop images (`.iso`, `.img`) without manual shell commands.
- Mount/unmount SSHFS remotes from saved profiles.
- Monitor active mounts and free space.
- Manage encrypted volumes (VeraCrypt-compatible `vcvolume`) through delegated command hooks.

### 6.2 Domain Model

Mount item classes:

- `BlockDevice`: USB/SATA/NVMe partitions, optical media
- `LoopDevice`: image-backed devices created by unilii action
- `SshfsMount`: remote path mounted via profile
- `VcVolume`: encrypted container/device mounted via configured backend
- `BindMount` (optional phase 2)

Each item exposes:

- id
- class
- source
- target mountpoint
- state (`unmounted`, `mounting`, `mounted`, `error`, `stale`)
- rw mode
- capacity + used % (when measurable)
- last_error

### 6.3 Backend Strategy

Primary backends:

- block devices: `lsblk`, `/proc/mounts`, optional `udisksctl`
- loop: `losetup`, `mount`, `umount`
- sshfs: `sshfs`, `fusermount3`
- vcvolume: `veracrypt --text` (or compatible backend command templates)
- free space: `statvfs` via Rust or `df -P`

Action runner policy:

- all privileged actions go through configured command templates
- no direct hardcoded `sudo`; use user-specified helper command when privilege escalation is required

### 6.4 UI Structure

Sections:

1. Active mounts (with unmount/open action)
2. Available local devices (mount action)
3. SSHFS profiles (mount/unmount/reconnect)
4. Loop images (attach/detach)
5. Quick actions (`Open mount root`, `Rescan`, `Mount all auto`, `Edit profiles`)

### 6.5 SSHFS Profiles

Profiles include:

- name
- host
- user
- remote_path
- local_mount
- port
- identity_file
- options
- automount policy (`manual`, `on_demand`, `on_login`)

Credential handling:

- default: SSH agent / key-based auth
- passphrase prompts delegated to SSH tooling
- secrets never stored by unilii in plaintext

### 6.6 VCVolume Flows

- `Unlock+Mount`: select profile/container -> request passphrase via pinentry/keyring bridge -> mount to configured target.
- `Lock+Unmount`: unmount filesystem -> close encrypted mapping/device.
- Optional read-only mode exposed per profile.
- Backend command templates must allow distro-specific variants while preserving uniform UI semantics.

### 6.7 Loop Image Flows

- `Attach`: choose image file -> create loop -> mount read-only by default
- `Detach`: unmount target -> detach loop device
- validation blocks unsupported files and non-regular files

### 6.8 Config Contract (TOML)

```toml
[menus.mount]
enabled = true
refresh_ms = 3000
mount_root = "/run/media/$USER"
prefer_udisks = true
allow_loop = true
loop_default_read_only = true
show_bind_mounts = false
allow_vcvolume = true
vcvolume_backend = "veracrypt"
vcvolume_mount_timeout_ms = 30000

[[menus.mount.vcvolume_profiles]]
name = "work-vault"
container = "/home/alice/secure/work.vc"
mount_point = "/home/alice/mnt/work-vault"
pim = 0
read_only = false
keyfiles = []
auto_lock_on_suspend = true

[[menus.mount.sshfs_profiles]]
name = "nas-media"
host = "nas.lan"
user = "alice"
remote_path = "/srv/media"
local_mount = "/home/alice/mnt/nas-media"
port = 22
identity_file = "~/.ssh/id_ed25519"
options = "reconnect,ServerAliveInterval=15,ServerAliveCountMax=3"
automount = "on_demand"
```

### 6.9 Failure and Recovery

- Busy unmount returns targeted guidance (`open files`, `cwd inside mount`, `force unmount` policy).
- SSHFS connection errors classify DNS/auth/network to improve retry guidance.
- VCVolume unlock failures classify wrong credential vs backend binary/permission issue.
- Partial action failures always reconcile by resyncing active mount table.

## 7. CalDAV / Calendar Menu Specification

### 7.1 User Outcomes

- View upcoming events from one or multiple calendars.
- Create, edit, and delete events.
- Trigger joins or related commands from events (meeting links, scripts).
- See reminders in bar and invoke quick snooze/dismiss actions.

### 7.2 Scope Tiers

- Phase 1: read-only agenda + reminders + external open link.
- Phase 2: create/update/delete events and recurring-event exception handling.
- Phase 3: availability summary and cross-calendar conflict cues.

### 7.3 Backend Integration

Provider abstraction:

- `CaldavProvider` trait with implementations for generic CalDAV endpoints.

Core operations:

- auth/bootstrap account
- sync calendar list
- sync event windows (`now-30d` to `now+90d` default)
- upsert/delete event
- acknowledge/snooze reminder

Storage:

- local cache (SQLite) for offline agenda and fast open
- per-account sync token / etag tracking for incremental sync

### 7.4 UI Structure

Sections:

1. Today/Next header (current/next event, time to start)
2. Agenda list grouped by date
3. Inline calendar filters
4. Quick compose/edit modal
5. Reminder tray (due soon, overdue)

### 7.5 Reminder Mechanics

- Reminder trigger source: local scheduler based on synced event alarms.
- Duplicate suppression key: `account_id + event_uid + alarm_trigger_time`.
- Snooze presets: 5m, 10m, 30m, custom.
- Dismiss records event-instance key to avoid immediate retrigger.

### 7.6 Config Contract (TOML)

```toml
[menus.calendar]
enabled = true
refresh_ms = 60000
offline_cache = true
default_window_past_days = 30
default_window_future_days = 90
show_declined = false
first_day_of_week = "monday"
time_format = "24h"

[[menus.calendar.accounts]]
name = "work"
server_url = "https://caldav.example.com/"
username = "alice@example.com"
auth = "keyring"
calendar_whitelist = ["Engineering", "OnCall"]
```

### 7.7 Auth and Security

- Secrets stored in OS keyring only.
- Plaintext password in config is unsupported.
- TLS verification required by default; insecure mode gated behind explicit debug flag.

### 7.8 Failure and Recovery

- Auth failures trigger non-destructive re-auth flow.
- Sync conflicts show conflict badge and preserve both local draft and server version until user resolves.
- Network failures degrade gracefully to cached agenda with stale indicator.

## 8. Keybinding Ergonomics and Mechanics Specification

### 8.1 Problem Statement

Users need seamless switching between classic press-based bindings and mod-release workflows without accidental triggers, especially for launcher, chord, and mode actions.

### 8.2 Binding Trigger Types

Each binding defines `trigger`:

- `press`: fire when full chord becomes active
- `release`: fire when designated trigger key is released while chord context is valid
- `modrelease`: fire when modifier in chord is released after a valid hold sequence
- `repeat`: optional, for held directional/volume actions

### 8.3 State Machine

Per keyboard device, maintain:

- pressed key set
- active chord candidates
- latched candidates (press satisfied, waiting for release/modrelease)
- suppression window to prevent duplicate press+release firing

Rules:

- `press` binds resolve first on key-down transitions.
- `release`/`modrelease` require prior activation stamp (chord became valid before release).
- If any required non-mod key leaves pressed set before release trigger, candidate invalidates.
- Overlapping bindings resolve by explicit priority, then most-specific chord (largest key count).

### 8.4 Ergonomic Features

- `hold_ms` threshold for mod-release to avoid accidental taps.
- `tap_vs_hold` discriminator for same chord family.
- per-binding debounce/cooldown.
- optional `strict_device` to tie binding to selected keyboards.
- optional `consume` policy to suppress lower-priority matches.

### 8.5 Config Contract (TOML)

```toml
[[keybindings]]
name = "launcher"
keysym = "Super+Space"
trigger = "modrelease"
hold_ms = 120
command_type = "internal"
command = "bar:toggle:launcher"
priority = 100
consume = true

[[keybindings]]
name = "terminal"
keysym = "Super+Return"
trigger = "press"
command_type = "shell"
command = "alacritty"
priority = 80
```

### 8.6 Compatibility Mode for Existing `sxhkd` Users

- Import tool parses `sxhkdrc` and maps entries to unilii TOML.
- Unsupported patterns are marked with explicit migration warnings.
- Dry-run mode prints parsed matches and conflict report before activation.

## 9. Replacing `sxhkd`-class Tooling: Roadmap

### 9.1 Strategy

Replacement is incremental and reversible. Users can run unilii daemon in shadow mode first, then cut over once parity metrics are met.

### 9.2 Phases

Phase A: Observability and parity baseline (1-2 releases)

- add trigger telemetry and conflict diagnostics
- add dry-run event simulation CLI
- add config validation and linting

Exit criteria:

- deterministic trigger tests pass
- no unhandled parser errors for common chord syntax

Phase B: Functional parity (2-3 releases)

- full press/release/modrelease semantics
- shell/internal command execution parity
- startup daemon reliability and restart strategy
- `sxhkdrc` import and migration report

Exit criteria:

- at least 95% of sampled user bindings migrate without manual rewrite
- no major stuck-key regressions in soak tests

Phase C: Advanced ergonomics (1-2 releases)

- modal layers and temporary keymaps
- tap-hold behaviors
- per-device rule targeting
- profile switching

Exit criteria:

- advanced users can model common WM workflows without external daemon

Phase D: Default-native key stack (1 release)

- mark external daemon integration optional
- include migration wizard and rollback command
- update docs and packaged defaults

Exit criteria:

- user-facing docs provide complete path from `sxhkd` to unilii-native

## 10. Architecture and Module Boundaries

Suggested crate/module additions:

- `unilii/bin/src/menus/wifi.rs`
- `unilii/bin/src/menus/mount.rs`
- `unilii/bin/src/menus/calendar.rs`
- `unilii/bin/src/menus/i3.rs`
- `unilii/bin/src/menus/tmux.rs`
- `unilii/core/src/key_engine.rs` (state machine + resolution)
- `unilii/core/src/key_import_sxhkd.rs`
- `unilii/lib/src/calendar/` (provider + cache)
- `unilii/lib/src/i3/` (workspace snapshot + command wrapper)
- `unilii/lib/src/tmux/` (session/window/pane snapshot + command wrapper)

Key interfaces:

- `MenuController` trait for shared menu lifecycle
- `ActionRunner` for command execution and cancellation
- `SnapshotProvider<T>` for menu data refresh
- `KeyEngine` trait for deterministic trigger resolution

## 11. Reliability, Security, and Privacy Requirements

- Every action path must be idempotent or explicitly marked non-idempotent.
- Secrets and tokens are never logged.
- Menu actions that can cause data loss (unmount force, calendar delete) require confirmation gate.
- Structured logs include correlation IDs for command and menu actions.

## 12. Testing and Validation Matrix

### 12.1 Unit Tests

- key trigger state machine (press/release/modrelease overlap cases)
- config parsing and defaults
- command template rendering
- calendar recurrence and reminder dedup logic

### 12.2 Integration Tests

- WiFi snapshot parse against fixture outputs
- mount table reconciliation under rapid mount/unmount
- SSHFS mount/unmount command orchestration
- CalDAV sync token update and conflict handling
- i3 tree/workspace parse and focused-window switch command orchestration
- tmux session/window/pane snapshot parse and command orchestration

### 12.3 End-to-End Scenarios

- connect to known WiFi while menu open and via hotkey
- mount USB + open path + safely unmount
- attach ISO loop + mount read-only + detach
- reminder triggers and snooze persistence through restart
- switch i3 workspace and focused window from menu and hotkey
- attach to tmux session, switch panes/windows, and send command from menu
- migrate `sxhkd` config, run shadow mode, then cut over

### 12.4 Performance and Soak

- 24h daemon soak with synthetic key streams
- repeated menu open/close action latency checks
- high event volume keyboard stress for stuck-state prevention

## 13. Telemetry and Debuggability

Core counters:

- key trigger attempts/success/fail by trigger type
- menu open latency percentiles
- action failure counts by backend command
- calendar sync duration and stale-cache age
- i3 command success/failure counts by action
- tmux command success/failure counts by action

Debug tools:

- `unilii --key-debug` live event trace (redacted)
- `unilii --menu-debug wifi|mount|calendar|i3|tmux` backend snapshots
- conflict inspector for overlapping keybindings

## 14. Delivery Order Recommendation

1. Harden key engine semantics and diagnostics first.
2. Upgrade WiFi menu to full parity (fastest visible win).
3. Deliver mount/sshfs/loop menu next (high utility for power users).
4. Deliver calendar read-only first, then write flows.
5. Deliver i3 window/workspace control menu and hotkey actions.
6. Deliver tmux remote-control menu and hotkey actions.
7. Add migration tooling and default-native rollout for external hotkey replacement.

## 15. Open Design Decisions (Default Recommendations)

- Privilege model for mount operations: prefer `udisks` path first, fallback to configurable helper command.
- Calendar edit scope in first write release: support single and recurring instances, but postpone complex recurrence editor UX.
- Keybinding modal layers: ship as opt-in profiles before default enablement.
- i3 scope boundary: support i3 IPC via `i3-msg` first, evaluate native IPC socket client later.
- tmux backend mode: CLI-first in v1, optional direct libtmux-style protocol layer in v2.

## 16. i3 Window/Workspace Switching (First-Class Spec)

### 16.1 User Outcomes

- See active i3 workspaces, current focused workspace, and urgent indicators.
- Switch workspace and focus windows from a unified menu.
- Move focused container to another workspace.
- Execute common layout actions (split, tabbed/stacking toggle, fullscreen toggle).
- Trigger all above via both menu actions and internal keybinding commands.

### 16.2 Backend and Data Sources

Primary backend: `i3-msg` JSON interfaces.

Command classes:

- workspace snapshot: `i3-msg -t get_workspaces`
- tree snapshot: `i3-msg -t get_tree`
- outputs snapshot: `i3-msg -t get_outputs`
- actions: `i3-msg workspace <name>`, `i3-msg [con_id=<id>] focus`, `i3-msg move container to workspace <name>`

### 16.3 UI Structure

Sections:

1. Focus header (workspace, output, layout, floating/fullscreen status)
2. Workspace strip (ordered, urgent badge, visible/focused markers)
3. Window list for focused workspace (name, app_id/class, marks, urgent/floating flags)
4. Quick actions (`Next/Prev workspace`, `Move focused -> workspace`, `Toggle floating/fullscreen`, `Reload i3 config`)

### 16.4 Action Semantics

- Workspace switch is optimistic in UI, then reconciled with fresh `get_workspaces` snapshot.
- Window focus action targets `con_id` when available; fallback to mark criteria only if configured.
- Move-container action requires focused container present; disabled when focus cannot be resolved.
- Actions are idempotent when invoked repeatedly on already-focused/already-selected targets.

### 16.5 Config Contract (TOML)

```toml
[menus.i3]
enabled = true
backend = "i3-msg"
refresh_ms = 1500
show_empty_workspaces = true
workspace_sort = "numeric_then_lexicographic"
max_window_rows = 30
allow_layout_actions = true
reload_command = "i3-msg reload"

[menus.i3.hotkeys]
next_workspace = "bar:i3:workspace:next"
prev_workspace = "bar:i3:workspace:prev"
focus_next_window = "bar:i3:window:next"
focus_prev_window = "bar:i3:window:prev"
```

### 16.6 Failure and Recovery

- If i3 IPC unavailable, show `i3 not running / unreachable` with retry action.
- Parse errors from `i3-msg` output classify as backend error and disable destructive actions.
- On action failure, immediately invalidate optimistic state and reload workspace/tree snapshots.

### 16.7 i3 Filter + Quickjump Exposure

- i3 menu exposes filter tokens per row:
  - workspace rows: workspace name, numeric index, output name, focused/visible/urgent markers
  - window rows: title, app class/app id, mark names, workspace name, output name
- Quickjump mode can target:
  - visible workspace rows
  - visible window rows (focused workspace first)
- Default quickjump order:
  1. focused workspace
  2. urgent workspaces
  3. remaining visible workspaces
  4. window rows in current render order
- Default quickjump action:
  - workspace target -> switch workspace
  - window target -> focus container (`con_id`)

## 17. tmux Remote Control (First-Class Spec)

### 17.1 User Outcomes

- See sessions, windows, panes, and active client status in one menu.
- Switch sessions/windows/panes quickly from bar UI.
- Create/rename/kill sessions/windows with confirmation policy.
- Send command text to target pane.
- Use internal key actions to drive tmux even when menu is closed.

### 17.2 Backend Strategy

V1 backend: `tmux` CLI command adapter.  
V2 optional backend: direct protocol/lib adapter for lower-latency bulk operations.

Command classes:

- snapshot: `tmux list-sessions`, `tmux list-windows -a`, `tmux list-panes -a`
- control: `tmux switch-client`, `tmux select-window`, `tmux select-pane`
- lifecycle: `tmux new-session`, `tmux kill-session`, `tmux rename-session`, `tmux new-window`, `tmux kill-window`
- command send: `tmux send-keys -t <target> <cmd> C-m`

### 17.3 UI Structure

Sections:

1. Active target header (socket, session, window, pane)
2. Session list with nested windows and panes
3. Quick actions (`New session`, `New window`, `Attach`, `Kill`, `Rename`)
4. Command input row (`send to pane`) with recent-command history

### 17.4 Targeting and Safety Semantics

- Every action resolves explicit tmux target (`session:window.pane`) before execution.
- Destructive actions (`kill-session`, `kill-window`) require confirmation.
- `send-keys` supports dry-preview mode for escaped command visibility before send.
- Multi-server support via socket profiles; each menu instance binds one active socket context.

### 17.5 Config Contract (TOML)

```toml
[menus.tmux]
enabled = true
backend = "cli"
refresh_ms = 1200
default_socket = "default"
show_detached_sessions = true
max_sessions = 20
max_windows_per_session = 30
max_panes_per_window = 20
enable_send_keys = true

[[menus.tmux.sockets]]
name = "default"
socket_path = "/tmp/tmux-1000/default"

[[menus.tmux.sockets]]
name = "work"
socket_path = "/tmp/tmux-work.sock"
```

### 17.6 Failure and Recovery

- Missing `tmux` binary: menu enters degraded mode with install hint.
- Socket unreachable or permission denied: scope error to affected socket profile only.
- Action-time failures trigger snapshot resync and retain prior selected target if still present.

### 17.7 tmux Filter + Pane Quickjump Exposure

- tmux menu exposes filter tokens per row:
  - session rows: session name, attached/detached status, socket profile
  - window rows: session name, window index, window name, activity flags
  - pane rows: pane index, pane title/path/current command, pane id
- Quickjump mode can target:
  - window rows
  - pane rows (primary pane navigation path)
- Default quickjump order:
  1. active pane
  2. panes in active window (index order)
  3. remaining visible panes
  4. visible windows
- Default quickjump action:
  - pane target -> `select-pane -t <target>`
  - window target -> `select-window -t <target>`

## 18. Custom Menu and Launcher Specification

### 18.1 User Outcomes

- Define arbitrary menu items from TOML, including shell actions and launcher actions.
- Attach custom menu to one or more tray app IDs or icon-name patterns.
- Show icon metadata per item (`theme icon`, `svg path`, `image path` such as `png`/`jpg`/`webp`).
- Compose menu content from multiple includable TOML files.
- Use for workflows like monitor profiles (`xrandr` scripts), app launchers, and scripted routines.

### 18.2 Config Model (Hybrid Include + Sources)

```toml
[menus.custom]
enabled = true
app_ids = ["custom-launcher"]
icon_name_patterns = ["launcher", "custom-menu"]
quickjump_alphabet = "asdfjkl;ghqwertyuiopzxcvbnm"
include = [
  "~/.config/unilii/menus/display.toml",
  "~/.config/unilii/menus/apps/*.toml",
]

[[menus.custom.sources]]
enabled = true
glob = "~/.config/unilii/menus/work/*.toml"
priority = 20

[[menus.custom.items]]
id = "display.docked"
title = "Docked Layout"
subtitle = "external monitor only"
action = "shell"
command = "~/.local/bin/xrandr-docked.sh"
filter_fields = ["title", "subtitle", "command", "tags"]
tags = ["display", "xrandr"]
confirm = false

[[menus.custom.items]]
id = "app.terminal"
title = "Terminal"
action = "launcher"
command = "alacritty"
args = ["--class", "main-term"]
desktop_id = "Alacritty.desktop"
```

### 18.3 Include Resolution and Merge Policy

- Supported include mechanisms:
  - `include = [...]` (simple ordered list)
  - `[[sources]]` (`path`/`glob`, `enabled`, `priority`)
- Resolution order:
  1. inline `items`
  2. `include` entries in declaration order
  3. `sources` sorted by `priority` then declaration order
- Relative paths resolve from the parent config file directory.
- Cycles are suppressed by canonical-path dedup.
- Duplicate `id` policy: last definition wins, warning logged.

### 18.4 Item Schema

- Required: `id`, `title`, action discriminator + action payload.
- Action variants:
  - `shell`: raw command string
  - `launcher`: command + args + optional desktop-id metadata
  - `choose_from_stdin`: `producer` + `chooser` + `consumer` pipeline
- Optional display metadata:
  - `subtitle`
  - `icon.theme_icon`
  - `icon.svg_path`
  - `icon.image_path`
- Optional behavior metadata:
  - `filter_fields` (controls exposed filter tokens)
  - `tags`
  - `working_dir`
  - `env`
  - `confirm`
  - `visible_if` (future expression gate)

## 19. Low-Level Cross-Menu Interfaces

### 19.1 Filtering Interface (`FilterableMenu`)

- Any menu can implement `FilterableMenu`.
- Contract:
  - expose `filter_tokens_for(item_id) -> Vec<String>`
  - menu decides token scope (title-only, title+path, title+command, etc.)
- Query semantics:
  - case-insensitive
  - whitespace-split terms
  - AND matching across terms (all terms must match at least one token)
- Applies uniformly to tray aggregate, wifi, mount, calendar, i3, tmux, and custom menus.
- UI text-input field contract:
  - Every `FilterableMenu` may expose a visible text input field at top of menu content.
  - Field value is menu-local state (`filter_query`) and does not leak between menus by default.
  - Keystrokes while filter field is focused update `filter_query`; matching updates visible rows incrementally.
  - `Enter` while focused:
    - if one visible actionable row remains, trigger it
    - otherwise, move focus back to row list
  - `Esc` while focused clears query first; second `Esc` closes menu.
  - `Ctrl+Backspace` removes previous token; `Ctrl+u` clears full query.
  - Menus without filter support remain valid and ignore filter focus actions.

### 19.2 Quickjump Interface (`QuickjumpMenu`)

- Any menu can implement `QuickjumpMenu`.
- Contract:
  - expose quickjump targets in visible selection order
  - provide alphabet (configurable; default home-row biased)
  - derive deterministic labels (`a,s,d,...` then two-character combinations)
- Behavior:
  - quickjump action opens overlay
  - typed label activates target action immediately
  - `Esc`/cancel closes overlay without side effects
- Label generation policy follows ace/avy-style shortest unique prefixes in render order.

### 19.3 Choose-From-STDIN Interface (`StdinChoiceAction`)

- Purpose: support dmenu/fzf-like flows where an action generates candidate lines, user picks one, then a command consumes it.
- Action contract (custom menu item metadata):
  - `action = "choose_from_stdin"`
  - `producer`: command that writes newline-delimited candidates to stdout
  - `chooser`: command that reads stdin and writes selected line(s) to stdout (e.g. `fzf`, `dmenu -l 20`)
  - `consumer`: command template receiving selection via env var and/or stdin
- Execution pipeline:
  1. run `producer`
  2. pipe to `chooser`
  3. if non-empty selection, invoke `consumer`
  4. surface cancel/no-selection as non-error user cancellation
- Transport model:
  - default: one-shot process pipeline (no long-lived socket required)
  - optional: Unix domain socket chooser daemon for low-latency/high-frequency invocations
    - recommended only when startup overhead of chooser dominates UX
    - failure to connect must gracefully fallback to one-shot pipeline
- Security and safety:
  - preserve existing command timeout/audit model
  - shell interpolation rules must be explicit (no implicit unescaped substitution)
  - enforce max candidate bytes/lines to avoid unbounded memory growth
