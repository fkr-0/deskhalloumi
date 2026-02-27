//! Keyboard input monitoring via the Linux evdev subsystem.
//!
//! The evdev API exposes input events from devices such as keyboards
//! and mice.  This module enumerates keyboard devices and provides
//! asynchronous streams of key events.  Because the evdev API is
//! character device based the streams live in the background and
//! yield values whenever a key is pressed or released.
//!
//! Note that reading from input devices typically requires elevated
//! privileges.  Running your bar as a regular user may require
//! adding the user to the `input` group or setting up udev rules.

use anyhow::Result;
use evdev::{enumerate, Device, EventType, KeyCode};
use futures::{stream, StreamExt};

use crate::StaticStream;

/// Represents a key press or release.  The `code` field contains the
/// hardware key code (see [`evdev::KeyCode`]) and the `value` field
/// indicates the state: 0 for release, 1 for press and 2 for auto‑repeat.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    /// The key code of the event.
    pub code: KeyCode,
    /// The raw value reported by the kernel: 0 release, 1 press, 2 auto‑repeat.
    pub value: i32,
}

/// Enumerate all input devices under `/dev/input` that report key events.
/// Devices which cannot be opened or which do not support the `KEY`
/// event type are silently ignored.  The returned list may be empty
/// if no keyboards are present or accessible.  Opening devices does
/// not grab them, so other applications will continue to receive
/// events.
pub fn read_keyboard_devices() -> Vec<Device> {
    let mut keyboards = Vec::new();
    for (_path, dev) in enumerate() {
        // Filter out devices that don't support key events.  The
        // supported events API returns an AttributeSetRef of
        // EventType; we check whether it contains the KEY bit.
        if dev.supported_events().contains(EventType::KEY) {
            keyboards.push(dev);
        }
    }
    keyboards
}

/// Helper function to convert an evdev device into a stream of KeyEvents.
fn device_to_keyevent_stream(dev: Device) -> Option<StaticStream<KeyEvent>> {
    match dev.into_event_stream() {
        Ok(ev_stream) => {
            // Use unfold to convert the evdev stream into a futures stream
            let stream = stream::unfold(ev_stream, |mut stream| async {
                loop {
                    match stream.next_event().await {
                        Ok(event) => {
                            if event.event_type() == EventType::KEY {
                                // Convert the event code to KeyCode using evdev's KeyCode::new
                                let code = KeyCode::new(event.code());
                                let key_event = KeyEvent { code, value: event.value() };
                                return Some((key_event, stream));
                            }
                            // Not a key event, continue polling
                        }
                        Err(_) => {
                            // Error reading event, end stream
                            return None;
                        }
                    }
                }
            })
            .boxed();
            Some(stream)
        }
        Err(_) => None,
    }
}

/// Listen for key events from all available keyboard devices.  The
/// returned stream yields [`KeyEvent`]s for each key press, release
/// or repeat.  If opening a device's event stream fails the device
/// is skipped.  Errors reading individual events terminate that
/// device's stream but do not stop the others.
pub fn listen_keyboard_events() -> Result<StaticStream<KeyEvent>> {
    let devices = read_keyboard_devices();
    let mut streams = Vec::new();

    for dev in devices {
        if let Some(s) = device_to_keyevent_stream(dev) {
            streams.push(s);
        }
    }

    // Merge all device streams into one combined stream.  If no
    // streams were created return an empty stream.
    let combined: StaticStream<KeyEvent> = if streams.is_empty() {
        stream::empty().boxed()
    } else {
        stream::select_all(streams).boxed()
    };
    Ok(combined)
}
