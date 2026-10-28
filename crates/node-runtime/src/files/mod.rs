mod access;
pub(crate) use access::{Access, Scope};
pub(crate) mod artifacts;
pub(crate) mod attachments;
pub(crate) mod browser;
pub(crate) mod checkpoints;
pub(crate) mod copy;
mod create;
mod directory;
pub(crate) mod edit;
mod lines;
pub(crate) use lines::bounded as index_text;
pub(crate) mod media;
pub(crate) mod mutations;
pub(crate) mod notebook;
pub(crate) mod path;
pub(crate) mod rename;
mod save;
mod search;
pub(crate) mod ssh;
pub(crate) mod text;
pub(crate) mod transfers;
mod trash;
mod version;
pub(crate) mod walk;
pub(crate) mod watch;
pub(crate) use create::directory;
pub(crate) use edit::edit;
pub(crate) use rename::rename;
pub(crate) use save::restore as restore_text;
pub(crate) use save::save;
pub(crate) use trash::remove_verified;
pub(crate) use trash::trash;

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

#[derive(Clone)]
pub(crate) struct Files {
    ocr: Option<Arc<crate::ocr::Service>>,
}

impl Files {
    pub(crate) fn new() -> Self {
        Self { ocr: None }
    }

    pub(crate) fn with_ocr(mut self, ocr: Arc<crate::ocr::Service>) -> Self {
        self.ocr = Some(ocr);
        self
    }

    pub(crate) async fn inspect(
        &self,
        root: PathBuf,
        command: Command,
        closed: CancellationToken,
    ) -> Result<Output, Fault> {
        self.inspect_with(root, command, closed, Access::Restricted)
            .await
    }

    pub(crate) async fn inspect_with(
        &self,
        root: PathBuf,
        command: Command,
        closed: CancellationToken,
        access: Access,
    ) -> Result<Output, Fault> {
        let searching = matches!(
            &command,
            Command::FindFiles { .. } | Command::SearchFiles { .. }
        );
        let budget = Duration::from_secs(if searching { 20 } else { 5 });
        self.inspect_until(root, command, closed, access, Instant::now() + budget)
            .await
    }

    async fn inspect_until(
        &self,
        root: PathBuf,
        command: Command,
        closed: CancellationToken,
        access: Access,
        deadline: Instant,
    ) -> Result<Output, Fault> {
        let cancelled = closed.child_token();
        let ocr = self.ocr.clone();
        let _guard = cancelled.clone().drop_guard();
        let searching = matches!(
            &command,
            Command::FindFiles { .. } | Command::SearchFiles { .. }
        );
        let worker = tokio::task::spawn_blocking(move || {
            let control = Control {
                cancelled,
                deadline,
            };
            control.check()?;
            let scope = access.scope(&root);
            match command {
                Command::BrowseFiles { directory, after } => {
                    browser::list(directory.as_deref(), after.as_ref(), &control)
                        .map(Output::FileListing)
                }
                Command::ListDirectory { path, after, .. } => {
                    directory::list(scope, path, after.as_ref(), &control).map(Output::Directory)
                }
                Command::ReadOffice { options, .. } => {
                    crate::office::read_with(scope, &options, ocr.as_deref(), &control.cancelled)
                        .map(Output::OfficeContent)
                }
                Command::ReadNotebook { options, .. } => {
                    notebook::read(scope, &options, &control).map(Output::NotebookPage)
                }
                Command::ReadFile { path, .. } => {
                    read(scope, path, &control).map(Output::FileContent)
                }
                Command::ReadFileLines {
                    path,
                    offset,
                    limit,
                    expected_revision,
                    ..
                } => lines::read_version(
                    scope,
                    path,
                    offset,
                    limit,
                    expected_revision.as_deref(),
                    &control,
                )
                .map(Output::FileLines),
                Command::FindFiles {
                    globs,
                    path,
                    limit,
                    offset,
                    ..
                } => {
                    walk::find(scope, &globs, &path, limit, offset, &control).map(Output::FileNames)
                }
                Command::SearchFiles { options, .. } => {
                    search::search(scope, &options, &control).map(Output::SearchResults)
                }
                _ => Err(Fault::new(
                    ErrorCode::Internal,
                    "invalid file inspection command",
                )),
            }
        });
        tokio::select! {
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Unavailable, "Node service is unavailable")),
            // Search workers return their actual partial results when the shared
            // deadline expires; the outer guard only covers an unresponsive worker.
            result = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()) + Duration::from_secs(u64::from(searching)), worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "file read deadline exceeded"))?
                .map_err(|_| Fault::new(ErrorCode::Internal, "file worker failed"))?,
        }
    }

    pub(crate) async fn capture(
        &self,
        root: PathBuf,
        path: String,
        expected: String,
        closed: CancellationToken,
    ) -> Result<String, Fault> {
        let cancelled = closed.child_token();
        let _guard = cancelled.clone().drop_guard();
        let worker = tokio::task::spawn_blocking(move || {
            let control = Control {
                cancelled,
                deadline: Instant::now() + Duration::from_secs(5),
            };
            capture_with(&root, &path, &expected, &control)
        });
        tokio::select! {
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Unavailable, "Node service is unavailable")),
            result = tokio::time::timeout(Duration::from_secs(5), worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "file read deadline exceeded"))?
                .map_err(|_| Fault::new(ErrorCode::Internal, "file worker failed"))?,
        }
    }

    pub(crate) async fn inspect_document(
        &self,
        name: String,
        bytes: Vec<u8>,
        offset: usize,
        closed: CancellationToken,
    ) -> Result<office::Inspection, Fault> {
        let cancelled = closed.child_token();
        let _guard = cancelled.clone().drop_guard();
        let ocr = self.ocr.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let control = Control::new(cancelled);
            control.check()?;
            let result =
                crate::office::inspect(&name, &bytes, offset, ocr.as_deref(), &control.cancelled);
            control.check()?;
            result
        });
        tokio::select! {
            _ = closed.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "document inspection was cancelled")),
            result = tokio::time::timeout(Duration::from_secs(5), worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "file read deadline exceeded"))?
                .map_err(|_| Fault::new(ErrorCode::Internal, "file worker failed"))?,
        }
    }
}

pub(crate) fn capture_text(
    root: &std::path::Path,
    path: &str,
    expected: &str,
) -> Result<String, Fault> {
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

fn capture_with(
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

pub(crate) struct Control {
    cancelled: CancellationToken,
    deadline: Instant,
}

impl Control {
    pub(crate) fn new(cancelled: CancellationToken) -> Self {
        Self::until(cancelled, Instant::now() + Duration::from_secs(5))
    }
    pub(crate) fn until(cancelled: CancellationToken, deadline: Instant) -> Self {
        Self {
            cancelled,
            deadline,
        }
    }
    pub(crate) fn check(&self) -> Result<(), Fault> {
        if self.cancelled.is_cancelled() {
            Err(Fault::new(ErrorCode::Cancelled, "file read cancelled"))
        } else if Instant::now() >= self.deadline {
            Err(Fault::new(ErrorCode::Busy, "file read deadline exceeded"))
        } else {
            Ok(())
        }
    }
}

fn read(root: Scope<'_>, path: String, control: &Control) -> Result<FileContent, Fault> {
    let parts = root.components(&path, false)?;
    let dir = path::descend(path::root(root.path())?, &parts[..parts.len() - 1])?;
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
            return Err(Fault::invalid("file is not UTF-8 text"));
        }
    };
    if text.contains('\0') {
        return Err(Fault::invalid("binary file cannot be displayed as text"));
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
    use futures::{FutureExt, StreamExt, stream::FuturesUnordered};

    fn read_command() -> Command {
        Command::ReadFile {
            worktree: WorktreeId::new(),
            path: "AGENTS.md".into(),
        }
    }

    #[test]
    fn admits_independent_reads() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let files = Files::new();
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().canonicalize().unwrap();
            std::fs::write(root.join("AGENTS.md"), "Shared content").unwrap();
            let (release, blocked) = std::sync::mpsc::channel();
            let (entered, ready) = tokio::sync::oneshot::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                entered.send(()).unwrap();
                let _ = blocked.recv();
            });
            ready.await.unwrap();
            let mut reads = (0..8)
                .map(|_| files.inspect(root.clone(), read_command(), CancellationToken::new()))
                .collect::<FuturesUnordered<_>>();
            // All workers remain pending behind the runtime's blocking worker.
            // Independent file requests must still be admitted together.
            assert!(reads.next().now_or_never().is_none());
            release.send(()).unwrap();
            while let Some(result) = reads.next().await {
                let Output::FileContent(content) = result.unwrap() else {
                    panic!("file content expected");
                };
                assert_eq!(content.text, "Shared content");
            }
            blocker.await.unwrap();
        });
    }

    #[tokio::test]
    async fn rejects_closed_reads() {
        let closed = CancellationToken::new();
        closed.cancel();
        assert!(
            Files::new()
                .inspect(PathBuf::new(), read_command(), closed)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn expires_pending_reads() {
        let error = Files::new()
            .inspect_until(
                PathBuf::new(),
                read_command(),
                CancellationToken::new(),
                Access::Restricted,
                Instant::now() - Duration::from_secs(1),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Busy);
        assert_eq!(error.message, "file read deadline exceeded");
    }

    #[test]
    fn cancels_read_control() {
        let stop = CancellationToken::new();
        let control = Control::new(stop.clone());
        stop.cancel();
        assert_eq!(control.check().unwrap_err().code, ErrorCode::Cancelled);
    }
}
