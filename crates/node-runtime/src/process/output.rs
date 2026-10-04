use sailry_protocol::process::Capture;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    task::JoinHandle,
};

const LIMIT: usize = 64 * 1024;

#[derive(Default)]
struct Buffer {
    bytes: Vec<u8>,
    truncated: bool,
    tail: bool,
    finished: bool,
}

impl Buffer {
    fn append(&mut self, bytes: &[u8]) {
        if self.tail {
            self.bytes.extend_from_slice(bytes);
            if self.bytes.len() > LIMIT {
                let mut discard = self.bytes.len() - LIMIT;
                while discard < self.bytes.len() && self.bytes[discard] & 0xc0 == 0x80 {
                    discard += 1;
                }
                self.bytes.drain(..discard);
                self.truncated = true;
            }
            return;
        }
        let count = bytes.len().min(LIMIT - self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..count]);
        self.truncated |= count < bytes.len();
    }

    fn capture(&self) -> Capture {
        let mut bytes = self.bytes.as_slice();
        if (!self.finished || self.truncated)
            && let Err(error) = std::str::from_utf8(bytes)
            && error.error_len().is_none()
        {
            bytes = &bytes[..error.valid_up_to()];
        }
        Capture {
            text: String::from_utf8_lossy(bytes).into_owned(),
            truncated: self.truncated,
            invalid_utf8: std::str::from_utf8(bytes).is_err(),
        }
    }
}

struct Reader {
    buffer: Arc<Mutex<Buffer>>,
    task: JoinHandle<std::io::Result<()>>,
}

impl Reader {
    fn new(mut pipe: impl AsyncRead + Unpin + Send + 'static, tail: bool) -> Self {
        let buffer = Arc::new(Mutex::new(Buffer {
            tail,
            ..Buffer::default()
        }));
        let output = buffer.clone();
        let task = tokio::spawn(async move {
            let mut bytes = [0; 8192];
            loop {
                let count = pipe.read(&mut bytes).await?;
                if count == 0 {
                    output.lock().unwrap().finished = true;
                    return Ok(());
                }
                output.lock().unwrap().append(&bytes[..count]);
            }
        });
        Self { buffer, task }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) struct Drain {
    stdout: Reader,
    stderr: Reader,
}

impl Drain {
    pub fn new(
        stdout: impl AsyncRead + Unpin + Send + 'static,
        stderr: impl AsyncRead + Unpin + Send + 'static,
    ) -> Self {
        Self::with_tail(stdout, stderr, false)
    }

    pub fn with_tail(
        stdout: impl AsyncRead + Unpin + Send + 'static,
        stderr: impl AsyncRead + Unpin + Send + 'static,
        tail: bool,
    ) -> Self {
        Self {
            stdout: Reader::new(stdout, tail),
            stderr: Reader::new(stderr, tail),
        }
    }

    pub fn snapshot(&self) -> (Capture, Capture) {
        (
            self.stdout.buffer.lock().unwrap().capture(),
            self.stderr.buffer.lock().unwrap().capture(),
        )
    }

    pub async fn finish(&mut self, timeout: Duration) -> (Capture, Capture, Option<String>) {
        let result = tokio::time::timeout(timeout, async {
            let (stdout, stderr) = tokio::join!(&mut self.stdout.task, &mut self.stderr.task);
            for result in [stdout, stderr] {
                result
                    .map_err(|error| format!("command output task failed: {error}"))?
                    .map_err(|error| format!("command output read failed: {error}"))?;
            }
            Ok::<_, String>(())
        })
        .await;
        let error = match result {
            Ok(result) => result.err(),
            Err(_) => Some("command output drain timed out".into()),
        };
        self.stdout.task.abort();
        self.stderr.task.abort();
        (
            self.stdout.buffer.lock().unwrap().capture(),
            self.stderr.buffer.lock().unwrap().capture(),
            error,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_unicode_output() {
        let mut buffer = Buffer::default();
        buffer.append(&vec![b'a'; LIMIT - 1]);
        buffer.append("🙂".as_bytes());
        let capture = buffer.capture();
        assert!(capture.truncated);
        assert!(!capture.invalid_utf8);
        assert_eq!(capture.text.len(), LIMIT - 1);
    }

    #[test]
    fn keeps_recent_unicode_output() {
        let mut buffer = Buffer {
            tail: true,
            ..Buffer::default()
        };
        buffer.append(&vec![b'a'; LIMIT]);
        buffer.append("🙂 latest output".as_bytes());
        let capture = buffer.capture();
        assert!(capture.truncated);
        assert!(!capture.invalid_utf8);
        assert!(capture.text.ends_with("🙂 latest output"));
        assert!(capture.text.len() <= LIMIT);
    }

    #[test]
    fn identifies_invalid_encoding() {
        let mut buffer = Buffer::default();
        buffer.append(&[b'a', 0xff, b'b']);
        let capture = buffer.capture();
        assert!(capture.invalid_utf8);
        assert!(!capture.truncated);
        assert_eq!(capture.text, "a�b");
    }

    #[test]
    fn snapshots_wait_for_complete_characters() {
        for tail in [false, true] {
            for text in ["中文进度 123", "🙂 456"] {
                for split in 1..text.len() {
                    let mut buffer = Buffer {
                        tail,
                        ..Buffer::default()
                    };
                    buffer.append(&text.as_bytes()[..split]);
                    let capture = buffer.capture();
                    assert!(!capture.invalid_utf8);
                    assert!(text.starts_with(&capture.text));
                    buffer.append(&text.as_bytes()[split..]);
                    buffer.finished = true;
                    let capture = buffer.capture();
                    assert_eq!(capture.text, text);
                    assert!(!capture.invalid_utf8);
                }
            }
        }
    }

    #[tokio::test]
    async fn readers_preserve_split_unicode() {
        use tokio::io::AsyncWriteExt;
        for tail in [false, true] {
            let (mut writer, reader) = tokio::io::duplex(64);
            let mut drain = Drain::with_tail(reader, tokio::io::empty(), tail);
            let text = "中文进度 123 🙂";
            writer.write_all(&text.as_bytes()[..2]).await.unwrap();
            tokio::time::timeout(Duration::from_secs(1), async {
                while drain.stdout.buffer.lock().unwrap().bytes.len() < 2 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let (stdout, _) = drain.snapshot();
            assert_eq!(stdout.text, "");
            assert!(!stdout.invalid_utf8);
            writer.write_all(&text.as_bytes()[2..]).await.unwrap();
            writer.shutdown().await.unwrap();
            let (stdout, _, error) = drain.finish(Duration::from_secs(1)).await;
            assert!(error.is_none());
            assert_eq!(stdout.text, text);
            assert!(!stdout.invalid_utf8);
        }
    }

    #[test]
    fn incomplete_final_output_is_invalid() {
        let mut buffer = Buffer::default();
        buffer.append(&[b'a', 0xe4, 0xb8]);
        assert_eq!(buffer.capture().text, "a");
        buffer.finished = true;
        let capture = buffer.capture();
        assert!(capture.invalid_utf8);
        assert_eq!(capture.text, "a�");
    }
}
