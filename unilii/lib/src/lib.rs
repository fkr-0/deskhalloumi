//! Core library for the `unilii` bar.
//!
//! This crate exposes a small set of modules inspired by the original
//! `liischte` Wayland bar.  It provides asynchronous streams of
//! system information such as power and backlight state, running
//! processes and keyboard input.  The intent is to separate data
//! collection from presentation so that the main bar binary can
//! remain focused on drawing to the screen.  The implementation is
//! self‑contained and depends only on standard Linux facilities like
//! sysfs, procfs, udev and evdev.
//!
//! The modules are gated behind Cargo features defined in
//! `Cargo.toml`.  For example, the `power` and `backlight` modules are
//! enabled when the corresponding feature flags are active.  All
//! features are enabled by default.

use futures::stream::BoxStream;
use log::warn;

/// Module providing asynchronous access to Linux sysfs power and
/// backlight information.  Only compiled when the `power` or
/// `backlight` features are enabled.
#[cfg(any(feature = "power", feature = "backlight"))]
pub mod sysfs;

/// Calendar and CalDAV domain types, provider traits, and cache logic.
pub mod calendar;

/// Module providing access to running processes via the procfs.
#[cfg(feature = "process")]
pub mod process;

/// Module providing access to keyboard input via the evdev subsystem.
#[cfg(feature = "input")]
pub mod input;

mod util;

/// A boxed stream with a `'static` lifetime.  All of the streams
/// produced by this crate use this type so that consumers do not have
/// to worry about lifetimes.  The streams yield values of type `T`.
pub type StaticStream<T> = BoxStream<'static, T>;

/// An extension trait that allows results returned from within a
/// stream to be logged and then converted into an `Option`.  This is
/// useful to gracefully handle errors in asynchronous streams: a
/// failing read or parse operation simply logs the error and causes
/// the corresponding item to be dropped rather than terminating the
/// entire stream.
pub trait StreamContext<T, E> {
    /// Convert a `Result<T, E>` into an `Option<T>`, logging any error
    /// with the provided stream name.  When called on an `Ok` value
    /// this returns `Some(value)`.  When called on an `Err` it logs
    /// the error and returns `None`.
    fn stream_log(self, name: &str) -> Option<T>;

    /// Convert a `Result<T, E>` into an `Option<T>`, logging any
    /// error with the provided stream name and contextual message.
    fn stream_context(self, stream: &str, context: &str) -> Option<T>;
}

impl<T, E: std::fmt::Display> StreamContext<T, E> for Result<T, E> {
    fn stream_log(self, stream: &str) -> Option<T> {
        match self {
            Ok(v) => Some(v),
            Err(e) => {
                warn!("failure in stream `{stream}`: {e:#}");
                None
            }
        }
    }

    fn stream_context(self, stream: &str, context: &str) -> Option<T> {
        match self {
            Ok(v) => Some(v),
            Err(e) => {
                warn!("failure in stream `{stream}`: {context} ({e:#})");
                None
            }
        }
    }
}
