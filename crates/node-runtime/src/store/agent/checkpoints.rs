//! Preserve the exact prepared change before dispatching an approved file mutation.
//! Original tool calls and durable write outcomes keep their existing owners.
use super::*;
use sailry_link::CancellationToken;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;

mod diff;
mod reads;
pub(in crate::store) use diff::read as diff;
pub(in crate::store) mod restore;
#[cfg(test)]
mod tests;
pub(in crate::store) use reads::{list, read};

#[derive(Serialize, Deserialize)]
struct Record {
    worktree: WorktreeId,
    path: String,
    before: Option<checkpoint::Version>,
    after: checkpoint::Version,
    digest: String,
}

impl Ingress {
    pub(crate) async fn checkpoint_write(
        &self,
        turn: TurnId,
        request: &Request,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let Command::WriteFile {
            worktree,
            path,
            expected_revision,
            ..
        } = &request.command
        else {
            return Err(invalid("file checkpoint requires a write"));
        };
        let root = self.worktree_root(*worktree).await?;
        let original = self
            .files
            .inspect(
                root,
                Command::ReadFile {
                    worktree: *worktree,
                    path: path.clone(),
                },
                stop.clone(),
            )
            .await;
        let before = match (original, expected_revision) {
            (Ok(Output::FileContent(content)), Some(expected))
                if !content.truncated && content.revision.as_ref() == Some(expected) =>
            {
                Some(content.text)
            }
            (Err(error), None) if error.code == ErrorCode::NotFound => None,
            (Ok(Output::FileContent(_)), _) => {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "file changed before checkpoint capture",
                ));
            }
            (Err(error), _) => return Err(error),
            _ => return Err(invalid("file checkpoint read returned an invalid response")),
        };
        if stop.is_cancelled() {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "file checkpoint capture cancelled",
            ));
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Checkpoint {
                turn,
                request: request.clone(),
                before,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}

pub(super) fn capture(
    database: &mut Database,
    turn: TurnId,
    request: &Request,
    before: Option<&str>,
) -> Result<(), Fault> {
    let Command::WriteFile {
        worktree,
        path,
        text,
        expected_revision,
    } = &request.command
    else {
        return Err(invalid("file checkpoint requires a write"));
    };
    let run = calls::active(&database.connection, turn)?;
    let session = super::super::commands::session(&database.connection, run.session)?;
    if session.worktree != *worktree {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "file checkpoint belongs to another worktree",
        ));
    }
    let root = database.worktree_root(*worktree)?;
    database.validate_relocation(&[(root, path.clone())])?;
    if text.len() > MAX_FILE_BYTES
        || text.contains('\0')
        || before.is_some_and(|text| text.len() > MAX_FILE_BYTES || text.contains('\0'))
    {
        return Err(invalid("file checkpoint exceeds the text limit"));
    }
    let previous = before.map(version);
    if previous.as_ref().map(|value| &value.revision) != expected_revision.as_ref() {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file checkpoint revision does not match",
        ));
    }
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let body: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT body FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1",
            params![turn.to_string(), request.id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let approval: Approval = serde_json::from_slice(&body.ok_or_else(|| {
        Fault::new(
            ErrorCode::PermissionDenied,
            "file checkpoint has no claimed approval",
        )
    })?)
    .map_err(storage_error)?;
    let event = event(&transaction, &approval)?;
    let valid = match event
        .content()
        .and_then(|content| content.parts.get(approval.index))
    {
        Some(adk_core::Part::FunctionCall { name, .. }) => {
            if let Some(context) = &request.plugin {
                let package = super::plugins::package(&transaction, context)?;
                // The trusted handler may prepare arguments from the exact claimed call.
                // Retain the resulting request digest, with the same frozen package and scope.
                context.turn == Some(turn)
                    && context.session == Some(run.session)
                    && context.worktree == Some(*worktree)
                    && crate::plugins::tools::operation(&package, name)
                        == Some(sailry_protocol::tool::Operation::WriteFile)
            } else {
                false
            }
        }
        _ => false,
    };
    if approval.state != ApprovalState::Approved || !valid {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "file checkpoint differs from the approved write",
        ));
    }
    let record = Record {
        worktree: *worktree,
        path: path.clone(),
        before: previous,
        after: version(text),
        digest: blake3::hash(&encode(request)?).to_hex().to_string(),
    };
    let encoded = encode(&record)?;
    let previous: Option<(Vec<u8>, Option<String>, String)> = transaction
        .query_row(
            "SELECT body,before_text,after_text FROM file_checkpoints WHERE approval=?1",
            [approval.id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some((stored, content, after)) = previous {
        if stored != encoded || content.as_deref() != before || after != *text {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "file checkpoint already has different content",
            ));
        }
    } else {
        transaction.execute(
            "INSERT INTO file_checkpoints(id,turn,approval,body,before_text,after_text) VALUES(?1,?2,?3,?4,?5,?6)",
            params![CheckpointId::new().to_string(), turn.to_string(), approval.id.to_string(), encoded, before, text],
        ).map_err(storage_error)?;
    }
    transaction.commit().map_err(storage_error)
}

fn version(text: &str) -> checkpoint::Version {
    checkpoint::Version {
        revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
        size: text.len() as u64,
    }
}

fn event(db: &Connection, approval: &Approval) -> Result<AdkEvent, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![approval.turn.to_string(), approval.entry],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    serde_json::from_slice(&body).map_err(storage_error)
}
