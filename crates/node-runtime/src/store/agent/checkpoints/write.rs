use super::*;
use crate::files::{Control, Scope, edit, path};
use sailry_runtime_core::files::lines;
use tokio::sync::mpsc;

pub(in crate::store) enum Prepared {
    Edit(Box<edit::Prepared>),
    Original(Option<String>),
}

impl Prepared {
    pub(in crate::store::agent) fn content<'a>(
        &'a self,
        request: &'a Request,
    ) -> Result<Content<'a>, Fault> {
        match self {
            Self::Edit(prepared) => {
                let Command::EditFile { .. } = &request.command else {
                    return Err(Fault::invalid("prepared edit requires an edit command"));
                };
                Ok(Content {
                    before: Some(&prepared.before),
                    after: Cow::Borrowed(&prepared.after),
                    previous: Some(checkpoint::Version {
                        revision: prepared.previous.clone(),
                        size: prepared.before.len() as u64,
                    }),
                    next: checkpoint::Version {
                        revision: prepared.revision.clone(),
                        size: prepared.after.len() as u64,
                    },
                })
            }
            Self::Original(before) => content(request, before.as_deref()),
        }
    }
}

pub(in crate::store) fn prepare(
    validation: &mpsc::Sender<Job>,
    scope: Scope<'_>,
    request: &Request,
) -> Result<Option<Box<edit::Prepared>>, Fault> {
    let Some(turn) = request.plugin.as_ref().and_then(|context| context.turn) else {
        return Ok(None);
    };
    let control = Control::new(sailry_link::CancellationToken::new());
    let prepared = match &request.command {
        Command::EditFile {
            path,
            old_text,
            new_text,
            replace_all,
            edits,
            expected_revision,
            ..
        } => {
            let changes = edit::changes(
                old_text.as_deref(),
                new_text.as_deref(),
                *replace_all,
                edits.as_deref(),
            )?;
            Prepared::Edit(Box::new(edit::read(
                scope,
                path,
                &changes,
                expected_revision,
            )?))
        }
        Command::WriteFile {
            path: relative,
            expected_revision,
            ..
        } => {
            let before = if expected_revision.is_some() {
                Some(lines::complete(scope, relative, &control)?)
            } else {
                match path::file(scope, relative) {
                    Err(error) if error.code == ErrorCode::NotFound => None,
                    Err(error) => return Err(error),
                    Ok(_) => {
                        return Err(Fault::new(
                            ErrorCode::RevisionConflict,
                            "file changed before checkpoint capture",
                        ));
                    }
                }
            };
            Prepared::Original(before)
        }
        Command::EditNotebook { edit, .. } => {
            Prepared::Original(Some(lines::complete(scope, &edit.path, &control)?))
        }
        _ => return Ok(None),
    };
    let (reply, response) = oneshot::channel();
    validation
        .blocking_send(Job::Agent(Box::new(Operation::Checkpoint {
            turn,
            request: request.clone(),
            prepared,
            reply,
        })))
        .map_err(|_| crate::store::unavailable())?;
    match response
        .blocking_recv()
        .map_err(|_| crate::store::unavailable())??
    {
        Prepared::Edit(prepared) => Ok(Some(prepared)),
        Prepared::Original(_) => Ok(None),
    }
}
