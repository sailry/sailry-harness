//! Durable configuration remains in Node storage; retrieval indexes are derived.
mod collections;
mod models;
pub(super) mod runtime;
pub(crate) mod semantic;
mod settings;
mod sources;
pub(crate) mod storage;

#[cfg(test)]
mod tests;

use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, KnowledgeId, Output, Request, SessionId, WorktreeId,
    knowledge::{self, Collection, Definition, Owner, Source},
    plugin::{self, storage::Scope},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Capture {
    pub(crate) package: Option<plugin::Reference>,
    pub(crate) worktree: Option<WorktreeId>,
    pub(crate) session: Option<SessionId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stored {
    pub(crate) collection: Collection,
    pub(crate) capture: Capture,
    #[serde(default)]
    pub(crate) automatic: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Catalog {
    pub(crate) settings: knowledge::Settings,
    pub(crate) models: Vec<knowledge::Registration>,
    pub(crate) collections: Vec<Stored>,
}

pub(super) fn catalog(db: &Connection) -> Result<Catalog, Fault> {
    Ok(Catalog {
        settings: settings::read(db)?,
        models: models::load(db)?,
        collections: collections::load(db)?,
    })
}

pub(super) fn list(db: &Connection, request: &Request) -> Result<Vec<Collection>, Fault> {
    let entries = collections::load(db)?;
    let package = request
        .plugin
        .as_ref()
        .map(|context| sources::package(db, context))
        .transpose()?;
    Ok(entries
        .into_iter()
        .filter(|stored| visible(stored, request) && readable(stored, request, package.as_ref()))
        .map(|stored| stored.collection)
        .collect())
}

/// Query plans must intersect these durable grants before reading a derived index.
pub(super) fn allowed(db: &Connection, request: &Request) -> Result<Vec<Stored>, Fault> {
    let package = request
        .plugin
        .as_ref()
        .map(|context| sources::package(db, context))
        .transpose()?;
    Ok(collections::load(db)?
        .into_iter()
        .filter(|stored| visible(stored, request) && readable(stored, request, package.as_ref()))
        .collect())
}

fn readable(stored: &Stored, request: &Request, package: Option<&plugin::Info>) -> bool {
    match (&request.plugin, package, &stored.collection.owner) {
        (Some(context), Some(package), Owner::Plugin { name }) if name == &context.package.name => {
            sources::readable(stored, package)
        }
        _ => true,
    }
}

fn visible(stored: &Stored, request: &Request) -> bool {
    let Some(context) = &request.plugin else {
        return true;
    };
    let collection = &stored.collection;
    let owner =
        matches!(&collection.owner, Owner::Plugin { name } if name == &context.package.name);
    if owner {
        match &collection.definition.source {
            Source::Worktree { worktree, .. } => Some(*worktree) == context.worktree,
            Source::Storage {
                scope: Scope::Conversation,
                ..
            } => stored.capture.session == context.session && context.session.is_some(),
            Source::Storage {
                scope: Scope::Node, ..
            } => true,
            Source::Directory { .. } | Source::File { .. } => true,
        }
    } else {
        collection
            .definition
            .shared_with
            .contains(&context.package.name)
    }
}

pub(super) fn settings(db: &Connection, request: &Request) -> Result<knowledge::Settings, Fault> {
    controller(request)?;
    settings::read(db)
}

pub(super) fn execute(
    db: &Connection,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    match &request.command {
        Command::RegisterKnowledgeModel { definition } => {
            controller(request)?;
            models::register(db, definition)
        }
        Command::SaveKnowledgeSettings(settings) => {
            controller(request)?;
            settings::save(db, settings)
        }
        Command::CreateKnowledgeCollection { definition, plugin } => {
            collections::create(db, request, definition, plugin.as_deref())
        }
        Command::UpdateKnowledgeCollection {
            id,
            expected_revision,
            definition,
        } => collections::update(db, request, *id, *expected_revision, definition),
        Command::DeleteKnowledgeCollection {
            id,
            expected_revision,
        } => collections::remove(db, request, *id, *expected_revision),
        Command::RebuildKnowledgeCollection {
            id,
            expected_revision,
        } => collections::rebuild(db, request, *id, *expected_revision),
        Command::PauseKnowledgeCollection {
            id,
            expected_revision,
            paused,
        } => collections::pause(db, request, *id, *expected_revision, *paused),
        _ => Err(Fault::invalid("knowledge mutation expected")),
    }
}

fn controller(request: &Request) -> Result<(), Fault> {
    if request.plugin.is_some() {
        Err(denied("knowledge model settings require a controller"))
    } else {
        Ok(())
    }
}

fn denied(message: &str) -> Fault {
    Fault::new(ErrorCode::PermissionDenied, message)
}

fn next(value: u64) -> Result<u64, Fault> {
    value
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("knowledge revision is exhausted"))
}

pub(super) fn validate_stored(db: &Connection) -> Result<(), Fault> {
    models::load(db)?;
    settings::read(db)?;
    collections::load(db)?;
    Ok(())
}

pub(super) fn progress(
    db: &Connection,
    id: KnowledgeId,
    generation: u64,
    progress: &knowledge::Progress,
) -> Result<Option<Event>, Fault> {
    let mut stored = collections::load(db)?
        .into_iter()
        .find(|stored| stored.collection.id == id)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "knowledge collection no longer exists"))?;
    if stored.collection.progress.generation != generation || progress.generation != generation {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "knowledge collection generation changed",
        ));
    }
    let model = stored
        .collection
        .definition
        .model
        .or(settings::read(db)?.model);
    if progress.model.is_some() && progress.model != model {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "knowledge collection model changed",
        ));
    }
    if stored.collection.progress == *progress {
        return Ok(None);
    }
    stored.collection.progress = progress.clone();
    collections::persist(db, &stored)?;
    Ok(Some(Event::KnowledgeCollectionsChanged))
}
