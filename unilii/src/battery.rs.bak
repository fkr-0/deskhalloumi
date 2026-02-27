use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::time::{self, Interval};

/// An asynchronous battery monitor that polls the first available
/// battery device under `/sys/class/power_supply` at a fixed
/// interval.  Each call to [`BatteryWatcher::next`] waits for the
/// polling interval, reads the battery's current charge and yields a
/// value between `0.0` and `1.0` when the charge has changed.  If no
/// battery device exists on the system then the monitor yields
/// `None` indefinitely.
pub struct BatteryWatcher {
    capacity_path: Option<PathBuf>,
    interval: Interval,
    last: Option<f64>,
}

impl BatteryWatcher {
    /// Construct a new battery watcher.  If a battery device cannot
    /// be located this function returns `Ok` with a watcher that
    /// produces no updates.  Errors reading the sysfs are surfaced
    /// through the returned [`Result`].
    pub async fn new() -> Result<Self> {
        let mut capacity_path = None;
        let entries = fs::read_dir("/sys/class/power_supply")
            .context("reading /sys/class/power_supply")?;
        for entry in entries {
            let entry = entry?;
            let dev_path = entry.path();
            // Read the device type (e.g. Battery, Mains).  Skip devices
            // that don't declare a type or aren't batteries.
            let type_path = dev_path.join("type");
            if let Ok(dev_type) = fs::read_to_string(&type_path) {
                if dev_type.trim() == "Battery" {
                    let cap_path = dev_path.join("capacity");
                    if cap_path.exists() {
                        capacity_path = Some(cap_path);
                        break;
                    }
                }
            }
        }
        Ok(Self {
            capacity_path,
            interval: time::interval(time::Duration::from_secs(30)),
            last: None,
        })
    }

    /// Poll the battery device until a new charge level is observed or
    /// until the watcher concludes that no battery exists.  Returns
    /// `None` if no battery is present.  Otherwise returns the new
    /// charge level as a floating point fraction in the range
    /// `0.0..=1.0` whenever it changes from the previous reading.
    pub async fn next(&mut self) -> Option<f64> {
        // If we never found a battery device we can exit early.
        let capacity_path = match &self.capacity_path {
            Some(p) => p,
            None => return None,
        };
        loop {
            self.interval.tick().await;
            // Read the capacity as an integer percentage.
            match fs::read_to_string(capacity_path) {
                Ok(cap) => {
                    if let Ok(value) = cap.trim().parse::<f64>() {
                        let level = (value / 100.0).clamp(0.0, 1.0);
                        if self.last.map_or(true, |l| (l - level).abs() > f64::EPSILON) {
                            self.last = Some(level);
                            return Some(level);
                        }
                    }
                }
                Err(_) => {
                    // On error (e.g. device removed) silently ignore
                }
            }
        }
    }
}