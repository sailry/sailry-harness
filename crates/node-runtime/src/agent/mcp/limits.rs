//! The SDK's AsyncRwTransport has no configurable line limit. Bound raw bytes
//! before its parser; do not implement a second MCP decoder or truncate a result.
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncRead, ReadBuf};

pub(super) const MAX_MESSAGE: usize = 8 * 1024 * 1024;

pub(super) struct Lines<R> {
    inner: R,
    length: usize,
}

impl<R> Lines<R> {
    pub(super) fn new(inner: R) -> Self {
        Self { inner, length: 0 }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Lines<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let mut bytes = [0; 8192];
        let count = output.remaining().min(bytes.len());
        let mut read = ReadBuf::new(&mut bytes[..count]);
        ready!(Pin::new(&mut this.inner).poll_read(cx, &mut read))?;
        for byte in read.filled() {
            if *byte == b'\n' {
                this.length = 0;
            } else {
                this.length += 1;
                if this.length > MAX_MESSAGE {
                    return Poll::Ready(Err(io::Error::other(
                        "MCP message exceeds the byte limit",
                    )));
                }
            }
        }
        output.put_slice(read.filled());
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn enforces_message_limit() {
        let message = "完整结果🙂\n".repeat(MAX_MESSAGE / 8);
        let mut read = Lines::new(message.as_bytes());
        let mut output = String::new();
        read.read_to_string(&mut output).await.unwrap();
        assert_eq!(output, message);
        let mut read = Lines::new(tokio::io::repeat(b'x').take(MAX_MESSAGE as u64 + 1));
        assert!(
            tokio::io::copy(&mut read, &mut tokio::io::sink())
                .await
                .is_err()
        );
    }
}
