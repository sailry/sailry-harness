use rolling_file::{RollingCondition, RollingFileAppender};
use std::{
    fs::{File, OpenOptions, TryLockError},
    io::{self, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

pub(super) const CURRENT: &str = "current.log";
const LOCK: &str = "writer.lock";

#[derive(Debug)]
struct Explicit;

impl RollingCondition for Explicit {
    fn should_rollover(&mut self, _: &chrono::DateTime<chrono::Local>, _: u64) -> bool {
        false
    }
}

/// The existing appender rotates complete records before they exceed the limit.
pub(super) struct Writer {
    inner: RollingFileAppender<Explicit>,
    // Keep the lease until the worker has drained and the appender has closed.
    _lease: File,
    maximum: u64,
    current: u64,
    failed: bool,
    pub errors: Arc<AtomicUsize>,
}

impl Writer {
    pub fn new(directory: &Path, maximum: usize, retained: usize) -> io::Result<Self> {
        if maximum == 0 || retained == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid log limits",
            ));
        }
        std::fs::create_dir_all(directory)?;
        let lock = directory.join(LOCK);
        if lock
            .symlink_metadata()
            .is_ok_and(|metadata| !metadata.is_file())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid diagnostic writer lock",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lease = options.open(lock)?;
        lease.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => io::Error::new(
                io::ErrorKind::WouldBlock,
                "diagnostic stream already has a writer",
            ),
            TryLockError::Error(error) => error,
        })?;
        let path = directory.join(CURRENT);
        match path.symlink_metadata() {
            Ok(metadata) if !metadata.is_file() || metadata.len() > maximum as u64 => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid current log file",
                ));
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && archive(entry.file_name().to_str().unwrap_or_default())
                    .is_some_and(|index| index >= retained)
            {
                std::fs::remove_file(entry.path())?;
            }
        }
        Ok(Self {
            inner: RollingFileAppender::new_with_buffer_capacity(path, Explicit, retained - 1, 0)?,
            _lease: lease,
            maximum: maximum as u64,
            current: directory.join(CURRENT).metadata()?.len(),
            failed: false,
            errors: Arc::default(),
        })
    }

    fn failure(&self, error: &io::Error) {
        if self.errors.fetch_add(1, Ordering::Relaxed) == 0 {
            eprintln!(
                "sailry logging: diagnostic write failed ({:?})",
                error.kind()
            );
        }
    }
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Safe formatting bounds every field before this point. Reject a raw
        // oversized write explicitly rather than split UTF-8 or lose a suffix.
        let result = if self.failed {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "diagnostic writer stopped after an I/O failure",
            ))
        } else if bytes.len() as u64 > self.maximum {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "diagnostic record exceeds the file limit",
            ))
        } else {
            // Calling rollover explicitly preserves its I/O error. The appender's
            // automatic condition path otherwise logs that error and keeps writing.
            let rotation = if self.current > 0
                && self.current.saturating_add(bytes.len() as u64) > self.maximum
            {
                self.inner.rollover().inspect(|_| self.current = 0)
            } else {
                Ok(())
            };
            let result = rotation
                .and_then(|_| self.inner.write(bytes))
                .inspect(|written| self.current += *written as u64);
            // An I/O error can follow a partial write. Stop rather than infer an
            // unchanged file size and risk exceeding the limit on a later write.
            self.failed = result.is_err();
            result
        };
        if let Err(error) = &result {
            self.failure(error);
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = self.inner.flush();
        if let Err(error) = &result {
            self.failed = true;
            self.failure(error);
        }
        result
    }
}

fn archive(name: &str) -> Option<usize> {
    let suffix = name.strip_prefix(CURRENT)?.strip_prefix('.')?;
    let index: usize = suffix.parse().ok()?;
    (index > 0 && index.to_string() == suffix).then_some(index)
}

/// Only the process that acquired the Node profile may retire old daily logs.
/// Other files, directories and symlinks in the log directory remain untouched.
pub(super) fn retire(directory: &Path) -> io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(date) = name.to_str().and_then(|name| name.strip_suffix(".log")) else {
            continue;
        };
        if date.len() == 10 && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok() {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
