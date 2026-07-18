//! Keyboard input monitoring via the Linux evdev subsystem.
//!
//! The evdev API exposes input events from devices such as keyboards
//! and mice. This module enumerates keyboard devices and provides
//! asynchronous streams of key events. Because the evdev API is
//! character device based the streams live in the background and
//! yield values whenever a key is pressed or released.
//!
//! Note that reading from input devices typically requires elevated
//! privileges. Running your bar as a regular user may require adding
//! the user to the `input` group or setting up udev rules.

use anyhow::{Result, anyhow};
use evdev::{Device, EventType, KeyCode, enumerate};
use futures::{StreamExt, stream};
use log::{info, warn};

use crate::StaticStream;

/// Represents a key press or release. The `code` field contains the
/// hardware key code (see [`evdev::KeyCode`]) and the `value` field
/// indicates the state: 0 for release, 1 for press and 2 for auto-repeat.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    /// The key code of the event.
    pub code: KeyCode,
    /// The raw value reported by the kernel: 0 release, 1 press, 2 auto-repeat.
    pub value: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct KeyboardScanStats {
    pub total_devices: usize,
    pub keyboard_candidates: usize,
}

fn is_keyboard_device(device: &Device) -> bool {
    let Some(keys) = device.supported_keys() else {
        return false;
    };
    keys.contains(KeyCode::KEY_A)
        && keys.contains(KeyCode::KEY_Z)
        && keys.contains(KeyCode::KEY_ENTER)
        && keys.contains(KeyCode::KEY_SPACE)
}

pub fn scan_keyboard_device_stats() -> KeyboardScanStats {
    let mut total_devices = 0usize;
    let mut keyboard_candidates = 0usize;

    for (_path, dev) in enumerate() {
        total_devices += 1;
        if dev.supported_events().contains(EventType::KEY) && is_keyboard_device(&dev) {
            keyboard_candidates += 1;
        }
    }

    KeyboardScanStats {
        total_devices,
        keyboard_candidates,
    }
}

/// Enumerate all input devices under `/dev/input` that report key events.
/// Devices which cannot be opened or which do not support the `KEY`
/// event type are silently ignored. The returned list may be empty
/// if no keyboards are present or accessible. Opening devices does
/// not grab them, so other applications will continue to receive events.
pub fn read_keyboard_devices() -> Vec<Device> {
    match read_keyboard_devices_with_grab(false) {
        Ok(devices) => devices,
        Err(error) => {
            warn!("keyboard device scan failed: {}", error);
            Vec::new()
        }
    }
}

/// Enumerate keyboard devices and optionally request an exclusive evdev grab.
///
/// In observe mode (`grab=false`), other applications and the window manager continue
/// to receive the same key events. In grab mode (`grab=true`), unilii asks the kernel
/// to suppress those events for other clients. Grab mode requires permission to open
/// and grab every selected keyboard device; failures are returned with actionable
/// diagnostics instead of silently falling back to shadow behavior.
pub fn read_keyboard_devices_with_grab(grab: bool) -> Result<Vec<Device>> {
    let mut keyboards = Vec::new();
    let mut total_devices = 0usize;
    for (path, mut dev) in enumerate() {
        total_devices += 1;
        let dev_name = dev.name().unwrap_or("unknown").to_string();
        if dev.supported_events().contains(EventType::KEY) && is_keyboard_device(&dev) {
            info!("keyboard input device: path={:?}, name={}", path, dev_name);
            if grab {
                dev.grab().map_err(|error| {
                    anyhow!(
                        "failed to grab evdev keyboard device path={:?} name='{}': {}. \
                         Active hotkey mode requires permission to read/grab /dev/input/event* \
                         devices; add the user to the input group, add a udev rule, or run with \
                         suitable privileges. Use --shadow/observe mode to test without grabbing.",
                        path,
                        dev_name,
                        error
                    )
                })?;
                info!(
                    "grabbed evdev keyboard device for active hotkey mode: path={:?}, name={}",
                    path, dev_name
                );
            }
            keyboards.push(dev);
        } else {
            info!(
                "input device skipped (not a full keyboard): path={:?}, name={}",
                path, dev_name
            );
        }
    }
    info!(
        "keyboard device scan completed: total_devices={}, keyboard_candidates={}, grab={}",
        total_devices,
        keyboards.len(),
        grab
    );

    if grab && keyboards.is_empty() {
        return Err(anyhow!(
            "no accessible keyboard devices were found for active grab mode. \
             Check /dev/input permissions or run unilii-hotkeyd --shadow first."
        ));
    }

    Ok(keyboards)
}

/// Convert an evdev device into a stream of [`KeyEvent`]s.
fn device_to_keyevent_stream(dev: Device) -> Option<StaticStream<KeyEvent>> {
    let dev_name = dev.name().unwrap_or("unknown").to_string();
    match dev.into_event_stream() {
        Ok(ev_stream) => {
            info!("opened evdev event stream for device: {}", dev_name);
            let stream = stream::unfold(ev_stream, |mut stream| async {
                loop {
                    match stream.next_event().await {
                        Ok(event) => {
                            if event.event_type() == EventType::KEY {
                                let code = KeyCode::new(event.code());
                                let key_event = KeyEvent {
                                    code,
                                    value: event.value(),
                                };
                                return Some((key_event, stream));
                            }
                        }
                        Err(_) => {
                            warn!("evdev event stream ended with error");
                            return None;
                        }
                    }
                }
            })
            .boxed();
            Some(stream)
        }
        Err(e) => {
            warn!(
                "failed to open evdev event stream for device {}: {}",
                dev_name, e
            );
            None
        }
    }
}

/// Merge all currently available keyboard devices into a single stream.
fn listen_keyboard_events_from_current_devices_with_grab(
    grab: bool,
) -> Result<StaticStream<KeyEvent>> {
    let devices = read_keyboard_devices_with_grab(grab)?;
    let mut streams = Vec::new();

    for dev in devices {
        if let Some(s) = device_to_keyevent_stream(dev) {
            streams.push(s);
        }
    }

    if streams.is_empty() {
        warn!("no keyboard event streams available after scan/open");
    } else {
        info!(
            "keyboard event streams initialized: count={}, grab={}",
            streams.len(),
            grab
        );
    }

    if streams.is_empty() {
        return Err(anyhow!(
            "no usable keyboard event streams are available. Check /dev/input permissions, \
             keyboard device detection, and whether another process exclusively grabbed the device"
        ));
    }

    Ok(stream::select_all(streams).boxed())
}

/// Merge all currently available keyboard devices into a non-grabbing stream.
fn listen_keyboard_events_from_current_devices() -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices_with_grab(false)
}

/// Listen for key events from all currently available keyboard devices.
///
/// This method does a one-time device scan and then listens on those devices.
/// Newly hot-plugged keyboards are not picked up. For experimental hot-plug
/// support via a udev monitor, use [`listen_keyboard_events_experimental`].
pub fn listen_keyboard_events() -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices()
}

/// Listen for key events from all currently available keyboard devices, optionally
/// requesting an exclusive evdev grab first.
pub fn listen_keyboard_events_with_grab(grab: bool) -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices_with_grab(grab)
}

/// Listen for keyboard events using experimental tokio-udev monitor support.
///
/// This currently creates an async monitor socket for input subsystem events
/// and then falls back to the current-device merge stream. It validates that
/// the monitor can be created and is ready for future dynamic hot-plug use,
/// while preserving stable event behavior.
#[cfg(feature = "input")]
pub fn listen_keyboard_events_experimental() -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_experimental_with_grab(false)
}

/// Listen for keyboard events using experimental tokio-udev monitor support,
/// optionally requesting an exclusive evdev grab before streaming current devices.
#[cfg(feature = "input")]
pub fn listen_keyboard_events_experimental_with_grab(grab: bool) -> Result<StaticStream<KeyEvent>> {
    use udev::MonitorBuilder;

    let socket = MonitorBuilder::new()?.match_subsystem("input")?.listen()?;
    let _monitor = tokio_udev::AsyncMonitorSocket::new(socket)?;
    info!("experimental tokio-udev monitor socket initialized for input subsystem");

    listen_keyboard_events_from_current_devices_with_grab(grab)
}

#[cfg(not(feature = "input"))]
pub fn listen_keyboard_events_experimental() -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices()
}

#[cfg(not(feature = "input"))]
pub fn listen_keyboard_events_experimental_with_grab(grab: bool) -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices_with_grab(grab)
}
