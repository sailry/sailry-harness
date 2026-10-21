//! Durable local model metadata and the shared execution owner's public boundary.
pub(in crate::store) mod approvals;
#[cfg(test)]
mod tests;

use super::{
    Ingress, Job,
    database::{Database, encode, storage_error},
};
use crate::models::registry::{Registration, Update, catalog};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_link::{Admission, CancellationToken};
use sailry_protocol::{
    Command, ErrorCode, Event, EventEnvelope, Fault, Output, Request,
    models::{Definition, Kind, Model, Settings},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::{broadcast, oneshot, watch};

#[derive(Clone, Debug)]
pub(crate) struct Catalog {
    pub settings: Settings,
    pub definitions: BTreeMap<String, Arc<Definition>>,
}

pub(in crate::store) fn initialize(db: &Connection) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO local_model_settings(singleton,body) VALUES(1,?1)",
        [encode(&Settings::default())?],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn settings(db: &Connection) -> Result<Settings, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM local_model_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let mut settings: Settings = serde_json::from_slice(&body).map_err(storage_error)?;
    settings.fill_defaults();
    settings.validate()?;
    Ok(settings)
}

pub(in crate::store) fn read(db: &Connection) -> Result<Catalog, Fault> {
    let settings = settings(db)?;
    let mut query = db
        .prepare("SELECT id,kind,digest,body FROM local_models ORDER BY id")
        .map_err(storage_error)?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })
        .map_err(storage_error)?;
    let records = rows
        .map(|row| {
            let (id, kind, digest, body) = row.map_err(storage_error)?;
            let definition: Definition = serde_json::from_slice(&body).map_err(storage_error)?;
            if digest != catalog::id(&definition) || kind != kind_name(definition.format.kind()) {
                return Err(storage_error(
                    "local model metadata does not match its definition",
                ));
            }
            Ok(Registration { id, definition })
        })
        .collect::<Result<Vec<_>, Fault>>()?;
    let definitions = catalog::definitions(&records)?;
    for (kind, choice) in &settings.choices {
        if let Some(id) = &choice.model
            && !definitions
                .get(id)
                .is_some_and(|definition| definition.format.kind() == *kind)
        {
            return Err(Fault::invalid(
                "selected local model is not registered for its kind",
            ));
        }
    }
    Ok(Catalog {
        settings,
        definitions,
    })
}

pub(in crate::store) fn require(
    db: &Connection,
    kind: Kind,
    id: Option<&str>,
) -> Result<(), Fault> {
    if let Some(id) = id
        && definition(db, id)?.format.kind() != kind
    {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "local model is not registered for its kind",
        ));
    }
    Ok(())
}

pub(in crate::store) fn definition(db: &Connection, id: &str) -> Result<Arc<Definition>, Fault> {
    read(db)?
        .definitions
        .remove(id)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "local model is not registered"))
}

pub(in crate::store) fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Embedding => "embedding",
        Kind::Ocr => "ocr",
        Kind::Decision => "decision",
        Kind::Speech => "speech",
    }
}

pub(in crate::store) fn execute(
    db: &Connection,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    match &request.command {
        Command::ReadLocalModelSettings => {
            controller(request)?;
            Ok((Output::LocalModelSettings(settings(db)?), None))
        }
        Command::SaveLocalModelSettings(settings) => {
            controller(request)?;
            save(db, settings)
        }
        Command::RegisterLocalModel { definition } => register(db, definition),
        _ => Err(Fault::invalid("local model configuration command expected")),
    }
}

fn save(db: &Connection, settings: &Settings) -> Result<(Output, Option<Event>), Fault> {
    let previous = self::settings(db)?;
    super::commands::check_revision(previous.revision, settings.revision)?;
    let mut updated = settings.clone();
    updated.fill_defaults();
    updated.validate()?;
    for (kind, choice) in &updated.choices {
        require(db, *kind, choice.model.as_deref())?;
    }
    updated.revision = updated
        .revision
        .checked_add(1)
        .filter(|revision| *revision <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("local model settings revision is exhausted"))?;
    if previous.selected(Kind::Embedding) != updated.selected(Kind::Embedding) {
        super::knowledge::selection_changed(db)?;
    }
    db.execute(
        "UPDATE local_model_settings SET body=?1 WHERE singleton=1",
        [encode(&updated)?],
    )
    .map_err(storage_error)?;
    Ok((
        Output::LocalModelSettings(updated.clone()),
        Some(Event::LocalModelSettingsChanged {
            revision: updated.revision,
        }),
    ))
}

fn register(db: &Connection, definition: &Definition) -> Result<(Output, Option<Event>), Fault> {
    definition.validate()?;
    if definition.format.kind() == Kind::Speech {
        return Err(Fault::invalid("speech belongs to the controller"));
    }
    crate::models::availability::ensure(definition.format.kind())?;
    let mut definition = definition.clone();
    definition
        .assets
        .sort_by(|left, right| left.name.cmp(&right.name));
    for asset in &mut definition.assets {
        asset.source.revision.make_ascii_lowercase();
        asset.sha256.make_ascii_lowercase();
    }
    let digest = catalog::id(&definition);
    // Definitions imported from the old embedding catalog retain their original ID.
    let previous: Option<String> = db
        .query_row(
            "SELECT id FROM local_models WHERE digest=?1",
            [&digest],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let registered = previous.is_some();
    let id = previous.unwrap_or_else(|| digest.clone());
    if !registered && !catalog::builtins().contains_key(&id) {
        let kind = definition.format.kind();
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM local_models WHERE kind=?1",
                [kind_name(kind)],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if count >= catalog::LIMIT as i64 {
            return Err(Fault::invalid("local model catalog exceeds its limit"));
        }
        db.execute(
            "INSERT INTO local_models(id,kind,digest,body) VALUES(?1,?2,?3,?4)",
            params![id, kind_name(kind), digest, encode(&definition)?],
        )
        .map_err(storage_error)?;
    }
    Ok((Output::LocalModel { id }, None))
}

pub(in crate::store) fn models(
    database: &mut Database,
    models: Vec<Model>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let envelopes = super::database::record(
        &transaction,
        database.node,
        vec![Event::LocalModelsChanged { models }],
    )?;
    transaction.commit().map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(())
}

pub(in crate::store) async fn start(ingress: &Arc<Ingress>) -> Result<(), Fault> {
    ingress.local_models.models(None).await?;
    let mut updates = ingress.local_models.subscribe();
    let previous = updates.borrow_and_update().clone();
    let weak = Arc::downgrade(ingress);
    let stop = ingress.closed.clone();
    let observer = weak.clone();
    tokio::spawn(follow(
        updates,
        previous,
        stop,
        move |models| {
            let weak = weak.clone();
            async move {
                let ingress = weak.upgrade().ok_or_else(super::unavailable)?;
                let (reply, result) = oneshot::channel();
                ingress
                    .send_internal(Job::LocalModels { models, reply })
                    .await?;
                result.await.map_err(|_| super::unavailable())?
            }
        },
        move |reaction| {
            if let Some(ingress) = observer.upgrade()
                && let Some(knowledge) = ingress.knowledge.get()
            {
                match reaction {
                    Reaction::Invalidate => knowledge.invalidate(),
                    Reaction::Refresh => knowledge.refresh(),
                }
            }
        },
    ));
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reaction {
    Invalidate,
    Refresh,
}

/// Records catalog changes and wakes knowledge until the registry channel closes.
async fn follow<W, F>(
    mut updates: watch::Receiver<Update>,
    mut previous: Update,
    stop: CancellationToken,
    mut write: W,
    mut react: impl FnMut(Reaction),
) where
    W: FnMut(Vec<Model>) -> F,
    F: std::future::Future<Output = Result<(), Fault>>,
{
    let mut recorded = previous.models.clone();
    loop {
        tokio::select! { _ = stop.cancelled() => break, result = updates.changed() => if result.is_err() { break } }
        let latest = updates.borrow_and_update().clone();
        if latest.models != recorded {
            match write(latest.models.clone()).await {
                Ok(()) => recorded = latest.models.clone(),
                // The next registry change retries with the complete catalog.
                Err(error) => tracing::warn!(
                    target: "sailry",
                    area = "model",
                    phase = "failed",
                    operation = "local_models_event",
                    code = ?error.code
                ),
            }
        }
        if let Some(reaction) = reaction(&previous, &latest) {
            react(reaction);
        }
        previous = latest;
    }
}

/// Embedding changes invalidate derived vectors; OCR changes only re-extract sources.
fn reaction(previous: &Update, latest: &Update) -> Option<Reaction> {
    if latest.embedding != previous.embedding
        || installed(&latest.models, Kind::Embedding)
            != installed(&previous.models, Kind::Embedding)
    {
        Some(Reaction::Invalidate)
    } else if latest.ocr != previous.ocr
        || installed(&latest.models, Kind::Ocr) != installed(&previous.models, Kind::Ocr)
    {
        Some(Reaction::Refresh)
    } else {
        None
    }
}

fn installed(models: &[Model], kind: Kind) -> BTreeSet<&str> {
    models
        .iter()
        .filter(|model| {
            model.definition.format.kind() == kind
                && model.state == sailry_protocol::models::State::Installed
        })
        .map(|model| model.id.as_str())
        .collect()
}

impl Ingress {
    pub(in crate::store) async fn read_local_models(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = match &request.command {
            Command::ReadLocalModels { kind } => {
                let models = self.local_models.models(*kind).await?;
                let catalog = tokio::task::spawn_blocking(move || {
                    crate::models::availability::catalog(models)
                })
                .await
                .map_err(storage_error)?;
                Output::LocalModels(catalog)
            }
            Command::CancelLocalModel { id } => {
                controller(&request)?;
                self.local_models.definition(id)?;
                self.local_models.cancel(id);
                Output::LocalModel { id: id.clone() }
            }
            _ => return Err(Fault::invalid("local model read expected")),
        };
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    pub(in crate::store) async fn decide(&self, request: Request) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let Command::Decide(input) = request.command else {
            return Err(Fault::invalid("decision request expected"));
        };
        let stop = self.closed.child_token();
        let _guard = stop.clone().drop_guard();
        let output = crate::decision::decide(&self.local_models, input, &stop).await?;
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(Output::Decision(output)));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    pub(in crate::store) async fn change_local_model(
        &self,
        request: &Request,
        stop: &CancellationToken,
        reporter: Option<&crate::installation::Reporter>,
    ) -> Result<Output, Fault> {
        if !matches!(request.command, Command::InstallLocalModel { .. }) {
            if request.plugin.is_some() {
                self.check_plugin(self.node, request.clone()).await?;
            } else {
                controller(request)?;
            }
        }
        let id = match &request.command {
            Command::InstallLocalModel { id } => {
                self.local_models.install(id, stop, reporter).await?;
                id
            }
            Command::DeleteLocalModel { id } => {
                self.local_models.remove(id).await?;
                id
            }
            _ => return Err(Fault::invalid("local model installation command expected")),
        };
        Ok(Output::LocalModel { id: id.clone() })
    }
}

fn controller(request: &Request) -> Result<(), Fault> {
    if request.plugin.is_some() {
        Err(Fault::new(
            ErrorCode::PermissionDenied,
            "local model settings require a controller",
        ))
    } else {
        Ok(())
    }
}
