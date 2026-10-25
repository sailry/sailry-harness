use std::time::Duration;

use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Fault, FileLines, Output, Topic, Update, WorktreeId};
use tokio::sync::watch;

use crate::Client;

/// A coalesced invalidation counter; consumers re-read only their loaded resources.
/// Missing intermediate versions is safe because each invalidates the whole view.
#[derive(Clone, Default)]
pub struct FileView {
    pub revision: u64,
    pub connected: bool,
    pub error: Option<Fault>,
}

struct Closing(watch::Sender<FileView>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| view.connected = false);
    }
}

impl Client {
    /// Offsets are zero-based; returned line numbers are one-based.
    /// A supplied full-content revision must match before any source is returned.
    pub async fn read_file_lines(
        &self,
        worktree: WorktreeId,
        path: &str,
        offset: usize,
        limit: usize,
        expected_revision: Option<String>,
    ) -> Result<FileLines, Fault> {
        match self
            .execute(self.prepare(Command::ReadFileLines {
                worktree,
                path: path.into(),
                offset,
                limit,
                expected_revision: expected_revision.clone(),
            }))
            .await?
        {
            Output::FileLines(lines)
                if lines.path == path
                    && lines.offset == offset
                    && lines.lines.len() <= limit
                    && expected_revision
                        .as_ref()
                        .is_none_or(|expected| expected == &lines.revision) =>
            {
                Ok(lines)
            }
            _ => Err(Fault::invalid("invalid file lines response")),
        }
    }

    pub async fn watch_files(
        &self,
        worktree: WorktreeId,
        updates: watch::Sender<FileView>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let _closing = Closing(updates.clone());
        let mut revision = 0u64;
        let mut delay = Duration::from_millis(250);
        loop {
            let stream = tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                stream = self.transport.subscribe(Topic::Files(worktree)) => stream,
            };
            let failure = match stream {
                Ok(mut stream) => loop {
                    let update = tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Ok(()),
                        _ = updates.closed() => return Ok(()),
                        update = stream.next() => update,
                    };
                    match update {
                        Ok(Update::FilesChanged {
                            node,
                            worktree: owner,
                        }) if node == self.target() && owner == worktree => {
                            revision = revision.checked_add(1).ok_or_else(|| {
                                Fault::new(ErrorCode::Internal, "file view revision exhausted")
                            })?;
                            updates.send_replace(FileView {
                                revision,
                                connected: true,
                                error: None,
                            });
                            delay = Duration::from_millis(250);
                        }
                        Ok(_) => {
                            break Fault::invalid("unexpected file subscription update");
                        }
                        Err(error) => break error,
                    }
                },
                Err(error) => error,
            };
            updates.send_replace(FileView {
                revision,
                connected: false,
                error: Some(failure.clone()),
            });
            if !matches!(failure.code, ErrorCode::Unavailable | ErrorCode::Busy) {
                return Err(failure);
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                _ = tokio::time::sleep(delay) => {},
            }
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    }
}
