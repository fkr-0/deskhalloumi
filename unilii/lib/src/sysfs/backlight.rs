//! Backlight brightness monitoring.
//!
//! Backlight devices are exposed under `/sys/class/backlight`.  Each
//! device has a `brightness` and `max_brightness` attribute.  This
//! module wraps backlight devices and provides methods to read the
//! current brightness as a fraction of the maximum and to listen for
//! changes via udev events.

use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use udev::MonitorBuilder;

use crate::{StaticStream, StreamContext};
use crate::util::udev::AsyncMonitorSocket;

use super::Device;

/// Represents a backlight device with its maximum brightness.
#[derive(Clone)]
pub struct BacklightDevice {
    /// The underlying sysfs device entry.
    pub device: Device,
    /// Maximum brightness of the device (from `max_brightness`).
    max: u32,
}

impl BacklightDevice {
    /// Read all backlight devices currently available.  Devices whose
    /// `max_brightness` cannot be read are skipped.  On error
    /// reading the device list returns [`Err`].
    pub async fn read_all() -> Result<Vec<Self>> {
        let devices = Device::read_devices("backlight").await?;
        Ok(futures::future::join_all(devices.into_iter().map(|d| async move {
            if let Ok(max) = d.read_device_attribute_int("max_brightness").await {
                Some(Self { device: d, max: max as u32 })
            } else {
                None
            }
        }))
        .await
        .into_iter()
        .filter_map(|o| o)
        .collect())
    }

    /// Read the current brightness as a fraction in the range 0–1.
    pub async fn read_brightness(&self) -> Result<f64> {
        self.device
            .read_device_attribute_int("brightness")
            .await
            .map(|b| b as f64 / self.max as f64)
    }

    /// Listen for udev events on this backlight device and produce a
    /// stream of brightness values.  Whenever an event for this
    /// device is received the brightness is read and emitted.
    pub fn listen_brightness(self) -> Result<StaticStream<f64>> {
        let socket = MonitorBuilder::new()?.match_subsystem("backlight")?.listen()?;
        let device_name = self.device.name.clone();
        let device = Arc::new(self);
        const STREAM: &str = "backlight brightness stream";

        let stream = AsyncMonitorSocket::new(socket)?
            .filter_map(move |r| {
                let device_name = device_name.clone();
                async move {
                    // Filter to events for this device.
                    if r
                        .stream_context(STREAM, "received invalid udev event")?
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
                async move { device.read_brightness().await }
            })
            .filter_map(|r| async move { r.stream_log(STREAM) })
            .boxed();
        Ok(stream)
    }
}
