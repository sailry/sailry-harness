//! Host settings and immutable session choices share the existing Node transaction.
use super::database::{encode, storage_error};
use sailry_protocol::{
    conversation::Provider,
    media::{self, Kind},
    *,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use turso::{Connection, params};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Model {
    pub provider: Provider,
    pub model: String,
    #[serde(skip)]
    pub effort: Option<Effort>,
}

pub(crate) type Snapshot = BTreeMap<Kind, Option<Model>>;

#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub primary: Option<Model>,
    pub fallback: Option<Model>,
}

pub(crate) type Models = BTreeMap<Kind, Plan>;

pub(super) async fn read(db: &Connection) -> Result<media::Settings, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM media_settings WHERE singleton=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(()).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(super) async fn catalog(db: &Connection) -> Result<Vec<media::Candidate>, Fault> {
    let mut providers = super::agent::providers::list(db).await?;
    for provider in &mut providers {
        super::providers::completion::fill(db, provider).await?;
        if super::providers::presets::capture_read(db, provider)
            .await
            .is_err()
        {
            provider.enabled = false;
        }
    }
    Ok(providers
        .into_iter()
        .flat_map(|provider| {
            provider
                .models
                .iter()
                .filter_map(|model| {
                    let kinds: Vec<_> = [Kind::Vision, Kind::Image, Kind::Video]
                        .into_iter()
                        .filter(|kind| crate::providers::media::supports(*kind, &provider, model))
                        .collect();
                    (!kinds.is_empty()).then(|| media::Candidate {
                        provider: provider.id,
                        provider_name: provider.name.clone(),
                        model: model.id.clone(),
                        kinds,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect())
}

async fn resolve(db: &Connection, settings: &media::Settings) -> Result<Snapshot, Fault> {
    let mut snapshot = BTreeMap::new();
    for (&kind, binding) in &settings.bindings {
        let Some(binding) = binding else {
            snapshot.insert(kind, None);
            continue;
        };
        let mut provider = super::agent::providers::get(db, binding.provider)
            .await?
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "media provider is unavailable"))?;
        super::providers::completion::fill(db, &mut provider).await?;
        super::providers::presets::capture_read(db, &mut provider).await?;
        let model = provider
            .models
            .iter()
            .find(|model| model.id == binding.model)
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "media model is unavailable"))?;
        if !crate::providers::media::supports(kind, &provider, model) {
            return Err(Fault::invalid("model does not support this media role"));
        }
        crate::providers::login::capture(&mut provider)?;
        snapshot.insert(
            kind,
            Some(Model {
                provider,
                model: binding.model.clone(),
                effort: None,
            }),
        );
    }
    Ok(snapshot)
}

/// Automatic routes use the admitted model; explicit fallbacks retain the
/// session-owned snapshot and disabled roles never create an execution plan.
pub(crate) fn for_turn(
    snapshot: Snapshot,
    provider: Option<&Provider>,
    config: &SessionConfig,
) -> Models {
    let primary = provider.map(|provider| {
        let mut provider = provider.clone();
        provider.credential = config.credential.clone();
        Model {
            provider,
            model: config.model.clone(),
            effort: Some(config.effort),
        }
    });
    [Kind::Vision, Kind::Image, Kind::Video]
        .into_iter()
        .filter_map(|kind| {
            let fallback = match snapshot.get(&kind) {
                Some(None) => return None,
                Some(Some(model)) => Some(model.clone()),
                None => None,
            };
            (primary.is_some() || fallback.is_some()).then(|| {
                (
                    kind,
                    Plan {
                        primary: primary.clone(),
                        fallback,
                    },
                )
            })
        })
        .collect()
}

pub(super) async fn save(
    db: &Connection,
    settings: &media::Settings,
) -> Result<(Output, Option<Event>), Fault> {
    if read(db).await?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "media settings changed",
        ));
    }
    resolve(db, settings).await?;
    let mut saved = settings.clone();
    saved.revision = saved
        .revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("media settings revision exhausted"))?;
    db.execute("INSERT INTO media_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&saved)?]).await.map_err(storage_error)?;
    Ok((
        Output::MediaSettings(saved.clone()),
        Some(Event::MediaSettingsChanged(saved)),
    ))
}

pub(super) async fn freeze(
    db: &Connection,
    session: SessionId,
    source: Option<SessionId>,
) -> Result<(), Fault> {
    let snapshot = match source {
        Some(source) => session_models(db, source).await?,
        None => resolve(db, &read(db).await?).await?,
    };
    db.execute(
        "INSERT INTO session_media(session,body) VALUES(?1,?2)",
        params![session.to_string(), encode(&snapshot)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn session_models(db: &Connection, session: SessionId) -> Result<Snapshot, Fault> {
    let body: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT body FROM session_media WHERE session=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([session.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    serde_json::from_slice(&body).map_err(storage_error)
}

#[cfg(test)]
mod tests;
