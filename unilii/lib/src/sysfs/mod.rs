//! Access to Linux sysfs for power and backlight devices.
//!
//! The `/sys/class/power_supply` and `/sys/class/backlight` hierarchies
//! expose information about batteries, mains power and display
//! backlights respectively.  This module defines a [`Device`]
//! abstraction for sysfs entries and provides higher‑level types to
//! monitor battery capacity, AC adapter state and backlight
//! brightness.  As with all modules in this crate the APIs are
//! asynchronous and return futures and streams.

use std::path::PathBuf;

use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::fs;

use crate::util::ReadDirStream;

/// Implementation of power information using events from udev and the
/// `power_supply` sysfs.  See the module documentation in
/// [`power`] for details.
#[cfg(feature = "power")]
pub mod power;

/// Implementation of backlight information using events from udev and
/// the `backlight` sysfs.  See [`backlight`] for details.
#[cfg(feature = "backlight")]
pub mod backlight;

/// Represents a device entry under `/sys/class`.  Each device has a
/// unique name (the directory name) and a path to its directory.
#[derive(Clone)]
pub struct Device {
    path: PathBuf,
    /// The directory name of the device.  This name can be compared
    /// against udev event `sysname`s to determine if a particular
    /// event pertains to this device.
    pub name: String,
}

impl Device {
    /// List all devices available in a given sysfs class.  The
    /// `class` argument should be the name of the directory under
    /// `/sys/class` (e.g. `"power_supply"` or `"backlight"`).  On
    /// error a [`Context`] message is attached to help the caller
    /// diagnose missing sysfs entries.
    async fn read_devices(class: &str) -> Result<Vec<Self>> {
        let devices = fs::read_dir(PathBuf::from("/sys/class").join(class))
            .await
            .context(format!(
                "`{class}` sysfs is required for `{class}` information"
            ))?;
        Ok(ReadDirStream::new(devices)
            .filter_map(async |result| result.ok())
            .filter_map(async |entry| {
                let path = entry.path();
                let name = path.file_name()?;
                Some(Self {
                    name: name.to_string_lossy().to_string(),
                    path,
                })
            })
            .collect::<Vec<_>>()
            .await)
    }

    /// Read a sysfs device attribute as a string.  Returns an error
    /// with context on failure.
    pub async fn read_device_attribute_string(&self, attribute: &str) -> Result<String> {
        fs::read_to_string(self.path.join(attribute))
            .await
            .with_context(|| format!("failed to read `{attribute}` for device `{}`", self.name))
    }

    /// Read a sysfs device attribute as an integer.  Returns an error
    /// with context on failure or parse error.
    pub async fn read_device_attribute_int(&self, attribute: &str) -> Result<i64> {
        self.read_device_attribute_string(attribute)
            .await
            .and_then(|s| {
                s.trim().parse::<i64>().with_context(|| {
                    format!("could not parse `{attribute}` for device `{}`", self.name)
                })
            })
    }
}
