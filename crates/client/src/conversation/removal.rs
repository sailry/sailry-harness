//! Confirm absence against the existing Node observer without replacing its projection.
use super::*;
use crate::watch::View as NodeView;
use sailry_protocol::Snapshot as NodeSnapshot;

fn missing(view: &NodeView, session: SessionId) -> bool {
    view.connected
        && view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.sessions.iter().any(|item| item.id == session))
}

fn confirmed(view: &NodeView, session: SessionId, snapshot: &NodeSnapshot) -> bool {
    missing(view, session)
        && view.snapshot.as_ref().is_some_and(|observed| {
            snapshot.node == observed.node && snapshot.cursor >= observed.cursor
        })
        && !snapshot.sessions.iter().any(|item| item.id == session)
}

pub(super) async fn watch(
    client: &Client,
    session: SessionId,
    updates: watch::Sender<View>,
    stop: CancellationToken,
) -> Result<(), Fault> {
    let (sender, mut receiver) = watch::channel(NodeView::default());
    let observation = client.watch(sender, stop.clone());
    let check = async {
        loop {
            let next = receiver.borrow_and_update().clone();
            if missing(&next, session) {
                let result = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Ok(()),
                    _ = updates.closed() => return Ok(()),
                    result = client.execute(client.prepare(Command::Snapshot)) => result,
                };
                // A newer authoritative observation supersedes this confirmation read.
                if receiver.has_changed().unwrap_or(false) {
                    continue;
                }
                match result {
                    Ok(Output::Snapshot(snapshot)) if snapshot.node == client.target() => {
                        if confirmed(&next, session, &snapshot) {
                            updates.send_modify(|view| {
                                view.removed = true;
                                view.connected = false;
                                view.loading_older = false;
                            });
                            return Ok(());
                        }
                    }
                    Ok(_) => updates.send_modify(|view| {
                        view.error = Some(Fault::new(
                            ErrorCode::WrongTarget,
                            "session snapshot belongs to another Node",
                        ))
                    }),
                    Err(error) => updates.send_modify(|view| view.error = Some(error)),
                }
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                result = receiver.changed() => if result.is_err() { return Ok(()); },
            }
        }
    };
    tokio::select! {
        result = observation => result,
        result = check => result,
    }
}

#[cfg(test)]
mod tests;
