use sailry_protocol::process::Capture;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    task::JoinHandle,
};

mod retained;
pub(crate) use retained::{Budget, COMMAND_LIMIT, Retained, clear};

const LIMIT: usize = 64 * 1024;

/// Locate UTF-8 boundaries without overlapping the retained byte ranges.
pub(crate) fn edges(bytes: &[u8], head: usize, tail: usize) -> (usize, usize) {
    let mut head = head.min(bytes.len());
    while head > 0 && bytes.get(head).is_some_and(|byte| byte & 0xc0 == 0x80) {
        head -= 1;
    }
    if let Some(start) = (head.saturating_sub(3)..head)
        .rev()
        .find(|index| bytes[*index] & 0xc0 != 0x80)
    {
        let width = match bytes[start] {
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 1,
        };
        if head - start < width {
            head = start;
        }
    }
    let mut tail = bytes.len().saturating_sub(tail).max(head);
    while bytes.get(tail).is_some_and(|byte| byte & 0xc0 == 0x80) {
        tail += 1;
    }
    (head, tail)
}

/// Select one raw byte page without splitting a UTF-8 sequence.
pub(crate) fn utf8_page(
    bytes: &[u8],
    offset: usize,
    limit: usize,
) -> Result<std::ops::Range<usize>, sailry_protocol::Fault> {
    if offset > bytes.len() || limit == 0 {
        return Err(sailry_protocol::Fault::invalid("output page is invalid"));
    }
    if offset > 0 && bytes.get(offset).is_some_and(|byte| byte & 0xc0 == 0x80) {
        return Err(sailry_protocol::Fault::invalid(
            "output page offset splits a UTF-8 character",
        ));
    }
    let mut end = offset.saturating_add(limit).min(bytes.len());
    while end > offset && bytes.get(end).is_some_and(|byte| byte & 0xc0 == 0x80) {
        end -= 1;
    }
    if end == offset && end < bytes.len() {
        end += 1;
        while bytes.get(end).is_some_and(|byte| byte & 0xc0 == 0x80) {
            end += 1;
        }
    }
    Ok(offset..end)
}

pub(crate) fn command_id(value: &serde_json::Value) -> Option<&str> {
    value
        .pointer("/sailry_command/id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            matches!(
                value["kind"].as_str(),
                Some("command_result" | "command_output")
            )
            .then(|| {
                value
                    .pointer("/data/info/id")
                    .or_else(|| value.pointer("/info/id"))
            })
            .flatten()
            .and_then(serde_json::Value::as_str)
        })
}

#[derive(Default)]
pub(super) struct Buffer {
    bytes: Vec<u8>,
    truncated: bool,
    tail: bool,
    finished: bool,
}

impl Buffer {
    pub(super) fn append(&mut self, bytes: &[u8]) {
        if self.tail {
            self.bytes.extend_from_slice(bytes);
            if self.bytes.len() > LIMIT {
                let (_, discard) = edges(&self.bytes, 0, LIMIT);
                self.bytes.drain(..discard);
                self.truncated = true;
            }
            return;
        }
        let count = bytes.len().min(LIMIT - self.bytes.len());
        self.bytes.extend_from_slice(&bytes[..count]);
        self.truncated |= count < bytes.len();
    }

    pub(super) fn capture(&self) -> Capture {
        let mut bytes = self.bytes.as_slice();
        if !self.finished || self.truncated && !self.tail {
            let (end, _) = edges(bytes, bytes.len(), 0);
            bytes = &bytes[..end];
        }
        Capture {
            text: String::from_utf8_lossy(bytes).into_owned(),
            truncated: self.truncated,
            invalid_utf8: std::str::from_utf8(bytes).is_err(),
        }
    }

    pub(super) fn tail() -> Self {
        Self {
            tail: true,
            ..Self::default()
        }
    }

    pub(super) fn finish(&mut self) {
        self.finished = true;
    }
}

struct Reader {
    buffer: Arc<Mutex<Buffer>>,
    task: JoinHandle<std::io::Result<()>>,
}

impl Reader {
    fn new(
        mut pipe: impl AsyncRead + Unpin + Send + 'static,
        tail: bool,
        retained: Option<(Arc<Retained>, sailry_protocol::process::OutputStream)>,
    ) -> Self {
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
                if let Some((retained, stream)) = &retained {
                    retained.append(*stream, &bytes[..count]);
                }
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
        Self::with_retained(stdout, stderr, false, None)
    }

    pub fn with_retained(
        stdout: impl AsyncRead + Unpin + Send + 'static,
        stderr: impl AsyncRead + Unpin + Send + 'static,
        tail: bool,
        retained: Option<Arc<Retained>>,
    ) -> Self {
        Self {
            stdout: Reader::new(
                stdout,
                tail,
                retained
                    .clone()
                    .map(|record| (record, sailry_protocol::process::OutputStream::Stdout)),
            ),
            stderr: Reader::new(
                stderr,
                tail,
                retained.map(|record| (record, sailry_protocol::process::OutputStream::Stderr)),
            ),
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
    fn bounds_incomplete_suffix_after_invalid_bytes() {
        let mut bytes = vec![b'x'; 8192];
        bytes[10] = 0xff;
        bytes[8190..].copy_from_slice(&[0xe4, 0xb8]);
        assert_eq!(edges(&bytes, bytes.len(), 0).0, 8190);
        bytes.push(0xad);
        assert_eq!(edges(&bytes, 8192, 0).0, 8190);
        assert_eq!(edges(&bytes, bytes.len(), 0).0, bytes.len());
        let mut buffer = Buffer::default();
        buffer.append(&bytes[..8192]);
        assert_eq!(buffer.capture().text.len(), 8192); // one invalid byte expands by two bytes
        buffer.append(&bytes[8192..]);
        assert!(buffer.capture().text.ends_with('中'));
    }

    #[test]
    fn rejects_business_ids_for_command_pages() {
        assert!(command_id(&serde_json::json!({"data":{"info":{"id":"business"}}})).is_none());
        assert_eq!(
            command_id(
                &serde_json::json!({"kind":"command_output","data":{"info":{"id":"command"}}})
            ),
            Some("command")
        );
    }

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
            let mut drain = Drain::with_retained(reader, tokio::io::empty(), tail, None);
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
        let mut tail = Buffer::tail();
        tail.append(&vec![b'a'; LIMIT]);
        tail.append(&[0xe4, 0xb8]);
        assert!(!tail.capture().invalid_utf8);
        tail.finish();
        let capture = tail.capture();
        assert!(capture.truncated && capture.invalid_utf8);
        assert!(capture.text.ends_with('�'));
    }
}
