pub(crate) mod attachments;
pub(crate) mod browser;
pub(crate) mod checkpoints;
pub(crate) mod copy;
mod create;
mod directory;
mod media;
pub(crate) mod path;
pub(crate) mod rename;
mod save;
mod search;
pub(crate) mod ssh;
pub(crate) mod transfers;
mod trash;
mod version;
pub(crate) mod watch;
pub(crate) use create::create_directory;
pub(crate) use rename::rename;
pub(crate) use save::{save, save_bytes};
pub(crate) use trash::trash;
pub(crate) use trash::trash_file;

use std::{
    io::Read,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::OpenOptions;
use sailry_link::CancellationToken;
use sailry_protocol::*;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(crate) struct Files {
    capacity: Arc<Semaphore>,
}

impl Files {
    pub(crate) fn new() -> Self {
        Self {
            capacity: Arc::new(Semaphore::new(4)),
        }
    }

    pub(crate) async fn inspect(
        &self,
        root: PathBuf,
        command: Command,
        closed: CancellationToken,
    ) -> Result<Output, Fault> {
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| Fault::new(ErrorCode::Busy, "file read capacity exhausted"))?;
        let cancelled = closed.child_token();
        let _guard = cancelled.clone().drop_guard();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let control = Control {
                cancelled,
                deadline: Instant::now() + Duration::from_secs(5),
            };
            control.check()?;
            match command {
                Command::BrowseFiles { directory, after } => {
                    browser::list(directory.as_deref(), after.as_ref(), &control)
                        .map(Output::FileListing)
                }
                Command::ListDirectory { path, after, .. } => {
                    directory::list(&root, path, after.as_ref(), &control).map(Output::Directory)
                }
                Command::OfficeRuntime { .. } => {
                    crate::office::environment().map(Output::OfficeRuntime)
                }
                Command::ReadOffice { options, .. } => {
                    crate::office::read(&root, &options).map(Output::OfficeContent)
                }
                Command::ReadFile { path, .. } => {
                    read(root, path, &control).map(Output::FileContent)
                }
                Command::SearchFiles { options, .. } => {
                    search::search(&root, &options, &control).map(Output::SearchResults)
                }
                _ => Err(Fault::new(
                    ErrorCode::Internal,
                    "invalid file inspection command",
                )),
            }
        });
        tokio::select! {
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Unavailable, "Node service is unavailable")),
            result = tokio::time::timeout(Duration::from_secs(5), worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "file read deadline exceeded"))?
                .map_err(|_| Fault::new(ErrorCode::Internal, "file worker failed"))?,
        }
    }
}

struct Control {
    cancelled: CancellationToken,
    deadline: Instant,
}

impl Control {
    fn check(&self) -> Result<(), Fault> {
        if self.cancelled.is_cancelled() {
            Err(Fault::new(ErrorCode::Cancelled, "file read cancelled"))
        } else if Instant::now() >= self.deadline {
            Err(Fault::new(ErrorCode::Busy, "file read deadline exceeded"))
        } else {
            Ok(())
        }
    }
}

fn read(root: PathBuf, path: String, control: &Control) -> Result<FileContent, Fault> {
    let parts = path::components(&path, false)?;
    let dir = path::descend(path::root(&root)?, &parts[..parts.len() - 1])?;
    let name = parts[parts.len() - 1];
    let mut file = open_regular(&dir, name)?;
    let metadata = file.metadata().map_err(io_error)?;
    let bytes = read_bytes(&mut file, MAX_FILE_BYTES + 1, control)?;
    let truncated = bytes.len() > MAX_FILE_BYTES;
    let revision = (!truncated).then(|| blake3::hash(&bytes).to_hex().to_string());
    let mut bytes = bytes;
    bytes.truncate(MAX_FILE_BYTES);
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) if truncated && error.utf8_error().error_len().is_none() => {
            let valid = error.utf8_error().valid_up_to();
            String::from_utf8(error.into_bytes()[..valid].to_vec()).expect("validated UTF-8 prefix")
        }
        Err(_) => {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "file is not UTF-8 text",
            ));
        }
    };
    if text.contains('\0') {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "binary file cannot be displayed as text",
        ));
    }
    Ok(FileContent {
        path,
        text,
        size: metadata.len(),
        truncated,
        revision,
    })
}

pub(crate) fn open_regular(dir: &cap_std::fs::Dir, name: &str) -> Result<cap_std::fs::File, Fault> {
    let metadata = dir.symlink_metadata(name).map_err(io_error)?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "only regular files can be read",
        ));
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
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "only regular files can be read",
        ));
    }
    Ok(file)
}

fn read_bytes(file: &mut impl Read, limit: usize, control: &Control) -> Result<Vec<u8>, Fault> {
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

pub(crate) fn io_error(error: std::io::Error) -> Fault {
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

    #[tokio::test]
    async fn rejects_unavailable_reads() {
        let files = Files::new();
        let permit = files.capacity.clone().acquire_many_owned(4).await.unwrap();
        let command = Command::ListDirectory {
            worktree: WorktreeId::new(),
            path: String::new(),
            after: None,
        };
        assert_eq!(
            files
                .inspect(PathBuf::new(), command.clone(), CancellationToken::new())
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        drop(permit);
        let closed = CancellationToken::new();
        closed.cancel();
        assert!(
            files
                .inspect(PathBuf::new(), command, closed)
                .await
                .is_err()
        );
    }
}
