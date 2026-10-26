//! File-backed output belongs to each existing managed-command entry.
use sailry_protocol::{
    ErrorCode, Fault, RequestId,
    process::{Capture, Info, OutputPage, OutputStream},
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    sync::Mutex,
};

const LIMIT: usize = 32 * 1024 * 1024;
const PAGE: usize = 32 * 1024;
const HEAD: usize = 8 * 1024;
const TAIL: usize = 24 * 1024;

struct Stream {
    writer: Option<tempfile::NamedTempFile>,
    path: Option<tempfile::TempPath>,
    error: Option<String>,
    retained: usize,
    total: u64,
    head: Vec<u8>,
    tail: Vec<u8>,
}

impl Default for Stream {
    fn default() -> Self {
        Self::new(tempfile::NamedTempFile::new())
    }
}

impl Stream {
    fn new(created: std::io::Result<tempfile::NamedTempFile>) -> Self {
        let (writer, error) = match created {
            Ok(file) => (Some(file), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            writer,
            path: None,
            error,
            retained: 0,
            total: 0,
            head: Vec::new(),
            tail: Vec::new(),
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
        let count = bytes.len().min(LIMIT - self.retained);
        if count == 0 || self.error.is_some() {
            return;
        }
        let Some(writer) = &mut self.writer else {
            self.error = Some("command output retention is closed".into());
            return;
        };
        match writer.write_all(&bytes[..count]) {
            Ok(()) => self.retained += count,
            Err(error) => {
                self.error = Some(error.to_string());
                self.writer.take();
            }
        }
    }

    fn finish(&mut self) {
        if let Some(writer) = self.writer.take() {
            self.path = Some(writer.into_temp_path());
        }
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

    fn preview(&self, id: RequestId, stream: OutputStream) -> Option<Capture> {
        if self.total <= (64 * 1024) as u64 {
            return None;
        }
        let mut head_end = self.head.len();
        if let Err(error) = std::str::from_utf8(&self.head)
            && error.error_len().is_none()
        {
            head_end = error.valid_up_to();
        }
        let mut tail_start = 0;
        while self
            .tail
            .get(tail_start)
            .is_some_and(|byte| byte & 0xc0 == 0x80)
        {
            tail_start += 1;
        }
        let head = &self.head[..head_end];
        let tail = &self.tail[tail_start..];
        let omitted = self.total.saturating_sub((head.len() + tail.len()) as u64);
        let stream = match stream {
            OutputStream::Stdout => "stdout",
            OutputStream::Stderr => "stderr",
        };
        let unavailable = if self.error.is_some() || self.total > self.retained as u64 {
            "; complete retained output is unavailable"
        } else {
            ""
        };
        Some(Capture {
            text: format!(
                "{}\n[... {omitted} bytes omitted; read_command id={id} stream={stream} offset=0 for retained output{unavailable} ...]\n{}",
                String::from_utf8_lossy(head),
                String::from_utf8_lossy(tail)
            ),
            truncated: true,
            invalid_utf8: std::str::from_utf8(head).is_err() || std::str::from_utf8(tail).is_err(),
        })
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
    pub(crate) fn new(profile: &std::path::Path) -> Self {
        let directory = profile.join("tmp");
        let created = std::fs::create_dir_all(&directory);
        let create = || match &created {
            Ok(()) => tempfile::Builder::new()
                .prefix("command-output-")
                .tempfile_in(&directory),
            Err(error) => Err(std::io::Error::new(error.kind(), error.to_string())),
        };
        Self {
            stdout: Mutex::new(Stream::new(create())),
            stderr: Mutex::new(Stream::new(create())),
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
        let stdout = self
            .stdout
            .lock()
            .unwrap()
            .preview(id, OutputStream::Stdout);
        let stderr = self
            .stderr
            .lock()
            .unwrap()
            .preview(id, OutputStream::Stderr);
        (stdout.is_some() || stderr.is_some())
            .then(|| serde_json::json!({"id":id,"stdout":stdout,"stderr":stderr}))
    }
}

#[cfg(test)]
mod tests;
