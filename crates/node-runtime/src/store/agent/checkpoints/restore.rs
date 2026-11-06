//! Revalidate queued restores on the authoritative store before a physical effect.
use super::*;
use std::path::Path;
use tokio::sync::mpsc;

pub(in crate::store) async fn prepare(
    database: &Database,
    session: SessionId,
    id: CheckpointId,
    worktree: WorktreeId,
    root: &Path,
) -> Result<checkpoint::Content, Fault> {
    let content = reads::content(&database.connection, session, id).await?;
    let file = &content.file;
    if file.worktree != worktree || database.worktree_root(worktree).await? != root {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "file checkpoint worktree changed",
        ));
    }
    let run = runs::get(&database.connection, file.turn).await?;
    if matches!(
        run.status,
        Status::Queued | Status::Running | Status::Stopping
    ) {
        return Err(Fault::new(
            ErrorCode::Busy,
            "file checkpoint turn is still active",
        ));
    }
    match file.receipt() {
        checkpoint::Receipt::Written | checkpoint::Receipt::Uncertain => {}
        checkpoint::Receipt::Pending => {
            return Err(Fault::new(
                ErrorCode::Busy,
                "checkpoint write is still pending",
            ));
        }
        checkpoint::Receipt::Absent if file.outcome == RequestOutcome::NotAdmitted => {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "checkpoint write was not admitted",
            ));
        }
        checkpoint::Receipt::Absent => {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "checkpoint write did not complete",
            ));
        }
    }
    database
        .validate_relocation(&[(root.to_owned(), file.path.clone())])
        .await?;
    Ok(content)
}

pub(in crate::store) fn execute(
    sender: &mpsc::Sender<Job>,
    root: &Path,
    command: &Command,
) -> Result<Output, Fault> {
    let Command::RestoreFileCheckpoint {
        session,
        checkpoint,
        worktree,
    } = command
    else {
        return Err(sailry_protocol::Fault::invalid(
            "file checkpoint restore command expected",
        ));
    };
    let (reply, response) = std::sync::mpsc::channel();
    sender
        .blocking_send(Job::RestoreCheckpoint {
            session: *session,
            checkpoint: *checkpoint,
            worktree: *worktree,
            root: root.to_owned(),
            reply,
        })
        .map_err(|_| crate::store::unavailable())?;
    let content = response.recv().map_err(|_| crate::store::unavailable())??;
    crate::files::checkpoints::restore(root, &content).map(Output::FileRestored)
}
