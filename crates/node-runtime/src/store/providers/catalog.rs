use super::super::{
    Ingress, Job,
    database::{Database, encode, storage_error},
    unavailable,
};
use crate::providers::catalog::{Download, metadata};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_link::Admission;
use sailry_protocol::{conversation::catalog, *};
use tokio::sync::{broadcast, oneshot};
mod lookup;

pub(crate) fn status(db: &Connection) -> Result<catalog::Status, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM model_catalog_status WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn page(db: &Connection, query: &catalog::Query) -> Result<catalog::Page, Fault> {
    if let Some(provider) = &query.provider {
        metadata::identifier(provider, 128)?;
    }
    if query.ids.len() > 100 || query.provider.is_none() && query.ids.is_empty() {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "catalog lookup requires bounded model identifiers",
        ));
    }
    for id in &query.ids {
        metadata::identifier(id, 256)?;
    }
    if query.limit == 0 || query.limit > 100 || (query.after.is_some() && query.revision.is_none())
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "catalog page requires bounded size and revision",
        ));
    }
    if let Some(after) = &query.after {
        metadata::identifier(after, 256)?;
    }
    let status = status(db)?;
    if status.revision == 0 {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "model catalog has not been refreshed",
        ));
    }
    if query
        .revision
        .is_some_and(|revision| revision != status.revision)
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "model catalog revision changed",
        ));
    }
    if !query.ids.is_empty() {
        return lookup::page(db, query, status.revision);
    }
    let mut statement = db
        .prepare("SELECT body FROM model_catalog WHERE provider=?1 AND id>?2 ORDER BY id LIMIT ?3")
        .map_err(storage_error)?;
    let rows = statement
        .query_map(
            params![
                query.provider,
                query.after.as_deref().unwrap_or(""),
                u32::from(query.limit) + 1
            ],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(storage_error)?;
    let mut models = Vec::new();
    let mut bytes = 0;
    let mut more = false;
    for row in rows {
        let model = metadata::model(
            &serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?,
        )?;
        let size = encode(&model)?.len() + 1;
        if models.len() == usize::from(query.limit) || bytes + size > 512 * 1024 {
            more = true;
            break;
        }
        models.push(model);
        bytes += size;
    }
    let next = if more {
        models.last().map(|model| model.id.clone())
    } else {
        None
    };
    Ok(catalog::Page {
        revision: status.revision,
        models,
        next,
    })
}

impl Database {
    pub(in crate::store) fn cache_catalog(
        &mut self,
        download: Download,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Result<catalog::Status, Fault> {
        let transaction = self.connection.transaction().map_err(storage_error)?;
        let previous = status(&transaction)?;
        let revision = previous
            .revision
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| storage_error("catalog revision exhausted"))?;
        let updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(storage_error)?
            .as_millis()
            .try_into()
            .map_err(storage_error)?;
        let status = catalog::Status {
            revision,
            updated_at_ms: Some(updated_at_ms),
            providers: download.providers,
            models: download.entries.len() as u32,
            bytes: download.bytes,
        };
        transaction
            .execute("DELETE FROM model_catalog", [])
            .map_err(storage_error)?;
        {
            let mut insert = transaction
                .prepare("INSERT INTO model_catalog(provider,id,body) VALUES(?1,?2,?3)")
                .map_err(storage_error)?;
            for entry in &download.entries {
                insert
                    .execute(params![entry.provider, entry.id, entry.body])
                    .map_err(storage_error)?;
            }
        }
        transaction.execute("INSERT INTO model_catalog_status(id,body) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body", [encode(&status)?]).map_err(storage_error)?;
        let updates = super::completion::refresh(&transaction, self.node)?;
        let event = Event::ModelCatalogChanged(status.clone());
        transaction
            .execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
            .map_err(storage_error)?;
        let envelope = EventEnvelope {
            node: self.node,
            cursor: transaction.last_insert_rowid() as u64,
            event,
        };
        transaction.commit().map_err(storage_error)?;
        for update in updates {
            let _ = events.send(update);
        }
        let _ = events.send(envelope);
        Ok(status)
    }
}

impl Ingress {
    pub(in crate::store) async fn refresh_catalog(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let download = tokio::select! {
            biased;
            _ = self.closed.cancelled() => return Err(unavailable()),
            result = self.catalog.download() => result?,
        };
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::Catalog { download, reply })
            .map_err(|_| Fault::new(ErrorCode::Busy, "Node request queue is unavailable"))?;
        let status = response.await.map_err(|_| unavailable())??;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::CatalogStatus(status)));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
