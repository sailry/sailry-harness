//! Process-owned rotating diagnostics; Node instances never install subscribers.
//! Records contain identifiers and lifecycle metadata, never operation payloads.
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
use tracing_appender::{
    non_blocking::{ErrorCounter, WorkerGuard},
    rolling::{RollingFileAppender, Rotation},
};
use tracing_log::AsLog as _;
use tracing_subscriber::{Layer, filter::filter_fn, layer::SubscriberExt};

pub(crate) mod commands;
mod format;
mod records;
pub use records::{Area, Context, Entry, Phase, Span};
#[cfg(test)]
mod tests;

const RETAINED_FILES: usize = 7;
const QUEUED_LINES: usize = 8192;
static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

#[derive(Clone, Copy)]
pub enum Application {
    Desktop,
    Host,
}
impl Application {
    fn name(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Host => "host",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("logging is unavailable")]
pub struct InitError;

/// Retain until ordinary Node shutdown and the application run loop have ended.
#[must_use]
pub struct Guard {
    worker: Option<WorkerGuard>,
    counter: ErrorCounter,
    _temporary: Option<tempfile::TempDir>,
}

pub fn directory() -> Option<&'static Path> {
    DIRECTORY.get().map(PathBuf::as_path)
}

pub fn install(profile: &Path, application: Application) -> Result<Guard, InitError> {
    let directory = profile.join("logs").join(application.name());
    let (writer, worker) = writer(&directory, Rotation::DAILY, RETAINED_FILES, QUEUED_LINES)?;
    let counter = writer.error_counter();
    tracing::subscriber::set_global_default(subscriber(writer, application))
        .map_err(|_| InitError)?;
    if tracing_log::LogTracer::builder()
        .with_max_level(tracing::level_filters::LevelFilter::INFO.as_log())
        .init()
        .is_err()
    {
        let mut entry = Entry::new(Area::Logging, Phase::Failed);
        entry.reason = Some("log_bridge");
        entry.code = Some(sailry_protocol::ErrorCode::Unavailable);
        entry.emit_warn();
        eprintln!("sailry logging: the existing log logger was preserved");
    }
    let _ = DIRECTORY.set(profile.join("logs"));
    Ok(Guard {
        worker: Some(worker),
        counter,
        _temporary: None,
    })
}

/// Preview diagnostics have no relationship to the normal data profile.
pub fn preview() -> Result<Guard, InitError> {
    let temporary = tempfile::Builder::new()
        .prefix("sailry-preview-logs-")
        .tempdir()
        .map_err(|_| InitError)?;
    let mut guard = install(temporary.path(), Application::Desktop)?;
    guard._temporary = Some(temporary);
    Ok(guard)
}

fn writer(
    directory: &Path,
    rotation: Rotation,
    retained: usize,
    capacity: usize,
) -> Result<(tracing_appender::non_blocking::NonBlocking, WorkerGuard), InitError> {
    // No prefix/suffix: retention only matches the appender's exact date format.
    // The enclosing profile or preview TempDir already confines these records.
    let appender = RollingFileAppender::builder()
        .rotation(rotation)
        .max_log_files(retained)
        .build(directory)
        .map_err(|_| InitError)?;
    Ok(
        tracing_appender::non_blocking::NonBlockingBuilder::default()
            .buffered_lines_limit(capacity)
            .lossy(true)
            .finish(appender),
    )
}

fn subscriber(
    writer: tracing_appender::non_blocking::NonBlocking,
    application: Application,
) -> impl tracing::Subscriber + Send + Sync {
    tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(format::Safe {
                application: application.name(),
            })
            .with_writer(writer)
            .with_ansi(false)
            .with_filter(filter_fn(|metadata| {
                metadata.target() == "sailry" && *metadata.level() <= tracing::Level::DEBUG
            })),
    )
}

impl Guard {
    pub fn dropped_lines(&self) -> usize {
        self.counter.dropped_lines()
    }
}

pub(crate) fn fault(error: &crate::Error) -> sailry_protocol::ErrorCode {
    use crate::Error;
    use sailry_protocol::ErrorCode;
    match error {
        Error::ProfileInUse(_) | Error::Busy => ErrorCode::Busy,
        Error::InvalidProfile { .. } => ErrorCode::InvalidRequest,
        Error::Stopped => ErrorCode::Unavailable,
        Error::ShutdownTimeout => ErrorCode::OutcomeUnknown,
        Error::Link(fault) => fault.code,
        Error::Io(_) | Error::Worker(_) | Error::Database(_) => ErrorCode::Internal,
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let dropped = self.counter.dropped_lines();
        if dropped > 0 {
            let mut entry = Entry::new(Area::Logging, Phase::Failed);
            entry.reason = Some("queue_overflow");
            entry.count = Some(dropped as u64);
            entry.emit();
            // The queue can still be full; do not conceal lost diagnostic records.
            eprintln!("sailry logging dropped at least {dropped} records");
        }
        drop(self.worker.take());
        // WorkerGuard finishes before the preview's temporary directory is removed.
    }
}
