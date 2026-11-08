use super::super::{
    Ingress, Job,
    database::{Database, encode, record, storage_error},
    unavailable,
};
use crate::providers::catalog::{Download, metadata};
use sailry_link::Admission;
use sailry_protocol::{conversation::catalog, *};
use std::collections::BTreeMap;
use tokio::sync::{broadcast, oneshot};
use turso::{Connection, params, transaction::TransactionBehavior};
mod lookup;

pub(crate) async fn status(db: &Connection) -> Result<catalog::Status, Fault> {
    let mut statement = db
        .prepare("SELECT body FROM model_catalog_status WHERE id=1")
        .await
        .map_err(storage_error)?;
    let mut status: catalog::Status = match statement.query_row(()).await {
        Ok(row) => serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
            .map_err(storage_error)?,
        Err(turso::Error::QueryReturnedNoRows) => catalog::Status::default(),
        Err(error) => return Err(storage_error(error)),
    };
    status.scores = super::scores::stored(db).await?.status;
    Ok(status)
}

pub(crate) async fn page(db: &Connection, query: &catalog::Query) -> Result<catalog::Page, Fault> {
    if let Some(provider) = &query.provider {
        metadata::identifier(provider, 128)?;
    }
    if query.ids.len() > 100 || query.provider.is_none() && query.ids.is_empty() {
        return Err(Fault::invalid(
            "catalog lookup requires bounded model identifiers",
        ));
    }
    for id in &query.ids {
        metadata::identifier(id, 256)?;
    }
    if query.limit == 0 || query.limit > 100 || (query.after.is_some() && query.revision.is_none())
    {
        return Err(Fault::invalid(
            "catalog page requires bounded size and revision",
        ));
    }
    if let Some(after) = &query.after {
        metadata::identifier(after, 256)?;
    }
    let status = status(db).await?;
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
        return lookup::page(db, query, status.revision).await;
    }
    let mut statement = db
        .prepare("SELECT body FROM model_catalog WHERE provider=?1 AND id>?2 ORDER BY id LIMIT ?3")
        .await
        .map_err(storage_error)?;
    let mut rows = statement
        .query(params![
            query.provider.as_deref(),
            query.after.as_deref().unwrap_or(""),
            u32::from(query.limit) + 1
        ])
        .await
        .map_err(storage_error)?;
    let mut models = Vec::new();
    let mut bytes = 0;
    let mut more = false;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let model = metadata::model(
            &serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
                .map_err(storage_error)?,
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

async fn replace(db: &Connection, entries: &[metadata::Entry]) -> Result<(), Fault> {
    let mut previous = BTreeMap::new();
    {
        let mut statement = db
            .prepare("SELECT provider,id,body FROM model_catalog")
            .await
            .map_err(storage_error)?;
        let mut rows = statement.query(()).await.map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            previous.insert(
                (
                    row.get::<String>(0).map_err(storage_error)?,
                    row.get::<String>(1).map_err(storage_error)?,
                ),
                row.get::<Vec<u8>>(2).map_err(storage_error)?,
            );
        }
    }
    let mut update = db
        .prepare("UPDATE model_catalog SET body=?1 WHERE provider=?2 AND id=?3")
        .await
        .map_err(storage_error)?;
    let mut insert = db
        .prepare("INSERT INTO model_catalog(provider,id,body) VALUES(?1,?2,?3)")
        .await
        .map_err(storage_error)?;
    // Keep unchanged rows and indexed keys in place. Deleting the entire catalog
    // makes each replacement INSERT seek through MVCC-shadowed index entries.
    for entry in entries {
        match previous.remove(&(entry.provider.clone(), entry.id.clone())) {
            Some(body) if body == entry.body => {}
            Some(_) => {
                update
                    .execute(params![
                        entry.body.as_slice(),
                        entry.provider.as_str(),
                        entry.id.as_str()
                    ])
                    .await
                    .map_err(storage_error)?;
            }
            None => {
                insert
                    .execute(params![
                        entry.provider.as_str(),
                        entry.id.as_str(),
                        entry.body.as_slice()
                    ])
                    .await
                    .map_err(storage_error)?;
            }
        }
    }
    let mut delete = db
        .prepare("DELETE FROM model_catalog WHERE provider=?1 AND id=?2")
        .await
        .map_err(storage_error)?;
    for (provider, id) in previous.into_keys() {
        delete
            .execute(params![provider, id])
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

impl Database {
    pub(in crate::store) async fn cache_catalog(
        &mut self,
        download: Result<Download, Fault>,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Result<catalog::Status, Fault> {
        let publication = self.publication.clone();
        let node = self.node;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Concurrent)
            .await
            .map_err(storage_error)?;
        let mut status = status(&transaction).await?;
        let mut updates = Vec::new();
        match download {
            Ok(download) => {
                status.revision = status
                    .revision
                    .checked_add(1)
                    .filter(|revision| *revision <= i64::MAX as u64)
                    .ok_or_else(|| storage_error("catalog revision exhausted"))?;
                status.updated_at_ms = Some(super::scores::now()?);
                status.providers = download.providers;
                status.models = download.entries.len() as u32;
                status.bytes = download.bytes;
                status.fault = None;
                replace(&transaction, &download.entries).await?;
                write(&transaction, &status).await?;
                updates = super::completion::refresh(&transaction, node).await?;
            }
            Err(fault) => {
                status.fault = Some(fault);
                write(&transaction, &status).await?;
            }
        }
        updates.extend(
            record(
                &transaction,
                node,
                vec![Event::ModelCatalogChanged(status.clone())],
            )
            .await?,
        );
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        for update in updates {
            publication.send(events, update);
        }
        Ok(status)
    }
}

impl Ingress {
    pub(in crate::store) async fn refresh_catalog(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        let _permit = self.catalog.acquire()?;
        let download = tokio::select! {
            biased;
            _ = self.closed.cancelled() => return Err(unavailable()),
            result = self.catalog.download() => result,
        };
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (reply, response) = oneshot::channel();
        self.submit(Job::Catalog { download, reply })
            .await
            .map_err(|_| Fault::new(ErrorCode::Busy, "Node request queue is unavailable"))?;
        let metadata = response.await.map_err(|_| unavailable())?;
        let storage = self.storage.get().ok_or_else(unavailable)?;
        let score_status = storage
            .read(|db| Box::pin(super::scores::stored(db)))
            .await?;
        let mut status = metadata.as_ref().ok().cloned();
        if score_status.status.enabled && self.catalog.scores.available() {
            let download = tokio::select! {
                biased;
                _ = self.closed.cancelled() => return Err(unavailable()),
                result = self.catalog.scores.download(score_status.etag.as_deref()) => result,
            };
            let (reply, response) = oneshot::channel();
            self.submit(Job::Scores {
                download,
                revision: score_status.status.revision,
                reply,
            })
            .await
            .map_err(|_| unavailable())?;
            status = Some(response.await.map_err(|_| unavailable())??);
        }
        let status = status.ok_or_else(|| metadata.as_ref().unwrap_err().clone())?;
        metadata?;
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

async fn write(db: &Connection, status: &catalog::Status) -> Result<(), Fault> {
    let mut body = serde_json::to_value(status).map_err(storage_error)?;
    body.as_object_mut().unwrap().remove("scores");
    db.execute(
        "INSERT INTO model_catalog_status(id,body) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
        [encode(&body)?],
    ).await.map_err(storage_error)?;
    Ok(())
}
