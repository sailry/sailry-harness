//! Preserve the exact prepared change before dispatching an approved file mutation.
//! Original tool calls and durable write outcomes keep their existing owners.
use super::*;
use sailry_link::CancellationToken;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;
use std::borrow::Cow;

pub(super) mod changes;
mod diff;
pub(in crate::store) mod hunks;
mod reads;
pub(crate) use diff::append as append_diff;
pub(in crate::store) use diff::read as diff;
pub(in crate::store) mod restore;
#[cfg(test)]
mod tests;
pub(in crate::store) mod write;
pub(in crate::store) use reads::{list, read, saved};

#[derive(Serialize, Deserialize)]
struct Record {
    worktree: WorktreeId,
    path: String,
    before: Option<checkpoint::Version>,
    after: checkpoint::Version,
    digest: String,
    /// One file of a multi-file change set; existing records describe a whole call.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    set: bool,
}

pub(super) struct Content<'a> {
    before: Option<&'a str>,
    after: Cow<'a, str>,
    previous: Option<checkpoint::Version>,
    next: checkpoint::Version,
}

fn content<'a>(request: &'a Request, before: Option<&'a str>) -> Result<Content<'a>, Fault> {
    let after = match &request.command {
        Command::WriteFile { text, .. } => Cow::Borrowed(text.as_str()),
        Command::EditFile {
            old_text,
            new_text,
            replace_all,
            edits,
            expected_revision,
            ..
        } => {
            let original = before.ok_or_else(|| {
                Fault::new(
                    ErrorCode::RevisionConflict,
                    "file edit checkpoint has no original content",
                )
            })?;
            let changes = crate::files::edit::changes(
                old_text.as_deref(),
                new_text.as_deref(),
                *replace_all,
                edits.as_deref(),
            )?;
            Cow::Owned(crate::files::edit::prepare(
                original,
                &changes,
                expected_revision,
            )?)
        }
        Command::EditNotebook { edit, .. } => {
            let original = before.ok_or_else(|| {
                Fault::new(
                    ErrorCode::RevisionConflict,
                    "notebook checkpoint has no original content",
                )
            })?;
            Cow::Owned(crate::files::notebook::prepare(original, edit)?)
        }
        _ => return Err(Fault::invalid("file checkpoint requires a write or edit")),
    };
    Ok(Content {
        before,
        previous: before.map(version),
        next: version(&after),
        after,
    })
}

pub(super) async fn capture(
    database: &mut Database,
    turn: TurnId,
    request: &Request,
    content: Content<'_>,
) -> Result<(), Fault> {
    let (worktree, path, expected_revision, operation, limit) = match &request.command {
        Command::WriteFile {
            worktree,
            path,
            expected_revision,
            ..
        } => (
            *worktree,
            path.as_str(),
            expected_revision.as_deref(),
            sailry_protocol::tool::Operation::WriteFile,
            MAX_FILE_BYTES,
        ),
        Command::EditFile {
            worktree,
            path,
            expected_revision,
            ..
        } => (
            *worktree,
            path.as_str(),
            Some(expected_revision.as_str()),
            sailry_protocol::tool::Operation::EditFile,
            MAX_EDIT_BYTES,
        ),
        Command::EditNotebook { worktree, edit } => (
            *worktree,
            edit.path.as_str(),
            Some(edit.expected_revision.as_str()),
            sailry_protocol::tool::Operation::EditNotebook,
            MAX_EDIT_BYTES,
        ),
        _ => return Err(Fault::invalid("file checkpoint requires a write or edit")),
    };
    let Content {
        before,
        after: text,
        previous,
        next,
    } = content;
    let run = calls::active(&database.connection, turn).await?;
    let session = super::super::commands::session(&database.connection, run.session).await?;
    if session.worktree != worktree {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "file checkpoint belongs to another worktree",
        ));
    }
    let root = database.worktree_root(worktree).await?;
    database
        .validate_relocation(&[(root, path.to_owned())])
        .await?;
    if text.len() > limit
        || text.contains('\0')
        || before.is_some_and(|text| text.len() > MAX_EDIT_BYTES || text.contains('\0'))
    {
        return Err(sailry_protocol::Fault::invalid(
            "file checkpoint exceeds the text limit",
        ));
    }
    if previous.as_ref().map(|value| value.revision.as_str()) != expected_revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file checkpoint revision does not match",
        ));
    }
    let transaction = database
        .connection
        .unchecked_transaction()
        .await
        .map_err(storage_error)?;
    let approval = approved(
        &transaction,
        turn,
        request,
        run.session,
        worktree,
        operation,
    )
    .await?;
    let record = Record {
        worktree,
        path: path.to_owned(),
        before: previous,
        after: next,
        digest: blake3::hash(&encode(request)?).to_hex().to_string(),
        set: false,
    };
    let encoded = encode(&record)?;
    let previous: Option<(Vec<u8>, Option<String>, String)> = {
        let mut statement = transaction
            .prepare("SELECT body,before_text,after_text FROM file_checkpoints WHERE approval=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([approval.id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    if let Some((stored, content, after)) = previous {
        if stored != encoded || content.as_deref() != before || after != text.as_ref() {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "file checkpoint already has different content",
            ));
        }
    } else {
        transaction.execute(
            "INSERT INTO file_checkpoints(id,turn,approval,body,before_text,after_text,rowid) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![CheckpointId::new().to_string(), turn.to_string(), approval.id.to_string(), encoded, before, text.as_ref(), crate::store::database::counters::next(&transaction, crate::store::database::counters::Namespace::Checkpoints).await? as i64],
        ).await.map_err(storage_error)?;
    }
    transaction.commit().await.map_err(storage_error)
}

/// The claimed approval must belong to the exact canonical call, frozen package and scope.
async fn approved(
    db: &Connection,
    turn: TurnId,
    request: &Request,
    session: SessionId,
    worktree: WorktreeId,
    operation: sailry_protocol::tool::Operation,
) -> Result<Approval, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1")
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![turn.to_string(), request.id.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let approval: Approval = serde_json::from_slice(&body.ok_or_else(|| {
        Fault::new(
            ErrorCode::PermissionDenied,
            "file checkpoint has no claimed approval",
        )
    })?)
    .map_err(storage_error)?;
    let event = event(db, &approval).await?;
    let valid = match event.message().and_then(|message| {
        message
            .content
            .iter()
            .find(|block| block.index == approval.index)
    }) {
        Some(crate::agent::canonical::Block {
            kind: crate::agent::canonical::BlockKind::ToolRequest(call),
            ..
        }) => {
            if let Some(context) = &request.plugin {
                let package = super::plugins::package(db, request).await?;
                // The trusted handler may prepare arguments from the exact claimed call.
                // Retain the resulting request digest, with the same frozen package and scope.
                context.turn == Some(turn)
                    && context.session == Some(session)
                    && context.worktree == Some(worktree)
                    && crate::plugins::tools::operation(&package, &call.name) == Some(operation)
            } else {
                false
            }
        }
        _ => false,
    };
    if approval.state != ApprovalState::Approved || !valid {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "file checkpoint differs from the approved mutation",
        ));
    }
    Ok(approval)
}

fn version(text: &str) -> checkpoint::Version {
    checkpoint::Version {
        revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
        size: text.len() as u64,
    }
}

async fn event(
    db: &Connection,
    approval: &Approval,
) -> Result<crate::agent::canonical::Record, Fault> {
    let body: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![approval.turn.to_string(), approval.entry.as_str()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    serde_json::from_slice(&body).map_err(storage_error)
}
