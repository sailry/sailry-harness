//! Subscribe before reading so package and connection commits cannot be lost.
use super::*;
use crate::store::{Ingress, Job, unavailable};
use sailry_link::{Pending, Subscription};
use sailry_protocol::{EventEnvelope, NodeId, Update, plugin::connector::Changes};
use tokio::sync::{broadcast, oneshot};

pub(in crate::store) fn snapshot(
    db: &Connection,
    profile: Option<&std::path::Path>,
) -> Result<Changes, Fault> {
    Ok(Changes {
        cursor: db
            .query_row("SELECT coalesce(max(cursor),0) FROM events", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(storage_error)? as u64,
        types: types(db)?,
        connections: connections(db, profile)?,
    })
}

pub(in crate::store) async fn subscribe(
    ingress: Ingress,
    _caller: NodeId,
) -> Result<Box<dyn Subscription>, Fault> {
    let receiver = ingress.events.subscribe();
    let initial = state(&ingress).await?;
    let cursor = initial.cursor;
    Ok(Box::new(Feed {
        ingress,
        receiver,
        initial: Some(initial),
        cursor,
    }))
}

async fn state(ingress: &Ingress) -> Result<Changes, Fault> {
    let (reply, response) = oneshot::channel();
    ingress
        .sender
        .try_send(Job::ConnectorState { reply })
        .map_err(|_| unavailable())?;
    response.await.map_err(|_| unavailable())?
}

struct Feed {
    ingress: Ingress,
    receiver: broadcast::Receiver<EventEnvelope>,
    initial: Option<Changes>,
    cursor: u64,
}

impl Subscription for Feed {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if let Some(initial) = self.initial.take() {
                return Ok(Update::ConnectionsChanged(initial));
            }
            loop {
                tokio::select! {
                    biased;
                    _ = self.ingress.closed.cancelled() => return Err(unavailable()),
                    event = self.receiver.recv() => match event {
                        Ok(event) if event.cursor > self.cursor && matches!(event.event, Event::PluginChanged(_)) => {
                            let changes = state(&self.ingress).await?;
                            self.cursor = changes.cursor;
                            return Ok(Update::ConnectionsChanged(changes));
                        }
                        Ok(_) => {},
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let changes = state(&self.ingress).await?;
                            self.cursor = changes.cursor;
                            return Ok(Update::ConnectionsChanged(changes));
                        }
                        Err(broadcast::error::RecvError::Closed) => return Err(unavailable()),
                    }
                }
            }
        })
    }
}
