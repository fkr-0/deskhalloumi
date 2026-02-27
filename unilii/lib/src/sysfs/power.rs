//! Battery and mains power monitoring.
//!
//! The `power_supply` sysfs exposes information about batteries and AC
//! adapters.  Each device has a `type` attribute indicating whether
//! it represents a battery (`"Battery"`), AC adapter (`"Mains"`) or
//! something else.  This module provides a set of types that wrap
//! power devices and offer both one‑shot reads and continuous
//! asynchronous streams of values.  Udev events are used to wake up
//! streams when attributes change.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use log::trace;
use tokio::time::Instant;
use udev::MonitorBuilder;

use crate::{StaticStream, StreamContext};
use crate::util::udev::AsyncMonitorSocket;

use super::Device;

/// Represents a device found under `/sys/class/power_supply` along
/// with its type.
#[derive(Clone)]
pub struct PowerDevice {
    pub device: Device,
    /// The kind of power device (battery, mains or unknown).
    pub kind: PowerDeviceKind,
}

/// The type of a power device.  Parsed from the `type` attribute in
/// `/sys/class/power_supply/<dev>/type`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PowerDeviceKind {
    /// An AC adapter.
    Mains,
    /// A battery.
    Battery,
    /// Some other type not explicitly handled.
    Unknown,
}

impl PowerDeviceKind {
    /// Parse a string from the sysfs into a [`PowerDeviceKind`].
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "Mains" => Self::Mains,
            "Battery" => Self::Battery,
            _ => Self::Unknown,
        }
    }
}

impl PowerDevice {
    /// Read all power devices currently available from the sysfs.  On
    /// error returns [`Err`] with context.  Each device is probed
    /// asynchronously for its type and the resulting vector contains
    /// both AC and battery devices.
    pub async fn read_all() -> Result<Vec<Self>> {
        let devices = Device::read_devices("power_supply").await?;
        Ok(futures::future::join_all(devices.into_iter().map(|d| async move {
            let kind = if let Ok(kind_str) = d.read_device_attribute_string("type").await {
                PowerDeviceKind::parse(&kind_str)
            } else {
                PowerDeviceKind::Unknown
            };
            Self { device: d, kind }
        }))
        .await)
    }
}

/// Wraps a mains power device.  Provides methods to read whether the
/// adapter is online and to listen for changes via udev.
#[derive(Clone)]
pub struct MainsPowerDevice(pub PowerDevice);

impl MainsPowerDevice {
    /// Read the `online` attribute of the AC adapter.  Returns `true`
    /// if the adapter is providing power.
    pub async fn read_online(&self) -> Result<bool> {
        self.0
            .device
            .read_device_attribute_int("online")
            .await
            .map(|v| v == 1)
    }

    /// Listen for udev events on the AC adapter and emit a stream of
    /// booleans representing the current online state.  Whenever an
    /// event for this device is received the sysfs is queried again
    /// and the new value is sent downstream.  Errors reading values
    /// are logged and result in the item being skipped.
    pub fn listen_online(self) -> Result<StaticStream<bool>> {
        let socket = MonitorBuilder::new()?
            .match_subsystem_devtype("power_supply", "power_supply")?
            .listen()?;
        let device_name = self.0.device.name.clone();
        let device = Arc::new(self);

        let stream = AsyncMonitorSocket::new(socket)?
            .filter_map(move |r| {
                let device_name = device_name.clone();
                async move {
                    // Filter events to those concerning our device.
                    if r
                        .context("invalid udev event")
                        .stream_log("ac online stream")?
                        .sysname()
                        .to_string_lossy()
                        == device_name
                    {
                        Some(())
                    } else {
                        None
                    }
                }
            })
            .then(move |_| {
                let device = device.clone();
                async move { device.read_online().await }
            })
            .filter_map(|r| async move { r.stream_log("ac online stream") })
            .boxed();
        Ok(stream)
    }
}

/// Wraps a battery device.  Provides methods to read its total
/// capacity and current charge, and to listen for changes via a
/// polling stream.
#[derive(Clone)]
pub struct BatteryPowerDevice(pub PowerDevice);

impl BatteryPowerDevice {
    /// Read the `energy_full` attribute and convert it to watt hours.
    pub async fn read_capacity(&self) -> Result<f64> {
        self.0
            .device
            .read_device_attribute_int("energy_full")
            .await
            .map(|energy| energy as f64 / 1e6)
    }

    /// Read the current charge level (0–1) from the `capacity` attribute.
    pub async fn read_charge(&self) -> Result<f64> {
        self.0
            .device
            .read_device_attribute_int("capacity")
            .await
            .map(|capacity| capacity as f64 / 100.0)
    }

    /// Create a stream which polls the battery charge at the given
    /// interval.  Whenever the charge level changes from the previous
    /// value the new value is emitted.  Errors reading the charge
    /// level cause the poll to be skipped but do not terminate the
    /// stream.
    pub fn listen_charge(self, polling: Duration) -> StaticStream<f64> {
        let mut interval = tokio::time::interval_at(Instant::now(), polling);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let device = Arc::new(self);

        futures::stream::unfold((interval, -1f64), move |(mut interval, last)| {
            let device = device.clone();
            async move {
                let mut next = last;
                while (next - last).abs() < f64::EPSILON {
                    interval.tick().await;
                    trace!("polling battery charge for device `{}`", device.0.device.name);
                    if let Some(charge) = device.read_charge().await.stream_log("battery charge stream")
                    {
                        next = charge;
                    }
                }
                Some((next, (interval, next)))
            }
        })
        .boxed()
    }
}
