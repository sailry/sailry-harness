#[cfg(test)]
mod access {
    use super::*;
    mod tests;
}
pub(crate) mod artifacts;
pub(crate) mod attachments;
pub(crate) mod browser;
pub(crate) mod changes;
pub(crate) mod checkpoints;
pub(crate) mod copy;
mod directory;
pub(crate) mod edit;
pub(crate) mod media;
pub(crate) mod mutations;
pub(crate) mod notebook;
pub(crate) mod patch;
pub(crate) mod rename;
mod search;
pub(crate) mod ssh;
pub(crate) mod transfers;
pub(crate) mod watch;
pub(crate) use edit::edit;
pub(crate) use rename::rename;
pub(crate) use sailry_runtime_core::files::{
    Access, Control, Scope, capture_text, directory, index_text, io_error, open_regular, path,
    remove_verified, restore_text, save, text, trash, walk,
};
use sailry_runtime_core::files::{capture_with, lines, read_bytes, version};

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

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

fn read(root: Scope<'_>, path: String, control: &Control) -> Result<FileContent, Fault> {
    let mut file = path::file(root, &path)?;
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

    #[tokio::test]
    async fn distinguishes_missing_paths() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let worktree = WorktreeId::new();
        let files = Files::new();
        for path in ["missing.ipynb", "missing/file.ipynb"] {
            let commands = [
                Command::ReadFile {
                    worktree,
                    path: path.into(),
                },
                Command::ReadFileLines {
                    worktree,
                    path: path.into(),
                    offset: 0,
                    limit: 1,
                    expected_revision: None,
                },
                Command::ListDirectory {
                    worktree,
                    path: path.into(),
                    after: None,
                },
                Command::ReadNotebook {
                    worktree,
                    options: sailry_protocol::notebook::Read {
                        path: path.into(),
                        offset: 0,
                        limit: 1,
                    },
                },
                Command::ReadOffice {
                    worktree,
                    options: office::Read {
                        source: office::Source::Path(path.into()),
                        offset: 0,
                    },
                },
            ];
            for (directory, expected) in [
                (root.clone(), Some(FaultDetails::WorktreePathNotFound)),
                (root.join("absent-root"), None),
            ] {
                for command in &commands {
                    let error = files
                        .inspect(directory.clone(), command.clone(), CancellationToken::new())
                        .await
                        .unwrap_err();
                    assert_eq!(error.code, ErrorCode::NotFound, "{command:?}: {error:?}");
                    assert_eq!(error.details, expected, "{command:?}: {error:?}");
                }
                let error = files
                    .media_input(directory, path.into(), CancellationToken::new())
                    .await
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::NotFound);
                assert_eq!(error.details, expected);
            }
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
}
