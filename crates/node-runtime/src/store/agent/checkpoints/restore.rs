//! Revalidate queued restores on the authoritative store before a physical effect.
use super::*;
use std::path::Path;
use tokio::sync::mpsc;

pub(in crate::store) fn prepare(
    database: &Database,
    session: SessionId,
    id: CheckpointId,
    worktree: WorktreeId,
    root: &Path,
) -> Result<checkpoint::Content, Fault> {
    let content = read(&database.connection, session, id)?;
    let file = &content.file;
    if file.worktree != worktree || database.worktree_root(worktree)? != root {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "file checkpoint worktree changed",
        ));
    }
    let run = runs::get(&database.connection, file.turn)?;
    if matches!(
        run.status,
        Status::Queued | Status::Running | Status::Stopping
    ) {
        return Err(Fault::new(
            ErrorCode::Busy,
            "file checkpoint turn is still active",
        ));
    }
    match &file.outcome {
        RequestOutcome::Unknown => {}
        RequestOutcome::Completed(result) => match result.as_ref() {
            Ok(Output::FileWritten(written))
                if written.path == file.path
                    && written.revision == file.after.revision
                    && written.size == file.after.size => {}
            Err(error) if error.code == ErrorCode::OutcomeUnknown => {}
            _ => {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "checkpoint write did not complete",
                ));
            }
        },
        RequestOutcome::Admitted => {
            return Err(Fault::new(
                ErrorCode::Busy,
                "checkpoint write is still pending",
            ));
        }
        RequestOutcome::NotAdmitted => {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "checkpoint write was not admitted",
            ));
        }
    }
    database.validate_relocation(&[(root.to_owned(), file.path.clone())])?;
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
        return Err(invalid("file checkpoint restore command expected"));
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
