//! Bulk visibility changes use the same command transaction and lifecycle rules.
use super::*;
use std::collections::BTreeSet;

pub(in crate::store) async fn execute(
    db: &Connection,
    request: &Request,
) -> Result<(Output, Vec<Event>), Fault> {
    let (expected, removing) = match &request.command {
        Command::ArchiveSessions { expected } => (expected, false),
        Command::RemoveSessions { expected } => (expected, true),
        _ => return Err(Fault::invalid("session batch command expected")),
    };
    if request.plugin.is_some() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "session batch requires a controller",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut sessions = Vec::with_capacity(expected.len());
    for expected in expected {
        if !ids.insert(expected.session) {
            return Err(Fault::invalid("session batch contains duplicate sessions"));
        }
        let session = commands::session(db, expected.session).await?;
        writable(&session)?;
        commands::check_revision(session.revision, expected.revision)?;
        sessions.push(session);
    }
    let mut events = Vec::new();
    let mut archived = Vec::new();
    let mut removed = Vec::new();
    for session in sessions {
        if removing {
            let (output, event) = remove(db, session.id, session.revision).await?;
            let Output::SessionsRemoved(ids) = output else {
                unreachable!("session removal returns its owned sessions")
            };
            removed.extend(ids);
            events.extend(event);
        } else if session.archived {
            archived.push(session);
        } else {
            let (output, event) = archive(db, session.id, session.revision, true).await?;
            let Output::Session(session) = output else {
                unreachable!("session archival returns its revision")
            };
            archived.push(session);
            events.extend(event);
        }
    }
    Ok((
        if removing {
            Output::SessionsRemoved(removed)
        } else {
            Output::SessionsArchived(archived)
        },
        events,
    ))
}
