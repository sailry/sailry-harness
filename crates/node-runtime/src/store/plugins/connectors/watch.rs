//! Subscribe before reading so package and connection commits cannot be lost.
use super::*;
use crate::store::{Ingress, unavailable};
use sailry_link::{Pending, Subscription};
use sailry_protocol::{EventEnvelope, NodeId, Update, plugin::connector::Changes};
use tokio::sync::broadcast;

pub(in crate::store) async fn snapshot(
    db: &Connection,
    profile: Option<&std::path::Path>,
) -> Result<Changes, Fault> {
    Ok(Changes {
        cursor: {
            let mut statement = db
                .prepare("SELECT coalesce(max(cursor),0) FROM events")
                .await
                .map_err(storage_error)?;
            let row = statement.query_row(()).await.map_err(storage_error)?;
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<i64>(0))(&row);
            mapped.map_err(storage_error)?
        } as u64,
        types: types(db).await?,
        connections: connections(db, profile).await?,
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
    let profile = ingress.profile.clone();
    ingress
        .storage
        .get()
        .ok_or_else(unavailable)?
        .read(move |connection| {
            Box::pin(async move { snapshot(connection, profile.as_deref()).await })
        })
        .await
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
