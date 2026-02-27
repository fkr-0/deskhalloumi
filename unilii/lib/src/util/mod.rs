//! Miscellaneous helper types and functions used throughout the
//! library.  These helpers are not exposed publicly but assist the
//! various modules in dealing with asynchronous udev streams and
//! stream combinators.

use futures::Future;
pub(crate) mod udev;

/// Simple wrapper around `tokio::fs::ReadDir` to make it a stream.
/// This is needed because newer versions of tokio-stream removed
/// `ReadDirStream` from their wrappers module.
pub struct ReadDirStream {
    inner: tokio::fs::ReadDir,
}

impl ReadDirStream {
    pub fn new(inner: tokio::fs::ReadDir) -> Self {
        Self { inner }
    }
}

impl futures::Stream for ReadDirStream {
    type Item = std::io::Result<tokio::fs::DirEntry>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;

        // Create a future for the next_entry call
        let future = self.inner.next_entry();

        // Use a pinning helper to poll the future
        tokio::pin!(future);

        match future.poll(cx) {
            Poll::Ready(Ok(Some(entry))) => Poll::Ready(Some(Ok(entry))),
            Poll::Ready(Ok(None)) => Poll::Ready(None),
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}