//! Monitoring of running processes.
//!
//! This module interacts with the procfs (`/proc`) to read
//! information about processes.  It provides both a one‑shot
//! function to list currently running processes and a stream that
//! periodically polls the process table.  A helper function allows
//! sending signals to processes via `nix`.

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use futures::StreamExt;
use log::{trace, warn};
use nix::{sys::signal::kill, unistd::Pid};
use tokio::{fs, time::Instant};

use crate::util::ReadDirStream;
use crate::{StaticStream, StreamContext};

pub use nix::sys::signal::Signal as ProcessSignal;

/// Information about a process.  Includes its PID, name (from
/// `/proc/<pid>/comm`) and command line (space separated from
/// `/proc/<pid>/cmdline`).
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Process ID.
    pub pid: u64,
    /// Name of the process (not the executable name).
    pub name: String,
    /// Command line (arguments separated by spaces).
    pub cmdline: String,
}

/// Read the list of running processes by scanning the procfs.  Each
/// entry in `/proc` that is a numeric directory is interpreted as a
/// PID.  Process names and command lines are read from `comm` and
/// `cmdline` respectively.  Missing or unreadable entries are
/// skipped with a warning.
pub async fn read_running_processes() -> Result<Vec<ProcessInfo>> {
    let entries = fs::read_dir("/proc")
        .await
        .context("cannot access procfs, are you on Linux?")?;
    Ok(ReadDirStream::new(entries)
        .filter_map(async |result| result.ok())
        .filter_map(async |entry| {
            entry
                .file_name()
                .into_string()
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
        })
        .filter_map(async |pid| {
            let dir = PathBuf::from("/proc").join(pid.to_string());
            let comm_path = dir.join("comm");
            let cmdline_path = dir.join("cmdline");
            let Ok(name) = fs::read_to_string(&comm_path).await else {
                warn!("failed to read comm attribute for process `{pid}`");
                return None;
            };
            let Ok(cmdline) = fs::read_to_string(&cmdline_path).await else {
                warn!("failed to read cmdline attribute for process `{pid}`");
                return None;
            };
            Some(ProcessInfo {
                pid,
                name: name.trim().to_owned(),
                cmdline: cmdline.replace('\0', " ").trim().to_owned(),
            })
        })
        .collect()
        .await)
}

/// Create a stream that polls the list of running processes at the
/// given interval.  The stream emits a vector of [`ProcessInfo`]
/// representing the processes at each poll.  Errors reading the
/// process table are logged and cause the poll to be skipped.
pub fn listen_running_processes(polling: Duration) -> StaticStream<Vec<ProcessInfo>> {
    let mut interval = tokio::time::interval_at(Instant::now(), polling);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    futures::stream::unfold(interval, async move |mut interval| {
        interval.tick().await;
        trace!("polling running process information");
        let procs = read_running_processes()
            .await
            .stream_log("running processes stream")?;
        Some((procs, interval))
    })
    .boxed()
}

/// Send a Unix signal to a process identified by its PID.  This is
/// simply a thin wrapper around `nix::sys::signal::kill` that
/// provides a convenient error message.
pub fn send_signal(pid: u64, signal: ProcessSignal) -> Result<()> {
    kill(Pid::from_raw(pid as i32), signal)
        .with_context(|| format!("failed to send signal `{signal}` to process `{pid}`"))
}
