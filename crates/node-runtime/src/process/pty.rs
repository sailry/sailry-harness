//! Native PTY I/O is shared by managed commands and interactive terminals.
use sailry_protocol::{ErrorCode, Fault};
use std::sync::Arc;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(unix)]
pub(crate) use unix::Io;
#[cfg(windows)]
pub(crate) use windows::Io;

pub(crate) enum Event {
    Bytes(Vec<u8>),
    Eof(Result<(), String>),
    #[cfg(windows)]
    InputError(String),
}

pub(crate) type Output = Arc<dyn Fn(Event) -> bool + Send + Sync>;

fn failure(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Unavailable, format!("PTY I/O failed: {error}"))
}
