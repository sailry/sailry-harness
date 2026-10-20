//! Confirm an absent session through the existing authoritative Node snapshot read.
use super::*;
use sailry_protocol::{ErrorCode, Fault, Snapshot};

pub(super) fn missing(view: &NodeView, session: SessionId) -> bool {
    view.connected
        && view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.sessions.iter().any(|item| item.id == session))
}

pub(super) fn removed(
    view: &NodeView,
    confirmed: Option<SessionId>,
    current: Option<SessionId>,
) -> Option<SessionId> {
    confirmed.filter(|session| current == Some(*session) && missing(view, *session))
}

pub(super) async fn read(client: &Client) -> Result<Snapshot, Fault> {
    match client.execute(client.prepare(Command::Snapshot)).await? {
        Output::Snapshot(snapshot) if snapshot.node == client.target() => Ok(snapshot),
        _ => Err(Fault::new(
            ErrorCode::WrongTarget,
            "session snapshot belongs to another Node",
        )),
    }
}

pub(super) fn confirm(
    next: &mut NodeView,
    node: NodeId,
    session: SessionId,
    result: Result<Snapshot, Fault>,
) -> bool {
    match result {
        Ok(snapshot) if snapshot.node == node => {
            // A restored Node is reconciled by Client's existing subscription recovery.
            next.snapshot
                .as_ref()
                .is_some_and(|observed| snapshot.cursor >= observed.cursor)
                && !snapshot.sessions.iter().any(|item| item.id == session)
        }
        Ok(_) => {
            next.error = Some(Fault::new(
                ErrorCode::WrongTarget,
                "session snapshot belongs to another Node",
            ));
            false
        }
        Err(error) => {
            next.error = Some(error);
            false
        }
    }
}

#[cfg(test)]
mod tests;
