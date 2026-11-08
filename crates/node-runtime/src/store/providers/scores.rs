use super::super::database::{Database, encode, record, storage_error};
use crate::providers::scores::{Dataset, Download, matching};
use sailry_protocol::{conversation::catalog, *};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::broadcast;
use turso::{Connection, params, transaction::TransactionBehavior};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Stored {
    #[serde(flatten)]
    pub status: catalog::ScoreStatus,
    pub etag: Option<String>,
    pub entries: u32,
}

pub(crate) async fn stored(db: &Connection) -> Result<Stored, Fault> {
    match db
        .prepare("SELECT body FROM model_score_status WHERE id=1")
        .await
        .map_err(storage_error)?
        .query_row(())
        .await
    {
        Ok(row) => serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
            .map_err(storage_error),
        Err(turso::Error::QueryReturnedNoRows) => Ok(Stored::default()),
        Err(error) => Err(storage_error(error)),
    }
}

async fn write(db: &Connection, state: &Stored) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO model_score_status(id,body) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
        [encode(state)?],
    ).await.map_err(storage_error)?;
    Ok(())
}

pub(super) fn now() -> Result<u64, Fault> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(storage_error)?
        .as_millis()
        .try_into()
        .map_err(storage_error)
}

fn increment(revision: u64) -> Result<u64, Fault> {
    revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| storage_error("model scores revision exhausted"))
}

async fn clear(db: &Connection) -> Result<(), Fault> {
    db.execute("DELETE FROM model_scores", ())
        .await
        .map_err(storage_error)?;
    db.execute("DELETE FROM model_score_aliases", ())
        .await
        .map_err(storage_error)?;
    Ok(())
}

async fn rows(db: &Connection) -> Result<BTreeMap<(String, String), Vec<u8>>, Fault> {
    let mut statement = db
        .prepare("SELECT model,effort,body FROM model_scores")
        .await
        .map_err(storage_error)?;
    let mut rows = statement.query(()).await.map_err(storage_error)?;
    let mut values = BTreeMap::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        values.insert(
            (
                row.get::<String>(0).map_err(storage_error)?,
                row.get::<String>(1).map_err(storage_error)?,
            ),
            row.get::<Vec<u8>>(2).map_err(storage_error)?,
        );
    }
    Ok(values)
}

async fn aliases(db: &Connection) -> Result<BTreeMap<String, String>, Fault> {
    let mut statement = db
        .prepare("SELECT alias,model FROM model_score_aliases")
        .await
        .map_err(storage_error)?;
    let mut rows = statement.query(()).await.map_err(storage_error)?;
    let mut values = BTreeMap::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        values.insert(
            row.get::<String>(0).map_err(storage_error)?,
            row.get::<String>(1).map_err(storage_error)?,
        );
    }
    Ok(values)
}

async fn replace(db: &Connection, dataset: &Dataset) -> Result<bool, Fault> {
    let current = dataset
        .entries
        .iter()
        .map(|entry| {
            Ok((
                (entry.model.clone(), entry.effort.clone()),
                encode(&entry.score)?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, Fault>>()?;
    if rows(db).await? == current && aliases(db).await? == dataset.aliases {
        return Ok(false);
    }
    clear(db).await?;
    let mut insert = db
        .prepare("INSERT INTO model_scores(model,effort,body) VALUES(?1,?2,?3)")
        .await
        .map_err(storage_error)?;
    for ((model, effort), body) in current {
        insert
            .execute(params![model, effort, body])
            .await
            .map_err(storage_error)?;
    }
    let mut insert = db
        .prepare("INSERT INTO model_score_aliases(alias,model) VALUES(?1,?2)")
        .await
        .map_err(storage_error)?;
    for (alias, model) in &dataset.aliases {
        insert
            .execute(params![alias.as_str(), model.as_str()])
            .await
            .map_err(storage_error)?;
    }
    Ok(true)
}

impl Database {
    pub(in crate::store) async fn cache_scores(
        &mut self,
        download: Result<Download, Fault>,
        revision: u64,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Result<catalog::Status, Fault> {
        let publication = self.publication.clone();
        let node = self.node;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Concurrent)
            .await
            .map_err(storage_error)?;
        let mut state = stored(&transaction).await?;
        if state.status.revision == revision {
            state.status.checked_at_ms = Some(now()?);
            match download {
                Ok(Download::Unchanged) => state.status.fault = None,
                Ok(Download::Content { dataset, etag }) => {
                    let changed = replace(&transaction, &dataset).await?
                        || state.status.batch.as_deref() != Some(&dataset.batch)
                        || state.status.published_at.as_deref() != Some(&dataset.published_at);
                    if changed {
                        state.status.revision = increment(state.status.revision)?;
                        state.status.updated_at_ms = state.status.checked_at_ms;
                    }
                    state.etag = etag;
                    state.status.batch = Some(dataset.batch);
                    state.status.published_at = Some(dataset.published_at);
                    state.status.models = dataset
                        .entries
                        .iter()
                        .map(|entry| &entry.model)
                        .collect::<BTreeSet<_>>()
                        .len() as u32;
                    state.entries = dataset.entries.len() as u32;
                    state.status.fault = None;
                }
                Err(fault) => state.status.fault = Some(fault),
            }
            write(&transaction, &state).await?;
        }
        let status = super::catalog::status(&transaction).await?;
        let updates = record(
            &transaction,
            node,
            vec![Event::ModelCatalogChanged(status.clone())],
        )
        .await?;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        for update in updates {
            publication.send(events, update);
        }
        Ok(status)
    }
}

pub(crate) async fn read(
    db: &Connection,
    references: &[catalog::ModelRef],
) -> Result<catalog::Scores, Fault> {
    if references.len() > 512 {
        return Err(Fault::invalid("too many model score references"));
    }
    let state = stored(db).await?;
    let mut output = catalog::Scores {
        revision: state.status.revision,
        batch: state.status.batch,
        published_at: state.status.published_at,
        scores: Vec::new(),
    };
    if state.entries == 0 || references.is_empty() {
        return Ok(output);
    }
    let rows = rows(db).await?;
    let aliases = aliases(db).await?;
    let models = rows.keys().map(|(model, _)| model.clone()).collect();
    let mut providers = BTreeMap::new();
    let mut catalogs = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for reference in references {
        if !seen.insert((reference.provider, &reference.model)) {
            continue;
        }
        if !providers.contains_key(&reference.provider) {
            providers.insert(
                reference.provider,
                super::super::agent::providers::get(db, reference.provider).await?,
            );
        }
        let Some(provider) = &providers[&reference.provider] else {
            continue;
        };
        let Some(model) = provider
            .models
            .iter()
            .find(|model| model.id == reference.model)
        else {
            continue;
        };
        let mut candidates = vec![model.id.clone()];
        candidates.extend(model.display_name.iter().cloned());
        if let Some(preset) = &provider.preset {
            if !catalogs.contains_key(&preset.id) {
                catalogs.insert(preset.id.clone(), catalog_models(db, &preset.id).await?);
            }
            candidates.extend(
                catalogs[&preset.id]
                    .iter()
                    .filter(|(id, name)| {
                        *id == model.id
                            || model.display_name.is_some() && *name == model.display_name
                    })
                    .map(|(id, _)| id.clone()),
            );
        }
        if let Some(matched) = matching::resolve(&candidates, &models, &aliases) {
            let efforts = rows
                .iter()
                .filter(|((model, _), _)| *model == matched)
                .map(|(_, body)| serde_json::from_slice(body).map_err(storage_error))
                .collect::<Result<Vec<_>, _>>()?;
            output.scores.push(catalog::ModelScore {
                provider: reference.provider,
                model: reference.model.clone(),
                efforts,
            });
        }
    }
    Ok(output)
}

async fn catalog_models(
    db: &Connection,
    provider: &str,
) -> Result<Vec<(String, Option<String>)>, Fault> {
    let mut statement = db
        .prepare("SELECT id,body FROM model_catalog WHERE provider=?1")
        .await
        .map_err(storage_error)?;
    let mut rows = statement.query([provider]).await.map_err(storage_error)?;
    let mut models = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let id = row.get::<String>(0).map_err(storage_error)?;
        let body: serde_json::Value =
            serde_json::from_slice(&row.get::<Vec<u8>>(1).map_err(storage_error)?)
                .map_err(storage_error)?;
        models.push((id, body["name"].as_str().map(str::to_owned)));
    }
    Ok(models)
}
