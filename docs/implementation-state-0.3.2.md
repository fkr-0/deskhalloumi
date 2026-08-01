# DeskHalloumi implementation state at 0.3.2

Snapshot: 2026-08-02

Baseline: annotated local tag `v0.3.2` at `84e9ca856a25a7988373d3622d4be9cb51646974`

This review describes the coordinated Cargo workspace as implemented at the
0.3.2 tag. It separates shipped foundations from transitional code and provides
the evidence used to define the 0.3.3, 0.4.0, and 1.0.0 roadmap horizons.

## Release and verification state

The repository contains seven first-party Cargo packages that inherit the same
workspace version:

| package | current role | 0.3.2 state |
| --- | --- | --- |
| `deskhalloumi` | small public facade/helper crate | builds and tests; public purpose is still underspecified |
| `deskhalloumi-core` | runtime, action, provider, menu, configuration, hotkey, i3/X11, and IPC contracts | strongest shared foundation; still exposes some renderer-coupled plugin API |
| `deskhalloumi-lib` | Linux data-source adapters and CalDAV/cache helpers | useful hardware/service boundary; documentation and backend injection remain uneven |
| `deskhalloumi-bin` | interactive bar, hotkey daemon, auxiliary frontends, compatibility launchers | feature-complete baseline with the largest architectural concentration and migration debt |
| `deskhalloumi-clock` | clock module/provider | small and deterministic, but directly reads wall-clock time rather than an injected backend |
| `deskhalloumi-battery` | battery module/provider | functional on battery hardware; startup and subscription still directly discover live sysfs devices |
| `deskhalloumi-tmux` | tmux pane module/provider | bounded command execution and queueing are present; command/service injection remains incomplete |

There are no Git submodules and no nested release repositories. All packages are
one coordinated release unit represented by one annotated workspace tag.

The 0.3.2 tag was created only after these local gates passed:

- `cargo fmt --all -- --check`;
- release metadata validation in candidate and release modes;
- `scripts/test_safe.sh`, including the live-session command audit and full
  workspace test suite;
- `scripts/test_i3_hotkeys.sh` in isolated Xvfb/i3 instances;
- `CARGO_INCREMENTAL=0 cargo clippy --workspace --all-targets -- -D warnings`;
- independent OpenCode review with no release-blocking findings.

The tag and branch have not been pushed and no remote release has been created.

## Shipped foundations

### Runtime and action execution

`deskhalloumi-core` contains a supervised Tokio runtime, cancellation tokens,
bounded spawn admission, bounded action execution, output limits, process-group
cleanup, timeout accounting, and runtime metrics. Active and queued action gauges
are cancellation-safe. The local action bus and hotkey control socket enforce
bounded newline-delimited UTF-8 frames, connection admission, and I/O timeouts.

These are production foundations, not future roadmap items. Later releases
should complete migration to them and improve operator visibility rather than
creating parallel runtimes.

### Provider lifecycle

Typed provider contracts, snapshots, refresh policy, health, last-known-good
retention, refresh generations, instance generations, watch channels, and
bounded keyed refresh admission are present. Clock, battery, network, audio,
system, and Tmux declare lifecycle contracts.

The remaining gap is behavioral completeness: declaring a test backend by name
is not the same as constructing each production provider through an injected,
hardware-free backend. Battery and Tmux still reach live services from their
module constructors or workers, and clock still reads `Local::now()` directly.

### Menus, tray, and interaction

A renderer-neutral `MenuModel`, typed actions, quick-select, action history, and
CLI introspection exist. The binary crate also contains substantial tested tray,
DBus, menu, widget, and update modules.

The migration is not complete. Several menu and enhanced-tray modules retain
local `dead_code` allowances and comments identifying planned canonical wiring.
`main.rs` remains the primary composition and update boundary, and renderer-
specific types still coexist with shared core models.

### Hotkeys and i3/X11

The supported global-shortcut boundary is explicit and well tested:

- generated i3 press/release bindings;
- recursive i3 configuration auditing;
- transactional generated-include replacement and rollback;
- selective native X11 handling for advanced semantics;
- dynamic evdev keyboard hot-plug;
- bounded action and event queues;
- exact/approximate/unsupported sxhkd migration diagnostics.

Logical versus physical key semantics and layout-sensitive migration remain open.
Sway/Wayland global-hotkey parity is not implemented or claimed.

### Release engineering

The workspace has coordinated versions, immutable annotated-tag policy,
deterministic Linux x86-64 archive assembly, checksums, artifact smoke tests,
release metadata validation, and an early disk-space preflight. The release
workflow can publish an immutable tag but does not publish crates automatically.

## Main architectural debt

### `deskhalloumi-bin` concentration

`unilii/bin/src/main.rs` is still roughly 193 KiB and owns broad portions of
bootstrap, runtime ownership, window/update dispatch, menu/tray coordination,
provider wiring, and rendering. Extraction has begun (`app.rs`,
`action_routing.rs`, `bar_control.rs`, `startup.rs`, `update/`, and dedicated
menu/tray modules), but the composition root is not yet thin.

Broad or module-level `dead_code` allowances remain in enhanced tray, menus,
widgets, and compatibility-oriented paths. They are useful markers of incomplete
migration but must not become the 1.0 steady state.

### Public API ambiguity

The top-level `deskhalloumi` crate exposes only a small utility module while
`deskhalloumi-core` exposes a broad API, including `iced::Element` in the
`Module` trait. The workspace has not yet decided which crates are intended for
third-party consumption, which are implementation details, and whether any
first-party crates will be published independently.

A 1.0 release requires an explicit supported-surface inventory rather than
implicitly stabilizing every currently public Rust item.

### Plugin contract mismatch

The plugin crates declare provider lifecycle metadata, but the current `Module`
trait combines lifecycle, mutable model state, and Iced rendering. The plugins
are compile-time optional dependencies rather than a separately versioned
dynamic plugin ABI.

That is a valid design, but it must be made explicit. The roadmap should not
promise dynamic loading unless a separate requirement and security model are
accepted.

### Repository and documentation residue

Some source comments still use the legacy `unilii` name, and tracked archival
`.bak` files remain under `unilii/src/`. `tasks.yml` retains useful historical
review evidence but also contains old line counts, old names, and completed
migration context. These do not block 0.3.2, but they reduce the accuracy of
future architecture reviews.

## Risk assessment

| risk | current mitigation | remaining requirement |
| --- | --- | --- |
| stale or overlapping async work | generations, cancellation, bounded supervisors | provider and reload churn soak tests |
| UI/runtime coupling | extracted core contracts and partial update modules | thin binary composition root and one authoritative state path |
| hardware-dependent tests | fixtures and selected test backends | real backend injection for every production provider |
| compatibility maintenance cost | explicit legacy launchers and path precedence | telemetry-free inventory and documented 1.0 retirement decision |
| Linux artifact portability | documented glibc/system-library boundary | native AArch64 lane, packaging, and post-publication installation tests |
| key-layout ambiguity | exact/approximate migration diagnostics | explicit physical/logical semantics and de-DE/en-US tests |
| accidental API stabilization | pre-1.0 status | supported-surface inventory and crate publication decision before 1.0 |

## Version-boundary conclusion

The next patch must remain narrow: release-state correctness, regression closure,
repository hygiene, and test seams without new compatibility promises.

The next minor should complete the provider lifecycle migration and make its
health visible end to end, while preserving the i3/X11 and compatibility
contracts.

The next major should stabilize only an intentionally selected set of CLI,
configuration, IPC, package, service, and Rust/plugin interfaces. Wayland parity
is not a prerequisite for 1.0 if i3/X11 remains the explicit supported platform
boundary.
