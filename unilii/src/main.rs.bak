//! A minimal status bar for the X11 windowing system.
//!
//! This binary provides a simple vertical bar similar in spirit to the
//! original `liischte` Wayland bar.  It eschews any Wayland–specific
//! dependencies and instead speaks the X11 protocol directly via the
//! [`x11rb`](https://docs.rs/x11rb) crate.  The bar displays the
//! current time and the charge level of the first available battery
//! device found under `/sys/class/power_supply`.  A small event loop
//! integrates X11 events with asynchronous tasks driven by [`tokio`].

use anyhow::Result;
use chrono::Local;
use log::{error, info};
use futures::StreamExt;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;
use x11rb::connection::Connection;
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::protocol::xproto::*;
use x11rb::protocol::xproto::ConnectionExt as XprotoConnectionExt;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt;

// Pull in our unified system monitoring library.  This module
// provides asynchronous streams of battery, backlight, process and
// keyboard events.  See the documentation in the `unilii-lib` crate
// for details.
use unilii_lib as lib;

// Re‑export frequently used types from the library to avoid deep
// module paths.
use lib::sysfs::power::{BatteryPowerDevice, PowerDevice};
use lib::sysfs::backlight::BacklightDevice;
use lib::process;
use lib::input;
mod util;

/// The width of the bar in pixels.  This value also doubles as the size of
/// the `_NET_WM_STRUT` reserved area on the left or right side of the
/// screen.  Feel free to adjust this to your liking.
const BAR_WIDTH: u16 = 48;

/// Identifiers for the different types of updates that can be sent
/// to the event loop.  Each variant carries the data associated with
/// that update.  When adding new functionality extend this enum
/// accordingly.
#[derive(Debug, Clone)]
enum Update {
    /// The system time has changed.  Carries a string in `HH:MM:SS` format.
    Time(String),
    /// The battery charge has changed.  Carries a value in the range
    /// `0.0..=1.0`, where `1.0` means fully charged.
    Battery(f64),
    /// The backlight brightness has changed.  Carries a value in the range
    /// `0.0..=1.0`, where `1.0` means maximum brightness.
    Brightness(f64),
    /// The number of running processes has changed.  Carries the current count.
    Processes(usize),
    /// A keyboard event occurred.  Carries a textual representation of the key.
    Key(String),
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    info!("starting unilii (X11 bar)");

    // Create an unbounded channel for sending updates from async tasks to
    // the blocking X11 event loop.  We use a tokio mpsc channel here so
    // that async senders can await on it without blocking the runtime.
    let (update_tx, mut update_rx) = tokio_mpsc::unbounded_channel::<Update>();

    // Spawn a task that periodically sends the current time.  We emit
    // updates every second so that seconds are accurate.  For a less
    // chatty bar one could increase this interval.
    let time_sender = update_tx.clone();
    tokio::spawn(async move {
        loop {
            let now = Local::now();
            let formatted = now.format("%H:%M:%S").to_string();
            if time_sender.send(Update::Time(formatted)).is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // Spawn a task that monitors the battery charge via the
    // `unilii-lib` power subsystem.  We query the list of power devices
    // once at startup and pick the first battery.  The resulting
    // stream emits a new charge value whenever it changes.
    let battery_sender = update_tx.clone();
    tokio::spawn(async move {
        if let Ok(devs) = PowerDevice::read_all().await {
            if let Some(dev) = devs.into_iter().find(|d| matches!(d.kind, lib::sysfs::power::PowerDeviceKind::Battery)) {
                let bat = BatteryPowerDevice(dev);
                let mut stream = bat.listen_charge(Duration::from_secs(30));
                while let Some(level) = stream.next().await {
                    if battery_sender.send(Update::Battery(level)).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Spawn a task that monitors backlight brightness.  We choose the
    // first available backlight device and listen for udev events.
    let brightness_sender = update_tx.clone();
    tokio::spawn(async move {
        if let Ok(devices) = BacklightDevice::read_all().await {
            if let Some(dev) = devices.into_iter().next() {
                match dev.clone().listen_brightness() {
                    Ok(mut stream) => {
                        while let Some(level) = stream.next().await {
                            if brightness_sender.send(Update::Brightness(level)).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        error!("backlight watcher could not start: {e:#}");
                    }
                }
            }
        }
    });

    // Spawn a task that monitors the process table.  Every minute the
    // number of running processes is counted and sent to the UI.
    let process_sender = update_tx.clone();
    tokio::spawn(async move {
        let mut stream = process::listen_running_processes(Duration::from_secs(60));
        while let Some(list) = stream.next().await {
            let count = list.len();
            if process_sender.send(Update::Processes(count)).is_err() {
                break;
            }
        }
    });

    // Spawn a task that monitors keyboard input.  For each key press or
    // release we send the key code as a string.  If no input devices
    // are accessible the stream will be empty.
    let key_sender = update_tx.clone();
    tokio::spawn(async move {
        match input::listen_keyboard_events() {
            Ok(mut stream) => {
                while let Some(evt) = stream.next().await {
                    let key_string = format!("{:?}", evt.code);
                    if key_sender.send(Update::Key(key_string)).is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                error!("keyboard watcher could not start: {e:#}");
            }
        }
    });

    // Create a synchronous channel for delivering updates into the X11
    // thread.  We bridge the tokio async world to the blocking X11
    // environment by forwarding messages through an std channel.
    let (event_tx, event_rx): (mpsc::Sender<Update>, Receiver<Update>) = mpsc::channel();
    // Spawn a forwarder on the async runtime that consumes from the
    // asynchronous receiver and forwards to the synchronous channel.
    tokio::spawn(async move {
        while let Some(update) = update_rx.recv().await {
            if event_tx.send(update).is_err() {
                break;
            }
        }
    });

    // Launch the X11 event loop on a separate OS thread.  Running X11
    // operations on a dedicated thread ensures that the connection is
    // confined to a single thread as required by the X11 protocol.  The
    // closure captures the event receiver and uses it to drive UI
    // updates.
    thread::spawn(move || {
        if let Err(e) = x11_event_loop(event_rx) {
            error!("unhandled X11 error: {e:#}");
        }
    })
    .join()
    .expect("X11 thread panicked");

    Ok(())
}

/// The central event loop of the bar.  This function connects to the
/// X11 server, creates and maps a window configured as a dock, and
/// listens for both X11 events and status updates coming from the
/// asynchronous side of the application.  When something changes it
/// redraws the window contents accordingly.
fn x11_event_loop(updates: Receiver<Update>) -> Result<()> {
    // Connect to the default X11 server.  An error here indicates that
    // no server is available (e.g. this program is run under Wayland).
    let (conn, screen_num) = x11rb::connect(None)?;
    let conn = RustConnection::from(conn);
    let screen = &conn.setup().roots[screen_num];

    // Determine our bar geometry.  We anchor the bar to the left side
    // of the screen.  To place it on the right side simply set `x`
    // accordingly (e.g. `screen.width_in_pixels - BAR_WIDTH as u16`).
    let x = 0;
    let y = 0;
    let width = BAR_WIDTH;
    let height = screen.height_in_pixels;

    let win_id = conn.generate_id()?;
    // We use a simple graphics context for drawing text.  In a real
    // bar you might want to load a specific font via the Xft or
    // FreeType APIs.  Here we stick to the server's default font.
    let gc_id = conn.generate_id()?;
    conn.create_gc(
        gc_id,
        screen.root,
        &CreateGCAux::new()
            .foreground(screen.white_pixel)
            .background(screen.black_pixel),
    )?;

    // Create an InputOutput window with no border and a black
    // background.  We subscribe to expose events so we know when to
    // redraw the contents.
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win_id,
        screen.root,
        x as i16,
        y as i16,
        width,
        height,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY)
            .background_pixel(screen.black_pixel),
    )?;

    // Set EWMH window type to `_NET_WM_WINDOW_TYPE_DOCK` so that
    // compliant window managers treat our bar as a dock.  This hint
    // prevents maximised windows from covering the bar.  We also set
    // `_NET_WM_STRUT` and `_NET_WM_STRUT_PARTIAL` properties to
    // reserve screen real estate equal to our width.  The semantics of
    // these properties are defined in the EWMH specification: the
    // reserved area extends from the top of the screen to the bottom
    // (`left_start_y = 0`, `left_end_y = screen.height_in_pixels`) and
    // has a width of `BAR_WIDTH` pixels on the left side【591885051952501†L158-L186】.
    let wm_type = conn
        .intern_atom(false, b"_NET_WM_WINDOW_TYPE")?
        .reply()?
        .atom;
    let wm_type_dock = conn
        .intern_atom(false, b"_NET_WM_WINDOW_TYPE_DOCK")?
        .reply()?
        .atom;
    let net_wm_strut = conn
        .intern_atom(false, b"_NET_WM_STRUT")?
        .reply()?
        .atom;
    let net_wm_strut_partial = conn
        .intern_atom(false, b"_NET_WM_STRUT_PARTIAL")?
        .reply()?
        .atom;

    // Apply the window type.
    conn.change_property32(PropMode::REPLACE, win_id, wm_type, AtomEnum::ATOM, &[wm_type_dock])?;
    // Reserve space on the left of the screen equal to our width.
    let left = width as u32;
    let right = 0u32;
    let top = 0u32;
    let bottom = 0u32;
    let strut_partial = [
        left,
        right,
        top,
        bottom,
        0,
        height as u32,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    conn.change_property32(
        PropMode::REPLACE,
        win_id,
        net_wm_strut_partial,
        AtomEnum::CARDINAL,
        &strut_partial,
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        win_id,
        net_wm_strut,
        AtomEnum::CARDINAL,
        &[left, right, top, bottom],
    )?;

    // Map (show) the window and flush the connection.  Without flushing
    // the server would not process our requests immediately.
    conn.map_window(win_id)?;
    conn.flush()?;

    // Maintain the latest values for each piece of state that we display.
    let mut time_string = String::new();
    let mut battery_level: Option<f64> = None;
    let mut brightness_level: Option<f64> = None;
    let mut process_count: Option<usize> = None;
    let mut last_key: Option<String> = None;

    // Enter the event loop.  We poll for X11 events and apply any
    // pending updates from the async tasks.  If either an expose
    // event occurs or we have new data we redraw the contents.
    loop {
        // Drain update messages without blocking.  We use a
        // while-let loop here to pull in all available messages at
        // once.  Doing it this way ensures that we don't lose updates
        // when multiple notifications arrive before the next redraw.
        let mut dirty = false;
        while let Ok(update) = updates.try_recv() {
            match update {
                Update::Time(s) => {
                    time_string = s;
                    dirty = true;
                }
                Update::Battery(level) => {
                    battery_level = Some(level);
                    dirty = true;
                }
                Update::Brightness(level) => {
                    brightness_level = Some(level);
                    dirty = true;
                }
                Update::Processes(count) => {
                    process_count = Some(count);
                    dirty = true;
                }
                Update::Key(key) => {
                    last_key = Some(key);
                    dirty = true;
                }
            }
        }

        // Non‑blocking poll for an X11 event.  If there is an event
        // available we handle it; otherwise `poll_for_event` returns
        // `None` and we continue.  We only care about expose events in
        // this simple bar.
        match conn.poll_for_event()? {
            Some(event) => match event {
                Event::Expose(_) => {
                    dirty = true;
                }
                _ => {}
            },
            None => {}
        }

        if dirty {
            draw_bar(
                &conn,
                win_id,
                gc_id,
                width,
                height,
                &time_string,
                battery_level,
                brightness_level,
                process_count,
                last_key.as_deref(),
            )?;
        }

        // Sleep briefly to avoid spinning the CPU when there are no
        // events to process.  A small delay (e.g. 50ms) suffices.
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Redraw the contents of the bar.  This function clears the window and
/// renders the current time and battery level.  Text is drawn using
/// the core X11 `ImageText8` request which supports only Latin1
/// characters.  We pack the battery level as a percentage and pad
/// strings to align them roughly in the middle of the bar.
/// Draw the bar contents.  This function uses `tiny-skia` to render
/// battery and brightness indicators into an offscreen pixmap and then
/// uploads the pixmap to the X11 window via `PutImage`.  Textual
/// information (time, battery percentage, brightness percentage,
/// process count and last key) is drawn using the X11 `ImageText8`
/// request.  Note that text rendering is limited to ISO‑8859‑1.
fn draw_bar(
    conn: &RustConnection,
    window: u32,
    gc: u32,
    width: u16,
    height: u16,
    time_string: &str,
    battery_level: Option<f64>,
    brightness_level: Option<f64>,
    process_count: Option<usize>,
    last_key: Option<&str>,
) -> Result<()> {
    // Create a pixmap for rendering shapes.  tiny‑skia uses RGBA
    // premultiplied pixels.  If the pixmap cannot be allocated we
    // simply skip drawing shapes.
    if let Some(mut pixmap) = tiny_skia::Pixmap::new(width as u32, height as u32) {
        // Fill background.
        let bg = tiny_skia::Color::from_rgba8(0x00, 0x00, 0x00, 0xff);
        pixmap.fill(bg);
        // Draw battery bar if available.  The bar fills from bottom to
        // top with a green colour.  Height is proportional to the
        // battery level.
        if let Some(level) = battery_level {
            let h = (level.clamp(0.0, 1.0) * height as f64) as u32;
            if h > 0 {
                let rect = tiny_skia::Rect::from_xywh(
                    0.0,
                    (height as u32 - h) as f32,
                    width as f32,
                    h as f32,
                )
                .unwrap();
                let mut paint = tiny_skia::Paint::default();
                // Colour transitions from red at low charge to green
                // at high charge.
                let green = (level * 255.0) as u8;
                let red = ((1.0 - level) * 255.0) as u8;
                paint.set_color(tiny_skia::Color::from_rgba8(red, green, 0x20, 0xff));
                pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
            }
        }
        // Draw brightness bar if available.  This bar is drawn on the
        // right half of the battery bar to distinguish it.  Colour
        // transitions from dark blue to light blue.
        if let Some(level) = brightness_level {
            let h = (level.clamp(0.0, 1.0) * height as f64) as u32;
            if h > 0 {
                let rect = tiny_skia::Rect::from_xywh(
                    (width as f32) * 0.5,
                    (height as u32 - h) as f32,
                    (width as f32) * 0.5,
                    h as f32,
                )
                .unwrap();
                let mut paint = tiny_skia::Paint::default();
                let blue = (level * 255.0) as u8;
                paint.set_color(tiny_skia::Color::from_rgba8(0x20, 0x40, blue, 0xff));
                pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
            }
        }
        // Convert the pixmap into raw pixel data for X11.  tiny‑skia
        // stores pixels in RGBA order; X11 expects BGRX (blue, green,
        // red, unused).  We convert each pixel accordingly.  The
        // resulting buffer has 4 bytes per pixel.
        let raw = pixmap.take();
        let mut img_data = Vec::with_capacity(raw.len());
        for chunk in raw.chunks(4) {
            let r = chunk[0];
            let g = chunk[1];
            let b = chunk[2];
            let _a = chunk[3];
            // X11 expects blue, green, red, then unused alpha
            img_data.push(b);
            img_data.push(g);
            img_data.push(r);
            img_data.push(0xff);
        }
        // Upload the image to the window.  We use Z_PIXMAP format and
        // depth 32, which matches our 4‑byte pixel representation.
        conn.put_image(
            ImageFormat::Z_PIXMAP,
            window,
            gc,
            width,
            height,
            0,
            0,
            0,
            32,
            &img_data,
        )?;
    }
    // Now draw the text.  Build a list of lines to render.
    let mut lines = Vec::new();
    lines.push(time_string.to_string());
    if let Some(level) = battery_level {
        let pct = (level * 100.0).round() as i32;
        lines.push(format!("Bat: {pct:>3}%"));
    }
    if let Some(level) = brightness_level {
        let pct = (level * 100.0).round() as i32;
        lines.push(format!("Brt: {pct:>3}%"));
    }
    if let Some(count) = process_count {
        lines.push(format!("Proc: {count}"));
    }
    if let Some(key) = last_key {
        lines.push(format!("Key: {key}"));
    }
    // Compute y positions for each line.  We place the first line at
    // the bottom and stack upwards.  Each character is roughly 8
    // pixels wide and 14 pixels tall in the default font.  If the
    // lines exceed the window height they will be clipped.
    let line_height: i16 = 14;
    let base_y: i16 = line_height * lines.len() as i16;
    for (i, text) in lines.iter().rev().enumerate() {
        let y = base_y - (i as i16 + 1) * line_height;
        let text_bytes = text.as_bytes();
        let text_width = text_bytes.len() as i16 * 8;
        let x = (width as i16 - text_width) / 2;
        conn.image_text8(window, gc, x, y, text_bytes)?;
    }
    conn.flush()?;
    Ok(())
}