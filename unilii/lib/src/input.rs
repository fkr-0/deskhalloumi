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
use log::{debug, info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

struct OpenedKeyboard {
    path: PathBuf,
    device: Device,
}

#[derive(Debug)]
enum DeviceStreamItem {
    Key {
        path: PathBuf,
        generation: u64,
        event: KeyEvent,
    },
    Closed {
        path: PathBuf,
        generation: u64,
        error: String,
    },
}

#[derive(Debug, Default)]
struct KeyboardDeviceRegistry {
    active: HashMap<PathBuf, u64>,
    next_generation: u64,
}

impl KeyboardDeviceRegistry {
    fn register(&mut self, path: PathBuf) -> Option<u64> {
        if self.active.contains_key(&path) {
            return None;
        }

        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let generation = self.next_generation;
        self.active.insert(path, generation);
        Some(generation)
    }

    fn is_active(&self, path: &Path) -> bool {
        self.active.contains_key(path)
    }

    fn remove(&mut self, path: &Path) -> bool {
        self.active.remove(path).is_some()
    }

    fn remove_generation(&mut self, path: &Path, generation: u64) -> bool {
        if self.accepts(path, generation) {
            self.active.remove(path);
            true
        } else {
            false
        }
    }

    fn accepts(&self, path: &Path, generation: u64) -> bool {
        self.active.get(path).copied() == Some(generation)
    }
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
/// to receive the same key events. In grab mode (`grab=true`), DeskHalloumi asks the
/// kernel to suppress those events for other clients. Grab mode requires permission
/// to open and grab every selected keyboard device; failures are returned with
/// actionable diagnostics instead of silently falling back to shadow behavior.
pub fn read_keyboard_devices_with_grab(grab: bool) -> Result<Vec<Device>> {
    Ok(read_keyboard_devices_with_paths(grab)?
        .into_iter()
        .map(|opened| opened.device)
        .collect())
}

fn read_keyboard_devices_with_paths(grab: bool) -> Result<Vec<OpenedKeyboard>> {
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
            keyboards.push(OpenedKeyboard { path, device: dev });
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
             Check /dev/input permissions or run deskhalloumi-hotkeyd --shadow first."
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
                                let key_event = KeyEvent {
                                    code: KeyCode::new(event.code()),
                                    value: event.value(),
                                };
                                return Some((key_event, stream));
                            }
                        }
                        Err(error) => {
                            warn!("evdev event stream ended with error: {}", error);
                            return None;
                        }
                    }
                }
            })
            .boxed();
            Some(stream)
        }
        Err(error) => {
            warn!(
                "failed to open evdev event stream for device {}: {}",
                dev_name, error
            );
            None
        }
    }
}

fn device_to_tagged_keyevent_stream(
    dev: Device,
    path: PathBuf,
    generation: u64,
) -> Result<StaticStream<DeviceStreamItem>> {
    let dev_name = dev.name().unwrap_or("unknown").to_string();
    let event_stream = dev.into_event_stream().map_err(|error| {
        anyhow!(
            "failed to open evdev event stream for path={} name='{}': {}",
            path.display(),
            dev_name,
            error
        )
    })?;
    info!(
        "opened hot-plug evdev event stream: path={}, name={}, generation={}",
        path.display(),
        dev_name,
        generation
    );

    Ok(
        stream::unfold(Some((event_stream, path, generation)), |state| async move {
            let (mut event_stream, path, generation) = state?;
            loop {
                match event_stream.next_event().await {
                    Ok(event) if event.event_type() == EventType::KEY => {
                        return Some((
                            DeviceStreamItem::Key {
                                path: path.clone(),
                                generation,
                                event: KeyEvent {
                                    code: KeyCode::new(event.code()),
                                    value: event.value(),
                                },
                            },
                            Some((event_stream, path, generation)),
                        ));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        return Some((
                            DeviceStreamItem::Closed {
                                path,
                                generation,
                                error: error.to_string(),
                            },
                            None,
                        ));
                    }
                }
            }
        })
        .boxed(),
    )
}

fn open_keyboard_device(path: &Path, grab: bool) -> Result<Option<Device>> {
    let mut device = Device::open(path).map_err(|error| {
        anyhow!(
            "failed to open hot-plug input device path={}: {}",
            path.display(),
            error
        )
    })?;
    let name = device.name().unwrap_or("unknown").to_string();
    if !device.supported_events().contains(EventType::KEY) || !is_keyboard_device(&device) {
        debug!(
            "hot-plug input device ignored (not a full keyboard): path={}, name={}",
            path.display(),
            name
        );
        return Ok(None);
    }

    if grab {
        device.grab().map_err(|error| {
            anyhow!(
                "failed to grab hot-plug keyboard path={} name='{}': {}",
                path.display(),
                name,
                error
            )
        })?;
    }
    Ok(Some(device))
}

/// Merge all currently available keyboard devices into a single stream.
fn listen_keyboard_events_from_current_devices_with_grab(
    grab: bool,
) -> Result<StaticStream<KeyEvent>> {
    let devices = read_keyboard_devices_with_grab(grab)?;
    let mut streams = Vec::new();

    for dev in devices {
        if let Some(stream) = device_to_keyevent_stream(dev) {
            streams.push(stream);
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
/// Newly hot-plugged keyboards are not picked up. For hot-plug support via a
/// udev monitor, use [`listen_keyboard_events_experimental`].
pub fn listen_keyboard_events() -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices()
}

/// Listen for key events from all currently available keyboard devices, optionally
/// requesting an exclusive evdev grab first.
pub fn listen_keyboard_events_with_grab(grab: bool) -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices_with_grab(grab)
}

/// Listen for keyboard events using tokio-udev hot-plug support.
///
/// The returned stream starts with all currently accessible keyboards and then
/// adds newly connected keyboards without restarting the daemon. Removed or
/// failed streams are retired independently, so one disappearing device does
/// not terminate input from the remaining keyboards. Device generations
/// suppress stale events when an `/dev/input/event*` path is quickly reused.
#[cfg(feature = "input")]
pub fn listen_keyboard_events_experimental() -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_experimental_with_grab(false)
}

/// Listen for keyboard events using tokio-udev hot-plug support, optionally
/// requesting an exclusive evdev grab for each current and future keyboard.
#[cfg(feature = "input")]
pub fn listen_keyboard_events_experimental_with_grab(grab: bool) -> Result<StaticStream<KeyEvent>> {
    use futures::stream::SelectAll;
    use tokio_udev::{AsyncMonitorSocket, EventType as UdevEventType, MonitorBuilder};

    let socket = MonitorBuilder::new()?.match_subsystem("input")?.listen()?;
    let monitor = AsyncMonitorSocket::new(socket)?;
    info!("tokio-udev hot-plug monitor initialized for input subsystem");

    let opened = read_keyboard_devices_with_paths(grab)?;
    let mut registry = KeyboardDeviceRegistry::default();
    let mut streams = SelectAll::new();
    for opened in opened {
        let Some(generation) = registry.register(opened.path.clone()) else {
            continue;
        };
        match device_to_tagged_keyevent_stream(opened.device, opened.path.clone(), generation) {
            Ok(stream) => streams.push(stream),
            Err(error) => {
                registry.remove_generation(&opened.path, generation);
                warn!("hot-plug keyboard stream setup failed: {}", error);
            }
        }
    }

    if streams.is_empty() {
        return Err(anyhow!(
            "no usable keyboard event streams are available for hot-plug monitoring. Check \
             /dev/input permissions and keyboard device detection"
        ));
    }

    struct HotplugState {
        monitor: AsyncMonitorSocket,
        monitor_live: bool,
        streams: SelectAll<StaticStream<DeviceStreamItem>>,
        registry: KeyboardDeviceRegistry,
        grab: bool,
    }

    enum NextInput {
        Monitor(Option<std::io::Result<tokio_udev::Event>>),
        Device(Option<DeviceStreamItem>),
    }

    let state = HotplugState {
        monitor,
        monitor_live: true,
        streams,
        registry,
        grab,
    };

    Ok(stream::unfold(state, |mut state| async move {
        loop {
            let next = match (state.monitor_live, state.streams.is_empty()) {
                (true, false) => {
                    tokio::select! {
                        event = state.monitor.next() => NextInput::Monitor(event),
                        event = state.streams.next() => NextInput::Device(event),
                    }
                }
                (true, true) => NextInput::Monitor(state.monitor.next().await),
                (false, false) => NextInput::Device(state.streams.next().await),
                (false, true) => return None,
            };

            match next {
                NextInput::Monitor(Some(Ok(event))) => {
                    let Some(path) = event.devnode().map(Path::to_path_buf) else {
                        continue;
                    };
                    match event.event_type() {
                        UdevEventType::Add | UdevEventType::Change => {
                            if state.registry.is_active(&path) {
                                continue;
                            }
                            match open_keyboard_device(&path, state.grab) {
                                Ok(Some(device)) => {
                                    let Some(generation) = state.registry.register(path.clone())
                                    else {
                                        continue;
                                    };
                                    match device_to_tagged_keyevent_stream(
                                        device,
                                        path.clone(),
                                        generation,
                                    ) {
                                        Ok(stream) => {
                                            state.streams.push(stream);
                                            info!(
                                                "hot-plug keyboard activated: path={}, generation={}",
                                                path.display(),
                                                generation
                                            );
                                        }
                                        Err(error) => {
                                            state.registry.remove_generation(&path, generation);
                                            warn!(
                                                "hot-plug keyboard activation failed for path={}: {}",
                                                path.display(),
                                                error
                                            );
                                        }
                                    }
                                }
                                Ok(None) => {}
                                Err(error) => warn!(
                                    "hot-plug input device could not be activated path={}: {}",
                                    path.display(),
                                    error
                                ),
                            }
                        }
                        UdevEventType::Remove | UdevEventType::Unbind => {
                            if state.registry.remove(&path) {
                                info!("hot-plug keyboard removed: path={}", path.display());
                            }
                        }
                        UdevEventType::Bind | UdevEventType::Unknown => {}
                    }
                }
                NextInput::Monitor(Some(Err(error))) => {
                    warn!("tokio-udev hot-plug monitor failed: {}", error);
                    state.monitor_live = false;
                }
                NextInput::Monitor(None) => {
                    warn!("tokio-udev hot-plug monitor ended");
                    state.monitor_live = false;
                }
                NextInput::Device(Some(DeviceStreamItem::Key {
                    path,
                    generation,
                    event,
                })) => {
                    if state.registry.accepts(&path, generation) {
                        return Some((event, state));
                    }
                    debug!(
                        "discarded stale keyboard event: path={}, generation={}",
                        path.display(),
                        generation
                    );
                }
                NextInput::Device(Some(DeviceStreamItem::Closed {
                    path,
                    generation,
                    error,
                })) => {
                    if state.registry.remove_generation(&path, generation) {
                        warn!(
                            "keyboard event stream closed: path={}, generation={}, error={}",
                            path.display(),
                            generation,
                            error
                        );
                    }
                }
                NextInput::Device(None) => {}
            }
        }
    })
    .boxed())
}

#[cfg(not(feature = "input"))]
pub fn listen_keyboard_events_experimental() -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices()
}

#[cfg(not(feature = "input"))]
pub fn listen_keyboard_events_experimental_with_grab(grab: bool) -> Result<StaticStream<KeyEvent>> {
    listen_keyboard_events_from_current_devices_with_grab(grab)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_registry_deduplicates_and_reactivates_paths() {
        let path = PathBuf::from("/dev/input/event7");
        let mut registry = KeyboardDeviceRegistry::default();

        let first = registry.register(path.clone()).expect("first generation");
        assert!(registry.register(path.clone()).is_none());
        assert!(registry.accepts(&path, first));

        assert!(registry.remove(&path));
        let second = registry.register(path.clone()).expect("second generation");
        assert_ne!(first, second);
        assert!(registry.accepts(&path, second));
        assert!(!registry.accepts(&path, first));
    }

    #[test]
    fn stale_stream_close_does_not_remove_reused_device_path() {
        let path = PathBuf::from("/dev/input/event9");
        let mut registry = KeyboardDeviceRegistry::default();
        let old = registry.register(path.clone()).expect("old generation");
        registry.remove(&path);
        let current = registry.register(path.clone()).expect("current generation");

        assert!(!registry.remove_generation(&path, old));
        assert!(registry.accepts(&path, current));
        assert!(registry.remove_generation(&path, current));
        assert!(!registry.accepts(&path, current));
    }
}
