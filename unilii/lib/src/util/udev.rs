//! Asynchronous udev monitor support.
//!
//! This module provides an [`AsyncMonitorSocket`] type which wraps a
//! udev [`MonitorSocket`] and implements the [`Stream`] trait for
//! `Event`s.  The implementation is adapted from the original
//! `tokio-udev` crate but is reproduced here so that the resulting
//! type implements [`Send`], allowing it to be used across async
//! tasks.  Without this wrapper the stream returned by
//! `tokio-udev` would not be `Send` and thus could not be passed
//! between threads.

use futures_core::stream::Stream;
use std::{io, pin::Pin, sync::Mutex, task::Poll};
use tokio::io::unix::AsyncFd;
use udev::{Event, MonitorSocket};

/// An asynchronous stream of udev events.  Wraps a udev
/// [`MonitorSocket`] in a [`tokio::io::unix::AsyncFd`] so that
/// readiness notifications can be integrated with the tokio runtime.
///
/// The stream yields `Result<Event, io::Error>` items.  On error
/// the stream will continue to yield further items (unless the
/// underlying file descriptor is no longer valid).  The type
/// implements [`Send`], unlike the equivalent type from
/// `tokio-udev`.
pub struct AsyncMonitorSocket {
    inner: Mutex<Inner>,
}

impl AsyncMonitorSocket {
    /// Construct an [`AsyncMonitorSocket`] from an existing udev
    /// [`MonitorSocket`].  Returns an [`io::Error`] if the file
    /// descriptor cannot be registered with the tokio reactor.
    pub fn new(monitor: MonitorSocket) -> io::Result<Self> {
        Ok(Self {
            inner: Mutex::new(Inner::new(monitor)?),
        })
    }
}

impl Stream for AsyncMonitorSocket {
    type Item = Result<Event, io::Error>;

    fn poll_next(
        self: Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match self.inner.lock() {
            Ok(mut inner) => inner.poll_receive(ctx),
            Err(poisoned) => poisoned.into_inner().poll_receive(ctx),
        }
    }
}

struct Inner {
    fd: AsyncFd<MonitorSocket>,
}

impl Inner {
    fn new(monitor: MonitorSocket) -> io::Result<Self> {
        Ok(Self {
            fd: AsyncFd::new(monitor)?,
        })
    }

    fn poll_receive(
        &mut self,
        ctx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<Event, io::Error>>> {
        loop {
            // Try to retrieve the next event from the udev monitor.  The
            // `iter()` iterator yields only events that are ready.  If
            // there are none we need to await readiness on the fd.
            if let Some(event) = self.fd.get_mut().iter().next() {
                return Poll::Ready(Some(Ok(event)));
            }
            match self.fd.poll_read_ready(ctx) {
                Poll::Ready(Ok(mut guard)) => {
                    guard.clear_ready();
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
