//! Lifecycle reference: sailry-platform 3749287 auxiliary.rs. Reuses the existing
//! authenticated connection; no separate endpoint, pairing or replay owner.
use iroh::endpoint::{RecvStream, SendStream};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub(super) struct NetworkStream {
    send: SendStream,
    recv: RecvStream,
    finished: bool,
}

impl NetworkStream {
    pub(super) fn new(send: SendStream, recv: RecvStream) -> Self {
        Self {
            send,
            recv,
            finished: false,
        }
    }
}

impl AsyncRead for NetworkStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buffer)
    }
}

impl AsyncWrite for NetworkStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        AsyncWrite::poll_write(Pin::new(&mut self.send), cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.send).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let result = Pin::new(&mut self.send).poll_shutdown(cx);
        if matches!(result, Poll::Ready(Ok(()))) {
            self.finished = true;
        }
        result
    }
}

impl Drop for NetworkStream {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.send.reset(2u32.into());
        }
        let _ = self.recv.stop(2u32.into());
    }
}
