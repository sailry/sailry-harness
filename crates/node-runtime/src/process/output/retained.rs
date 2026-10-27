//! File-backed output belongs to each existing managed-command entry.
use sailry_protocol::{
    ErrorCode, Fault, RequestId,
    process::{Info, OutputPage, OutputStream},
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

const STREAM_LIMIT: usize = 4 * 1024 * 1024;
pub(crate) const COMMAND_LIMIT: usize = 2 * STREAM_LIMIT;
const TOTAL_LIMIT: usize = 256 * 1024 * 1024;
const PAGE: usize = 32 * 1024;
const HEAD: usize = 8 * 1024;
const TAIL: usize = 8 * 1024;

pub(crate) struct Budget {
    used: AtomicUsize,
    limit: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit: TOTAL_LIMIT,
        }
    }
}

impl Budget {
    pub(crate) fn available(&self) -> usize {
        self.limit.saturating_sub(self.used.load(Ordering::Relaxed))
    }

    #[cfg(test)]
    pub(crate) fn limited(limit: usize) -> Self {
        Self {
            used: AtomicUsize::new(0),
            limit,
        }
    }

    fn take(&self, requested: usize) -> usize {
        let previous = self
            .used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                Some(used.saturating_add(requested).min(self.limit))
            })
            .expect("the retention budget update always succeeds");
        requested.min(self.limit.saturating_sub(previous))
    }
}

pub(crate) fn clear(profile: &std::path::Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(profile.join("tmp")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("command-output-"))
            && (kind.is_file() || kind.is_symlink())
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

struct Stream {
    writer: Option<tempfile::NamedTempFile>,
    path: Option<tempfile::TempPath>,
    error: Option<String>,
    retained: usize,
    reserved: usize,
    limit: usize,
    total: u64,
    head: Vec<u8>,
    tail: Vec<u8>,
    budget: Arc<Budget>,
}

impl Default for Stream {
    fn default() -> Self {
        Self::new(tempfile::NamedTempFile::new(), Arc::default(), STREAM_LIMIT)
    }
}

impl Stream {
    fn new(
        created: std::io::Result<tempfile::NamedTempFile>,
        budget: Arc<Budget>,
        limit: usize,
    ) -> Self {
        let (writer, error) = match created {
            Ok(file) => (Some(file), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let reserved = if writer.is_some() {
            budget.take(limit)
        } else {
            0
        };
        Self {
            writer,
            path: None,
            error,
            retained: 0,
            reserved,
            limit: reserved,
            total: 0,
            head: Vec::new(),
            tail: Vec::new(),
            budget,
        }
    }
    fn append(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len() as u64);
        self.head
            .extend_from_slice(&bytes[..bytes.len().min(HEAD - self.head.len())]);
        if bytes.len() >= TAIL {
            self.tail.clear();
            self.tail.extend_from_slice(&bytes[bytes.len() - TAIL..]);
        } else {
            let discard = (self.tail.len() + bytes.len()).saturating_sub(TAIL);
            self.tail.drain(..discard);
            self.tail.extend_from_slice(bytes);
        }
        if bytes.is_empty() || self.error.is_some() {
            return;
        }
        let Some(writer) = &mut self.writer else {
            self.error = Some("command output retention is closed".into());
            return;
        };
        let count = bytes.len().min(self.limit.saturating_sub(self.retained));
        match writer.write_all(&bytes[..count]) {
            Ok(()) => self.retained += count,
            Err(error) => {
                self.error = Some(error.to_string());
                self.budget.used.fetch_sub(self.reserved, Ordering::Relaxed);
                self.retained = 0;
                self.reserved = 0;
                self.writer.take();
            }
        }
    }

    fn finish(&mut self) {
        if let Some(writer) = self.writer.take() {
            self.path = Some(writer.into_temp_path());
        }
        self.budget
            .used
            .fetch_sub(self.reserved - self.retained, Ordering::Relaxed);
        self.reserved = self.retained;
    }

    fn evict(&mut self) {
        self.writer.take();
        self.path.take();
        self.budget.used.fetch_sub(self.reserved, Ordering::Relaxed);
        self.retained = 0;
        self.reserved = 0;
        self.error = Some("command output was evicted".into());
    }

    fn page(
        &self,
        info: Info,
        stream: OutputStream,
        offset: usize,
        limit: usize,
    ) -> Result<OutputPage, Fault> {
        if let Some(error) = &self.error {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                format!("retained command output is unavailable: {error}"),
            ));
        }
        if offset > self.retained || limit == 0 {
            return Err(Fault::invalid("command output page is invalid"));
        }
        let path = self
            .writer
            .as_ref()
            .map(|writer| writer.path())
            .or_else(|| self.path.as_deref().map(std::path::Path::new))
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::Unavailable,
                    "retained command output is unavailable",
                )
            })?;
        let mut file = File::open(path).map_err(failure)?;
        file.seek(SeekFrom::Start(offset as u64)).map_err(failure)?;
        let count = (self.retained - offset).min(limit.min(PAGE).saturating_add(4));
        let mut bytes = vec![0; count];
        file.read_exact(&mut bytes).map_err(failure)?;
        if offset > 0 && bytes.first().is_some_and(|byte| byte & 0xc0 == 0x80) {
            return Err(Fault::invalid(
                "command output offset splits a UTF-8 character",
            ));
        }
        let mut end = bytes.len().min(limit.min(PAGE));
        while end > 0 && bytes.get(end).is_some_and(|byte| byte & 0xc0 == 0x80) {
            end -= 1;
        }
        if end == 0 && !bytes.is_empty() {
            end = 1;
            while bytes.get(end).is_some_and(|byte| byte & 0xc0 == 0x80) {
                end += 1;
            }
        }
        let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
        Ok(OutputPage {
            info,
            stream,
            offset,
            total_bytes: self.total,
            next_offset: (offset + end < self.retained).then_some(offset + end),
            text,
            truncated: self.total > self.retained as u64,
            invalid_utf8: std::str::from_utf8(&bytes[..end]).is_err(),
        })
    }

    fn preview(&self) -> Option<serde_json::Value> {
        if self.total <= (64 * 1024) as u64 {
            return None;
        }
        let (head_end, _) = super::edges(&self.head, HEAD, 0);
        let (_, tail_start) = super::edges(&self.tail, 0, TAIL);
        let head = &self.head[..head_end];
        let tail = &self.tail[tail_start..];
        Some(serde_json::json!({
            "head":String::from_utf8_lossy(head), "head_bytes":head.len(),
            "tail":String::from_utf8_lossy(tail), "tail_offset":self.total.saturating_sub(tail.len() as u64),
            "total_bytes":self.total, "retained_bytes":self.retained,
            "invalid_utf8":std::str::from_utf8(head).is_err() || std::str::from_utf8(tail).is_err(),
        }))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.reserved, Ordering::Relaxed);
    }
}

fn failure(error: std::io::Error) -> Fault {
    Fault::new(
        ErrorCode::Unavailable,
        format!("retained command output is unavailable: {error}"),
    )
}

#[derive(Default)]
pub(crate) struct Retained {
    stdout: Mutex<Stream>,
    stderr: Mutex<Stream>,
}

impl Retained {
    pub(crate) fn new(profile: &std::path::Path, budget: Arc<Budget>) -> Self {
        let directory = profile.join("tmp");
        let created = std::fs::create_dir_all(&directory);
        let create = || match &created {
            Ok(()) => tempfile::Builder::new()
                .prefix("command-output-")
                .tempfile_in(&directory),
            Err(error) => Err(std::io::Error::new(error.kind(), error.to_string())),
        };
        Self {
            stdout: Mutex::new(Stream::new(create(), budget.clone(), STREAM_LIMIT)),
            stderr: Mutex::new(Stream::new(create(), budget, STREAM_LIMIT)),
        }
    }

    fn stream(&self, stream: OutputStream) -> &Mutex<Stream> {
        match stream {
            OutputStream::Stdout => &self.stdout,
            OutputStream::Stderr => &self.stderr,
        }
    }

    pub(crate) fn append(&self, stream: OutputStream, bytes: &[u8]) {
        self.stream(stream).lock().unwrap().append(bytes);
    }

    pub(crate) fn finish(&self) {
        self.stdout.lock().unwrap().finish();
        self.stderr.lock().unwrap().finish();
    }

    pub(crate) fn evict(&self) {
        self.stdout.lock().unwrap().evict();
        self.stderr.lock().unwrap().evict();
    }

    pub(crate) fn page(
        &self,
        info: Info,
        stream: OutputStream,
        offset: usize,
        limit: usize,
    ) -> Result<OutputPage, Fault> {
        self.stream(stream)
            .lock()
            .unwrap()
            .page(info, stream, offset, limit)
    }

    pub(crate) fn preview(&self, id: RequestId) -> Option<serde_json::Value> {
        let stdout = self.stdout.lock().unwrap().preview();
        let stderr = self.stderr.lock().unwrap().preview();
        (stdout.is_some() || stderr.is_some())
            .then(|| serde_json::json!({"id":id,"stdout":stdout,"stderr":stderr}))
    }
}

#[cfg(test)]
mod tests;
