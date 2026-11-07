//! Confined worktree file primitives shared by Node services.
mod access;
pub use access::{Access, Scope};
mod create;
pub mod lines;
pub use lines::bounded_at as index_text;
pub mod path;
pub mod save;
pub mod text;
pub mod trash;
pub mod version;
pub mod walk;
pub use create::directory;
pub use save::restore as restore_text;
pub use save::save;
pub use trash::remove_verified;
pub use trash::trash;

use std::{
    io::Read,
    time::{Duration, Instant},
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
use sailry_protocol::{ErrorCode, Fault};
use tokio_util::sync::CancellationToken;

pub fn capture_text(root: &std::path::Path, path: &str, expected: &str) -> Result<String, Fault> {
    capture_with(
        root,
        path,
        expected,
        &Control {
            cancelled: CancellationToken::new(),
            deadline: Instant::now() + Duration::from_secs(5),
        },
    )
}

pub fn capture_with(
    root: &std::path::Path,
    path: &str,
    expected: &str,
    control: &Control,
) -> Result<String, Fault> {
    let text = lines::complete(root, path, control)?;
    if blake3::hash(text.as_bytes()).to_hex().as_str() != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed before checkpoint capture",
        ));
    }
    control.check()?;
    Ok(text)
}

pub struct Control {
    pub cancelled: CancellationToken,
    pub deadline: Instant,
}

impl Control {
    pub fn new(cancelled: CancellationToken) -> Self {
        Self::until(cancelled, Instant::now() + Duration::from_secs(5))
    }
    pub fn until(cancelled: CancellationToken, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }
    pub fn check(&self) -> Result<(), Fault> {
        if self.cancelled.is_cancelled() {
            Err(Fault::new(ErrorCode::Cancelled, "file read cancelled"))
        } else if Instant::now() >= self.deadline {
            Err(Fault::new(ErrorCode::Busy, "file read deadline exceeded"))
        } else {
            Ok(())
        }
    }
}

pub fn open_regular(dir: &cap_std::fs::Dir, name: &str) -> Result<cap_std::fs::File, Fault> {
    let metadata = dir.symlink_metadata(name).map_err(io_error)?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(Fault::invalid("only regular files can be read"));
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    // A racing replacement by a FIFO must not occupy a worker indefinitely.
    #[cfg(unix)]
    cap_std::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = dir.open_with(name, &options).map_err(io_error)?;
    path::identity::file(&file)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(Fault::invalid("only regular files can be read"));
    }
    Ok(file)
}

pub fn read_bytes(file: &mut impl Read, limit: usize, control: &Control) -> Result<Vec<u8>, Fault> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    while bytes.len() < limit {
        control.check()?;
        let capacity = buffer.len().min(limit - bytes.len());
        let size = file.read(&mut buffer[..capacity]).map_err(io_error)?;
        if size == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..size]);
    }
    control.check()?;
    Ok(bytes)
}

pub fn io_error(error: std::io::Error) -> Fault {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::NotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        _ => ErrorCode::InvalidRequest,
    };
    Fault::new(
        code,
        format!("worktree filesystem operation failed: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancels_read_control() {
        let stop = CancellationToken::new();
        let control = Control::new(stop.clone());
        stop.cancel();
        assert_eq!(control.check().unwrap_err().code, ErrorCode::Cancelled);
    }
}
