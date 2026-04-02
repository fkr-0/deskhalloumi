# unilii – an X11 port of the liischte panel

This repository contains **unilii**, a minimalist rewrite of the
original [liischte](https://github.com/VirtCode/liischte) Wayland bar for
the X11 windowing system.  While the upstream project relies on
`wlr-layer-shell` and Iced’s Wayland support, those technologies are
unavailable under X11.  After studying the code base and the EWMH
specification it became clear that a hybrid implementation supporting
both Wayland and X11 would be brittle and complex – window management
concepts such as layer surfaces simply have no analogue under X11.  As
such, **unilii** opts for a clean break: Wayland dependencies are
removed entirely and the bar is reimplemented using idiomatic Rust and
the [`x11rb`](https://docs.rs/x11rb) crate.  The result is a fully
self‑contained binary that displays useful system information on a
docked panel.

## Features

* **Dock integration** – the bar registers itself as a dock window
  using the EWMH `_NET_WM_WINDOW_TYPE_DOCK` hint and reserves space
  along the side of the screen via the `_NET_WM_STRUT` and
  `_NET_WM_STRUT_PARTIAL` properties【591885051952501†L158-L186】.  This ensures that other applications
  respect the bar’s reserved area when maximised.
* **Current time** – the bar shows the current time updated every
  second.  Formatting is provided by the `chrono` crate.
* **Battery status** – on systems with a battery the bar polls the
  first battery device found under `/sys/class/power_supply` and
  displays its charge level as a percentage.  When no battery is
  present the battery indicator is omitted.
* **Asynchronous design** – background tasks driven by [`tokio`]
  collect status updates without blocking the X11 event loop.  A
  bridge channel forwards these updates into the bar’s drawing code.

## Building

The project is a standard Cargo workspace comprising a single binary
crate.  To build the bar install a recent Rust toolchain and run

```sh
cargo build --release
```

The resulting binary can be found at `target/release/unilii`.  When
run under an X11 session the bar will appear on the left edge of the
primary screen.  You can tweak the bar’s width by adjusting the
`BAR_WIDTH` constant in `src/main.rs`.

## Tests

Several integration tests live in the `unilii/tests` directory.  They
verify that helper functions such as the strut calculation produce
correct results and that time formatting follows the expected
`HH:MM:SS` pattern.  Run them with

```sh
cargo test
```

## Limitations and future work

This port implements only a subset of liischte’s functionality.  It
does not (yet) display audio, network, timer or process information.
Extending the bar to support these features would involve polling
additional data sources such as PipeWire, NetworkManager and DBus.
The architecture of **unilii** makes such extensions straightforward:
simply spawn additional asynchronous tasks that forward updates into
the X11 event loop.

## License

Licensed under the same terms as the original liischte project, the
GNU General Public License version 3 or later (GPL‑3.0+).  See
`LICENSE.md` in the upstream repository for more details.
