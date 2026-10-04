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
}

pub(crate) type Snapshot = BTreeMap<Kind, Model>;

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
    Ok(super::agent::providers::list(db)?
        .into_iter()
        .flat_map(|provider| {
            provider
                .models
                .iter()
                .filter_map(|model| {
                    let kinds: Vec<_> = [Kind::Vision, Kind::Image, Kind::Video]
                        .into_iter()
                        .filter(|kind| kind.supports(&provider, model))
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
            let provider =
                super::agent::providers::get(db, binding.provider)?.ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "media provider is unavailable")
                })?;
            let model = provider
                .models
                .iter()
                .find(|model| model.id == binding.model)
                .ok_or_else(|| {
                    Fault::new(ErrorCode::NotConfigured, "media model is unavailable")
                })?;
            if !kind.supports(&provider, model) {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "model does not support this media role",
                ));
            }
            Ok((
                kind,
                Model {
                    provider,
                    model: binding.model.clone(),
                },
            ))
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
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::InvalidRequest,
                "media settings revision exhausted",
            )
        })?;
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
