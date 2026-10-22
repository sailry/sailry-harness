//! Return the stdout reader to the process owner after the SDK closes its transport.
use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, ReadBuf},
    process::ChildStdout,
};

pub(super) type Retained = Arc<Mutex<Option<ChildStdout>>>;

pub(super) struct Reader {
    inner: Option<ChildStdout>,
    retained: Retained,
}

impl Reader {
    pub(super) fn new(inner: ChildStdout) -> (Self, Retained) {
        let retained = Arc::new(Mutex::new(None));
        (
            Self {
                inner: Some(inner),
                retained: retained.clone(),
            },
            retained,
        )
    }
}

impl AsyncRead for Reader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().inner.as_mut().expect("stdout remains owned")).poll_read(cx, output)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        *self.retained.lock().unwrap() = self.inner.take();
    }
}
