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
const PREFIX_LIMIT: usize = 1024 * 1024;
pub(crate) const COMMAND_LIMIT: usize = 2 * STREAM_LIMIT;
const TOTAL_LIMIT: usize = 256 * 1024 * 1024;
const PAGE: usize = 32 * 1024;
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
    written: u64,
    tail: Vec<u8>,
    budget: Arc<Budget>,
}

#[cfg(test)]
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
            written: 0,
            tail: Vec::new(),
            budget,
        }
    }
    fn append(&mut self, bytes: &[u8]) {
        self.total = self.total.saturating_add(bytes.len() as u64);
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
            if self.path.is_none() {
                self.error = Some("command output retention is closed".into());
            }
            return;
        };
        let prefix = self.limit.min(PREFIX_LIMIT);
        let capacity = self.limit - prefix;
        let start = self.total - bytes.len() as u64;
        let mut write = || -> std::io::Result<()> {
            let mut cursor = start;
            let mut pending = bytes;
            if cursor < prefix as u64 {
                let count = pending.len().min(prefix - cursor as usize);
                writer.seek(SeekFrom::Start(cursor))?;
                writer.write_all(&pending[..count])?;
                cursor += count as u64;
                pending = &pending[count..];
            }
            if capacity > 0 && !pending.is_empty() {
                let skip = pending.len().saturating_sub(capacity);
                pending = &pending[skip..];
                cursor += skip as u64;
                let position = (cursor - prefix as u64) % capacity as u64;
                let count = pending.len().min(capacity - position as usize);
                writer.seek(SeekFrom::Start(prefix as u64 + position))?;
                writer.write_all(&pending[..count])?;
                if count < pending.len() {
                    writer.seek(SeekFrom::Start(prefix as u64))?;
                    writer.write_all(&pending[count..])?;
                }
            }
            Ok(())
        };
        match write() {
            Ok(()) => {
                self.retained = self.limit.min(self.total as usize);
                self.written = if capacity > 0 {
                    self.total
                } else {
                    self.total.min(prefix as u64)
                };
            }
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

    fn read(&self, file: &mut File, offset: usize, count: usize) -> std::io::Result<Vec<u8>> {
        let prefix = self.limit.min(PREFIX_LIMIT);
        let capacity = self.limit - prefix;
        let mut bytes = vec![0; count];
        let mut cursor = 0;
        while cursor < count {
            let logical = offset + cursor;
            let physical = if logical < prefix {
                logical
            } else {
                prefix + (logical - prefix) % capacity
            };
            let available = if physical < prefix {
                prefix - physical
            } else {
                self.limit - physical
            };
            let size = (count - cursor).min(available);
            file.seek(SeekFrom::Start(physical as u64))?;
            file.read_exact(&mut bytes[cursor..cursor + size])?;
            cursor += size;
        }
        Ok(bytes)
    }

    fn ranges(&self, file: &mut File) -> std::io::Result<(usize, usize, usize)> {
        let mut head = self.retained.min(self.limit.min(PREFIX_LIMIT));
        let tail_bytes = self.retained - head;
        let mut tail = (self.written as usize).saturating_sub(tail_bytes);
        if tail_bytes > 0 && tail > head {
            let start = head.saturating_sub(3);
            let suffix = self.read(file, start, head - start)?;
            head = start + super::edges(&suffix, suffix.len(), 0).0;
            let beginning = self.read(file, tail, tail_bytes.min(4))?;
            tail += beginning
                .iter()
                .take_while(|byte| **byte & 0xc0 == 0x80)
                .count();
        }
        Ok((
            head,
            tail,
            if tail_bytes > 0 {
                self.written as usize
            } else {
                head
            },
        ))
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
        if limit == 0 {
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
        let (head_end, tail_start, written) = self.ranges(&mut file).map_err(failure)?;
        let gap = tail_start > head_end && written > head_end;
        let offset = if gap && offset == head_end {
            tail_start
        } else {
            offset
        };
        let end = if gap && offset < head_end {
            head_end
        } else if offset <= written && (!gap || offset >= tail_start) {
            written
        } else {
            return Err(Fault::invalid(
                "command output offset is outside retained ranges",
            ));
        };
        let count = (end - offset).min(limit.min(PAGE).saturating_add(4));
        let bytes = self.read(&mut file, offset, count).map_err(failure)?;
        if offset > 0 && bytes.first().is_some_and(|byte| byte & 0xc0 == 0x80) {
            return Err(Fault::invalid(
                "command output offset splits a UTF-8 character",
            ));
        }
        let range = super::utf8_page(&bytes, 0, limit.min(PAGE))?;
        let next = offset + range.end;
        let next_offset = if next < end {
            Some(next)
        } else if gap && next == head_end {
            Some(tail_start)
        } else {
            None
        };
        let text = String::from_utf8_lossy(&bytes[range.clone()]).into_owned();
        Ok(OutputPage {
            info,
            stream,
            offset,
            total_bytes: self.total,
            next_offset,
            text,
            truncated: self.total > self.retained as u64,
            invalid_utf8: std::str::from_utf8(&bytes[range]).is_err(),
        })
    }

    fn preview(&self) -> Option<serde_json::Value> {
        if self.total <= (64 * 1024) as u64 {
            return None;
        }
        let (_, tail_start) = super::edges(&self.tail, 0, TAIL);
        let tail = &self.tail[tail_start..];
        let head_bytes = self.retained.min(self.limit.min(PREFIX_LIMIT));
        let tail_bytes = self.retained - head_bytes;
        let ranges = self
            .writer
            .as_ref()
            .map(|writer| writer.path())
            .or_else(|| self.path.as_deref().map(std::path::Path::new))
            .and_then(|path| File::open(path).ok())
            .and_then(|mut file| self.ranges(&mut file).ok())
            .unwrap_or((
                head_bytes,
                self.written as usize - tail_bytes,
                self.written as usize,
            ));
        Some(serde_json::json!({
            "tail":String::from_utf8_lossy(tail), "tail_offset":self.total.saturating_sub(tail.len() as u64),
            "total_bytes":self.total, "retained_bytes":self.retained,
            "retained_head_bytes":ranges.0, "retained_tail_offset":ranges.1,
            "retained_end":ranges.2,
            "invalid_utf8":std::str::from_utf8(tail).is_err(),
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

pub(crate) struct Retained {
    stdout: Mutex<Stream>,
    stderr: Mutex<Stream>,
}

#[cfg(test)]
impl Default for Retained {
    fn default() -> Self {
        Self {
            stdout: Mutex::default(),
            stderr: Mutex::default(),
        }
    }
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

    pub(crate) fn bytes(&self) -> usize {
        self.stdout.lock().unwrap().reserved + self.stderr.lock().unwrap().reserved
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
