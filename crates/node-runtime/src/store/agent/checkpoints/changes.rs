//! A change set saves one checkpoint per content entry under its single approval.
//! The first file uses the approval's ordinary checkpoint row; later files are
//! entries of that row. Moves and deletions keep their single-file behavior and
//! have no checkpoint: the move itself or the Node's Trash recovers them.
use super::*;
use sailry_protocol::FileChange;

impl Ingress {
    /// Node resource checks run before approval, so each protected path names its entry.
    pub(crate) async fn validate_paths(
        &self,
        paths: Vec<(std::path::PathBuf, String)>,
    ) -> Result<Vec<Option<Fault>>, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::ValidatePaths {
            paths,
            reply,
        })))
        .await
        .map_err(|_| crate::store::unavailable())?;
        response.await.map_err(|_| crate::store::unavailable())
    }

    pub(crate) async fn checkpoint_changes(
        &self,
        turn: TurnId,
        request: &Request,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let Command::ApplyChanges { worktree, changes } = &request.command else {
            return Err(Fault::invalid("change checkpoints require a change set"));
        };
        let root = self.worktree_root(*worktree).await?;
        let mut before = Vec::with_capacity(changes.len());
        for change in changes {
            before.push(match change {
                FileChange::Write {
                    path,
                    expected_revision,
                    ..
                }
                | FileChange::Edit {
                    path,
                    expected_revision,
                    ..
                } => Some(
                    self.files
                        .capture(
                            root.clone(),
                            path.clone(),
                            expected_revision.clone(),
                            stop.clone(),
                        )
                        .await?,
                ),
                _ => None,
            });
        }
        if stop.is_cancelled() {
            return Err(Fault::new(
                ErrorCode::Cancelled,
                "file checkpoint capture cancelled",
            ));
        }
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::ChangeCheckpoints {
            turn,
            request: request.clone(),
            before,
            reply,
        })))
        .await
        .map_err(|_| crate::store::unavailable())?;
        response.await.map_err(|_| crate::store::unavailable())?
    }
}

struct Prepared<'a> {
    path: &'a str,
    before: Option<&'a str>,
    after: Cow<'a, str>,
    previous: Option<checkpoint::Version>,
}

pub(in crate::store) async fn capture(
    database: &mut Database,
    turn: TurnId,
    request: &Request,
    before: &[Option<String>],
) -> Result<(), Fault> {
    let Command::ApplyChanges { worktree, changes } = &request.command else {
        return Err(Fault::invalid("change checkpoints require a change set"));
    };
    crate::files::changes::shape(changes)?;
    if before.len() != changes.len() {
        return Err(Fault::invalid(
            "change checkpoint content does not match its entries",
        ));
    }
    let mut files = Vec::new();
    for (change, before) in changes.iter().zip(before) {
        let before = before.as_deref();
        let (path, after, expected, limit) = match change {
            FileChange::Create { path, text } => {
                (path, Cow::Borrowed(text.as_str()), None, MAX_FILE_BYTES)
            }
            FileChange::Write {
                path,
                text,
                expected_revision,
            } => (
                path,
                Cow::Borrowed(text.as_str()),
                Some(expected_revision.as_str()),
                MAX_FILE_BYTES,
            ),
            FileChange::Edit {
                path,
                expected_revision,
                edits,
            } => {
                let original = before.ok_or_else(|| {
                    Fault::new(
                        ErrorCode::RevisionConflict,
                        "file edit checkpoint has no original content",
                    )
                })?;
                (
                    path,
                    Cow::Owned(crate::files::edit::prepare(
                        original,
                        edits,
                        expected_revision,
                    )?),
                    Some(expected_revision.as_str()),
                    MAX_EDIT_BYTES,
                )
            }
            FileChange::Move { .. } | FileChange::Delete { .. } => {
                if before.is_some() {
                    return Err(Fault::invalid(
                        "moves and deletions have no checkpoint content",
                    ));
                }
                continue;
            }
        };
        if after.len() > limit
            || after.contains('\0')
            || before.is_some_and(|text| text.len() > limit || text.contains('\0'))
        {
            return Err(Fault::invalid("file checkpoint exceeds the text limit"));
        }
        let previous = before.map(version);
        if previous.as_ref().map(|value| value.revision.as_str()) != expected {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "file checkpoint revision does not match",
            ));
        }
        files.push(Prepared {
            path,
            before,
            after,
            previous,
        });
    }
    if files.is_empty() {
        return Ok(());
    }
    let run = calls::active(&database.connection, turn).await?;
    let session = super::super::super::commands::session(&database.connection, run.session).await?;
    if session.worktree != *worktree {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "file checkpoint belongs to another worktree",
        ));
    }
    let root = database.worktree_root(*worktree).await?;
    let paths: Vec<_> = changes
        .iter()
        .flat_map(|change| std::iter::once(change.path()).chain(change.destination()))
        .map(|path| (root.clone(), path.to_owned()))
        .collect();
    database.validate_relocation(&paths).await?;
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
        *worktree,
        sailry_protocol::tool::Operation::ApplyChanges,
    )
    .await?;
    let digest = blake3::hash(&encode(request)?).to_hex().to_string();
    let records = files
        .iter()
        .map(|file| {
            encode(&Record {
                worktree: *worktree,
                path: file.path.to_owned(),
                before: file.previous.clone(),
                after: version(&file.after),
                digest: digest.clone(),
                set: true,
            })
        })
        .collect::<Result<Vec<_>, Fault>>()?;
    let stored = stored(&transaction, approval.id).await?;
    if stored.is_empty() {
        let id = CheckpointId::new();
        let rowid = crate::store::database::counters::next(
            &transaction,
            crate::store::database::counters::Namespace::Checkpoints,
        )
        .await? as i64;
        transaction.execute(
            "INSERT INTO file_checkpoints(id,turn,approval,body,before_text,after_text,rowid) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![id.to_string(), turn.to_string(), approval.id.to_string(), records[0].clone(), files[0].before, &*files[0].after, rowid],
        ).await.map_err(storage_error)?;
        for (position, (file, record)) in files.iter().zip(&records).enumerate().skip(1) {
            transaction.execute(
                "INSERT INTO file_checkpoint_entries(id,checkpoint,position,body,before_text,after_text) VALUES(?1,?2,?3,?4,?5,?6)",
                params![CheckpointId::new().to_string(), id.to_string(), position as i64, record.clone(), file.before, &*file.after],
            ).await.map_err(storage_error)?;
        }
    } else if stored.len() != files.len()
        || stored.iter().zip(files.iter().zip(&records)).any(
            |((body, before, after), (file, record))| {
                body != record || before.as_deref() != file.before || after.as_str() != &*file.after
            },
        )
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "file checkpoint already has different content",
        ));
    }
    transaction.commit().await.map_err(storage_error)
}

/// Every saved file of one approval, in change-set order.
async fn stored(
    db: &Connection,
    approval: ApprovalId,
) -> Result<Vec<(Vec<u8>, Option<String>, String)>, Fault> {
    let first: Option<(String, Vec<u8>, Option<String>, String)> = {
        let mut statement = db
            .prepare(
                "SELECT id,body,before_text,after_text FROM file_checkpoints WHERE approval=?1",
            )
            .await
            .map_err(storage_error)?;
        match statement.query_row([approval.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))(
                        &row,
                    );
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let Some((id, body, before, after)) = first else {
        return Ok(Vec::new());
    };
    let mut files = vec![(body, before, after)];
    let mut statement = db
        .prepare("SELECT body,before_text,after_text FROM file_checkpoint_entries WHERE checkpoint=?1 ORDER BY position")
        .await
        .map_err(storage_error)?;
    let mut rows = statement.query([id]).await.map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let mapped: turso::Result<_> =
            (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
        files.push(mapped.map_err(storage_error)?);
    }
    Ok(files)
}
