# Installing DeskHalloumi

DeskHalloumi currently publishes a prebuilt Linux x86-64 archive for each
supported GitHub release. Building from source remains the recommended path for
other architectures, distributions with incompatible system libraries, and
contributors.

## Prebuilt Linux archive

Open the GitHub release page and download both files for the desired version:

```text
deskhalloumi-<version>-x86_64-unknown-linux-gnu.tar.gz
deskhalloumi-<version>-x86_64-unknown-linux-gnu.tar.gz.sha256
```

For `0.2.0`, the release page is:

<https://github.com/fkr-0/deskhalloumi/releases/tag/v0.2.0>

Verify the archive from the directory containing both downloaded files:

```sh
sha256sum -c deskhalloumi-0.2.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Extract it:

```sh
tar -xzf deskhalloumi-0.2.0-x86_64-unknown-linux-gnu.tar.gz
cd deskhalloumi-0.2.0-x86_64-unknown-linux-gnu
```

The archive contains these primary commands:

```text
deskhalloumi
deskhalloumi-bar
deskhalloumi-copyq
deskhalloumi-filter-tab
deskhalloumi-hotkeyd
deskhalloumi-i3-vis
```

It also contains lightweight `unilii-*` compatibility launchers for existing
pre-1.0 scripts and window-manager configuration.

`deskhalloumi` is the supported interactive desktop runtime. The separate
`deskhalloumi-bar` command is intentionally a synchronous, headless reference
runtime for configuration, fixture, scheduler, and reload diagnostics; it is
not a second graphical panel daemon. Its role can be inspected with:

```sh
deskhalloumi-bar --runtime-contract
```

### Local user installation

Install the commands under `~/.local/bin`:

```sh
install -d "$HOME/.local/bin"
install -m755 bin/* "$HOME/.local/bin/"
```

Ensure `~/.local/bin` is in `PATH`. For a shell profile:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Do not copy the compatibility launchers selectively if existing scripts still
refer to them.

### System-wide installation

A system administrator can install the commands under `/usr/local/bin`:

```sh
sudo install -m755 bin/* /usr/local/bin/
```

The generic release archive does not register files with a package manager.
Prefer a distribution package once one is available if uninstall tracking and
automatic upgrades are required.

## Runtime dependencies

DeskHalloumi is a Linux desktop application. Depending on the enabled surface,
it expects common X11/Wayland, DBus, input, and desktop command-line libraries or
utilities. The exact dynamic-library requirements can be inspected with:

```sh
ldd "$HOME/.local/bin/deskhalloumi"
```

The supported global-hotkey environment is i3/X11. Advanced X11 hotkeys may
need access to the X server, while the raw evdev backend additionally needs
suitable `/dev/input` permissions. Prefer generated i3 bindings or the selective
X11 backend unless raw device access is specifically required.

Desktop actions may call tools such as `i3-msg`, `nmcli`, `pactl`, `xrandr`,
`xset`, `systemctl`, `copyq`, or `tmux`. Missing optional tools should affect
only the associated feature, but configuration should override commands when a
distribution uses different paths.

## Configuration

Primary configuration and runtime locations are:

```text
~/.config/deskhalloumi/
$XDG_RUNTIME_DIR/deskhalloumi/
```

Legacy `unilii` locations remain readable during the 0.x compatibility period.
New DeskHalloumi environment variables and paths take precedence.

Start from the included examples:

```sh
install -d "$HOME/.config/deskhalloumi"
cp share/doc/deskhalloumi/examples/deskhalloumi.toml \
  "$HOME/.config/deskhalloumi/deskhalloumi.toml"
```

See the [configuration reference](../CONFIGURATION.md) and
[keybinding guide](../KEYBINDINGS.md).

## Running

Show command help:

```sh
deskhalloumi --help
deskhalloumi-hotkeyd --help
```

A combined i3/X11 setup commonly runs the bar without its embedded hotkey
worker and starts the standalone supervisor separately:

```sh
deskhalloumi run --no-hotkeyd
deskhalloumi-hotkeyd \
  --config "$HOME/.config/deskhalloumi/hotkeys.toml" \
  --watch
```

## systemd user service

The release archive includes example user units under:

```text
share/doc/deskhalloumi/contrib/systemd/user/
```

Install the primary unit without enabling it automatically:

```sh
install -d "$HOME/.config/systemd/user"
install -m644 \
  share/doc/deskhalloumi/contrib/systemd/user/deskhalloumi-hotkeyd.service \
  "$HOME/.config/systemd/user/"
systemctl --user daemon-reload
```

Inspect and adjust paths before enabling:

```sh
systemctl --user cat deskhalloumi-hotkeyd.service
systemctl --user enable --now deskhalloumi-hotkeyd.service
systemctl --user status deskhalloumi-hotkeyd.service
```

Do not enable both the DeskHalloumi and legacy `unilii` hotkey services. The
provided units conflict with each other, but configuration should still have one
clear owner for global shortcuts.

## Building from source

Install Rust using rustup and the system development libraries required by Iced,
X11/Wayland, and libudev. Then:

```sh
git clone https://github.com/fkr-0/deskhalloumi.git
cd deskhalloumi
cargo build --release -p deskhalloumi-bin
```

The binaries are written under `target/release/`. Run the repository safety
gates before installing a custom build:

```sh
cargo fmt --all -- --check
scripts/test_safe.sh
scripts/test_i3_hotkeys.sh
CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings
```

## Upgrade

1. Download and verify the new release archive.
2. Stop the user services or panel process.
3. Back up `~/.config/deskhalloumi`.
4. Replace the installed binaries atomically where practical.
5. Run `deskhalloumi --version` and strict configuration validation.
6. Start the services and inspect logs.

Configuration migrations must preserve the documented 0.x compatibility
contract. Read the release notes and `CHANGELOG.md` before upgrading.

## Rollback

Keep the previous verified archive or package available. Stop the current
processes, reinstall the prior binaries, restore configuration only when the
newer version changed it incompatibly, and restart the user units. Release tags
and attached assets are immutable inputs for rollback.

## Removal

For a user-local archive installation:

```sh
rm -f "$HOME/.local/bin/deskhalloumi"*
rm -f "$HOME/.local/bin/unilii"*
systemctl --user disable --now deskhalloumi-hotkeyd.service 2>/dev/null || true
rm -f "$HOME/.config/systemd/user/deskhalloumi-hotkeyd.service"
systemctl --user daemon-reload
```

Configuration is intentionally not deleted automatically. Remove
`~/.config/deskhalloumi` only after reviewing or backing it up.
