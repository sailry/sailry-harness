//! Host settings and immutable session choices share the existing Node transaction.
use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    conversation::Provider,
    media::{self, Kind},
    *,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

pub(super) fn read(db: &Connection) -> Result<media::Settings, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM media_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(super) fn catalog(db: &Connection) -> Result<Vec<media::Candidate>, Fault> {
    let mut providers = super::agent::providers::list(db)?;
    for provider in &mut providers {
        super::providers::completion::fill(db, provider)?;
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

fn resolve(db: &Connection, settings: &media::Settings) -> Result<Snapshot, Fault> {
    settings
        .bindings
        .iter()
        .map(|(&kind, binding)| {
            let Some(binding) = binding else {
                return Ok((kind, None));
            };
            let mut provider =
                super::agent::providers::get(db, binding.provider)?.ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "media provider is unavailable")
                })?;
            super::providers::completion::fill(db, &mut provider)?;
            let model = provider
                .models
                .iter()
                .find(|model| model.id == binding.model)
                .ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "media model is unavailable")
                })?;
            if !crate::providers::media::supports(kind, &provider, model) {
                return Err(Fault::invalid("model does not support this media role"));
            }
            crate::providers::login::capture(&mut provider)?;
            Ok((
                kind,
                Some(Model {
                    provider,
                    model: binding.model.clone(),
                    effort: None,
                }),
            ))
        })
        .collect()
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

pub(super) fn save(
    db: &Connection,
    settings: &media::Settings,
) -> Result<(Output, Option<Event>), Fault> {
    if read(db)?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "media settings changed",
        ));
    }
    resolve(db, settings)?;
    let mut saved = settings.clone();
    saved.revision = saved
        .revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("media settings revision exhausted"))?;
    db.execute("INSERT INTO media_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&saved)?]).map_err(storage_error)?;
    Ok((
        Output::MediaSettings(saved.clone()),
        Some(Event::MediaSettingsChanged(saved)),
    ))
}

pub(super) fn freeze(
    db: &Connection,
    session: SessionId,
    source: Option<SessionId>,
) -> Result<(), Fault> {
    let snapshot = match source {
        Some(source) => session_models(db, source)?,
        None => resolve(db, &read(db)?)?,
    };
    db.execute(
        "INSERT INTO session_media(session,body) VALUES(?1,?2)",
        params![session.to_string(), encode(&snapshot)?],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn session_models(db: &Connection, session: SessionId) -> Result<Snapshot, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM session_media WHERE session=?1",
            [session.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    serde_json::from_slice(&body).map_err(storage_error)
}

#[cfg(test)]
mod tests;
