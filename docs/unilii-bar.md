# deskhalloumi-bar

`deskhalloumi-bar` is the renderer-neutral scaffold for the planned Polybar replacement. It currently provides typed configuration, module metadata, a headless scheduler/reload loop, command actions, and tested runtime modules. Native Makepad rendering is still pending.

## Commands

```sh
cargo run -p deskhalloumi-bin --bin deskhalloumi-bar -- --print-default-config
cargo run -p deskhalloumi-bin --bin deskhalloumi-bar -- --check-config --config templates/bar.toml
cargo run -p deskhalloumi-bin --bin deskhalloumi-bar -- --config templates/bar.toml
cargo run -p deskhalloumi-bin --bin deskhalloumi-bar -- --config templates/bar.toml --watch --ticks 3 --tick-interval-ms 1000
```

The regular `deskhalloumi` CLI can generate and validate bar configuration:

```sh
cargo run -p deskhalloumi-bin --bin deskhalloumi -- init-bar-config --output ~/.config/deskhalloumi/bar.toml
cargo run -p deskhalloumi-bin --bin deskhalloumi -- validate-bar-config --config ~/.config/deskhalloumi/bar.toml
```

## Config discovery

`deskhalloumi-bar` accepts `--config <path>`. Without it, it tries configured/default paths and finally falls back to the built-in starter config.

Lookup inputs:

- `UNILII_BAR_CONFIG`
- `$XDG_CONFIG_HOME/unilii/bar.toml`
- `$HOME/.config/unilii/bar.toml`
- the project directory fallback from `directories::ProjectDirs`

## Runtime modules

Current renderer-neutral modules:

| type | current support | notes |
| --- | --- | --- |
| `workspaces` | env, JSON file, command, i3/sway backend preset | Click-to-switch is exposed via runtime dispatch; Makepad click wiring is pending. |
| `window_title` | env, tree JSON file, command, i3/sway backend preset | Extracts focused title/name from i3/sway tree JSON. |
| `clock` | local time formatting | Uses chrono format strings. |
| `system` | load and memory | CPU and temperature remain pending. |
| `network` | sysfs-style interface status, MAC address, IP, SSID, status file/env, command fallbacks | Netlink/NetworkManager native backends remain pending. |
| `vpn` | tunnel interface detection | Detects `tun*`, `tap*`, `wg*`, `ppp*`, `tailscale*`, `zt*`. |
| `audio` | `UNILII_BAR_AUDIO_STATUS` fixture parser | PipeWire/PulseAudio direct backends remain pending. |
| `battery` | sysfs power_supply | Supports charging, warning, critical, and ok states. |
| `script` | shell command with timeout and output limit | Currently synchronous inside the headless runtime. |
| `notifications` | env count or source file | Full notification daemon is not in scope yet. |

## i3 and sway backend presets

For workspace and focused-title modules, set `backend = "i3"` or `backend = "sway"`.

Preset mappings:

| backend | workspaces command | tree command | switch template |
| --- | --- | --- | --- |
| `i3` | `i3-msg -t get_workspaces` | `i3-msg -t get_tree` | `i3-msg workspace -- {workspace_shell}` |
| `sway` | `swaymsg -t get_workspaces` | `swaymsg -t get_tree` | `swaymsg workspace -- {workspace_shell}` |

Explicit `command`, `workspaces_command`, `tree_command`, or `switch_command_template` values override presets.

Example:

```toml
[[module]]
id = "workspaces"
type = "workspaces"
backend = "i3"
format_active = "[{name}]"
format_inactive = "{name}"
separator = " "

[[module]]
id = "window_title"
type = "window_title"
backend = "i3"
format = "{title}"
max_len = 80
```

## Network enrichment

The `network` module supports `{interface}`, `{state}`, `{address}`, `{ip}`, and `{ssid}` placeholders.

Resolution order for IP and SSID:

1. sysfs-style fixture files under the selected interface directory, such as `ipv4`, `ip_address`, or `ssid`
2. module `status_file` or `UNILII_BAR_NETWORK_STATUS_FILE` containing key/value data
3. `UNILII_BAR_NETWORK_STATUS`
4. direct env overrides, `UNILII_BAR_NETWORK_IP` and `UNILII_BAR_NETWORK_SSID`
5. module `ip_command`/`ssid_command` or env command fallbacks

Example:

```toml
[[module]]
id = "network"
type = "network"
interface = "wlan0"
format = "{interface} {state} {ip} {ssid}"
ip_command = "ip -o -4 addr show wlan0 | awk '{print $4; exit}'"
ssid_command = "iwgetid -r"
```

## Environment fixture inputs

These inputs are useful for testing and for custom integrations:

| variable | use |
| --- | --- |
| `UNILII_BAR_WORKSPACES` | Simple workspace snapshot, for example `1:A,2:V,3:U`. |
| `UNILII_BAR_I3_WORKSPACES_JSON` | Inline i3/sway workspace JSON. |
| `UNILII_BAR_I3_WORKSPACES_FILE` | File containing i3/sway workspace JSON. |
| `UNILII_BAR_WINDOW_TITLE` | Focused title override. |
| `UNILII_BAR_I3_TREE_JSON` | Inline i3/sway tree JSON. |
| `UNILII_BAR_I3_TREE_FILE` | File containing i3/sway tree JSON. |
| `UNILII_BAR_SYS_CLASS_NET` | Fixture root for network interfaces. |
| `UNILII_BAR_NETWORK_STATUS` | Inline network key/value data such as `ip=192.0.2.10/24 ssid=OfficeNet`. |
| `UNILII_BAR_NETWORK_STATUS_FILE` | File containing network key/value data. |
| `UNILII_BAR_NETWORK_IP` | Direct IP override. |
| `UNILII_BAR_NETWORK_SSID` | Direct Wi-Fi SSID override. |
| `UNILII_BAR_NETWORK_IP_COMMAND` | Command whose first stdout line is used as IP. |
| `UNILII_BAR_NETWORK_SSID_COMMAND` | Command whose first stdout line is used as SSID. |
| `UNILII_BAR_POWER_SUPPLY_ROOT` | Fixture root for battery devices. |
| `UNILII_BAR_PROC_ROOT` | Fixture root for load/memory files. |
| `UNILII_BAR_AUDIO_STATUS` | Audio parser fixture, for example `device=sink0 volume=42% muted`. |
| `UNILII_BAR_NOTIFICATION_COUNT` | Notification count override. |
| `UNILII_BAR_NOTIFICATION_FILE` | File containing `count=<n>` or a raw number. |

## i3 startup example

```i3
# Disable/replace an existing Polybar startup line first.
exec_always --no-startup-id cargo run -p deskhalloumi-bin --bin deskhalloumi-bar -- --config ~/.config/deskhalloumi/bar.toml --watch
```

Until native Makepad rendering/window reservation lands, this scaffold prints a grouped render model rather than opening a real bar window.

## sway startup example

```sway
# Disable/replace an existing Polybar startup line first.
exec_always cargo run -p deskhalloumi-bin --bin deskhalloumi-bar -- --config ~/.config/deskhalloumi/bar.toml --watch
```

Layer-shell/window reservation is pending with the native renderer.

## Troubleshooting

- `Unavailable` workspaces usually means no env/file/command/backend source returned parseable JSON.
- `Unavailable` audio means `UNILII_BAR_AUDIO_STATUS` is unset and no direct audio backend has been implemented yet.
- `Disconnected` VPN means no matching tunnel interface was found.
- Script/action modules enforce timeouts and output limits, but command execution is still synchronous in the current scaffold.
- If a full workspace test fails due disk space, clean build artifacts with `cargo clean` and retry.


## Test safety audit

Normal test runs must not mutate a developer's live desktop session. The canonical safe test command is:

```sh
scripts/test_safe.sh
```

The audit flags test code that appears to execute live-session commands such as WM, network, audio, power, login, or notification commands. Tests that only parse or render command text should use explicit comments with this marker:

```rust
// unilii-audit: allow-live-session-command-reference -- this test only asserts data; it does not execute commands.
```

Rendered-command assertions should prefer non-executing helpers. Dispatch tests should use harmless mock commands such as `printf`.
