# ADR 0001: Do not publish a musl artifact for DeskHalloumi 0.4

- Status: Accepted
- Date: 2026-08-02
- Decision owners: DeskHalloumi maintainers
- Applies to: 0.4.x release train

## Context

DeskHalloumi is a Linux desktop-control application rather than a self-contained
terminal utility. Its production binary graph includes Iced/Winit/WGPU,
X11/Wayland client libraries, DBus, udev/evdev, font discovery and rendering,
and desktop-session command integrations. A musl target would therefore not by
itself produce a universally portable or fully static desktop application.

The release contract requires more than successful compilation. Every published
architecture or libc target must pass native archive extraction, installation,
all packaged command smokes, supported runtime probes, and removal from the
assembled artifact. A cross-compiled file that still depends on unavailable or
untested graphics, input, DBus, font, or session facilities is not a supported
release.

## Decision

DeskHalloumi 0.4 will **not publish a musl release artifact**.

The supported binary artifacts remain GNU/Linux builds whose desktop-system
library boundary is documented. The release train adds native AArch64 GNU/Linux
validation instead, because it can be tested end to end on a native hosted
runner.

This is a publication decision, not a ban on experiments. Contributors may use
musl targets locally to discover portability defects, but those builds are not
release assets and must not be described as static, universal, or supported.

## Reasons

1. The value of a musl artifact would be portability, but DeskHalloumi still
   requires host graphics, window-system, DBus, udev, input, font, and desktop
   services.
2. A successful Rust link would not prove that Iced/WGPU, tray/DBus, input,
   hotkey, popup, or provider behavior works on a musl distribution.
3. Maintaining another target before native install/runtime evidence exists
   would weaken the fail-closed release policy.
4. Native AArch64 GNU/Linux provides useful architecture coverage without
   pretending that libc substitution removes desktop integration dependencies.

## Consequences

- The release workflow publishes x86-64 GNU/Linux and conditionally publishes
  native AArch64 GNU/Linux after both archives pass installation and smoke tests.
- Documentation must state the GNU/Linux system-library boundary.
- The absence of a musl artifact is intentional and is not a missing 0.4 release
  task.
- No code path may select a musl artifact automatically or imply static linking.

## Revisit criteria

A later release may reopen this decision only when all of the following exist:

- a named musl distribution and native test environment;
- a reproducible native build rather than cross-compilation alone;
- documented dynamic and system-library requirements;
- archive installation, every packaged command smoke, supported runtime probes,
  and clean removal;
- an owner for target-specific failures and security updates;
- evidence that the artifact provides meaningful user value beyond a differently
  linked binary.

Until those criteria are met, the decision remains to publish no musl artifact.
