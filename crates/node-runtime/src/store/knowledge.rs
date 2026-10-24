//! Durable configuration remains in Node storage; retrieval indexes are derived.
mod collections;
pub(super) mod runtime;
pub(crate) mod semantic;
mod sources;
pub(crate) mod storage;

#[cfg(test)]
mod tests;

use super::database::{encode, storage_error};
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, KnowledgeId, Output, Request, SessionId, WorktreeId,
    knowledge::{self, Collection, Definition, Owner, Source},
    plugin::{self, storage::Scope},
};
use serde::{Deserialize, Serialize};
use turso::{Connection, params};

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
    pub(crate) collections: Vec<Stored>,
}

pub(super) async fn catalog(db: &Connection) -> Result<Catalog, Fault> {
    Ok(Catalog {
        collections: collections::load(db).await?,
    })
}

pub(super) async fn list(db: &Connection, request: &Request) -> Result<Vec<Collection>, Fault> {
    let entries = collections::load(db).await?;
    let package = (match request.plugin.as_ref() {
        Some(value) => Some(
            async {
                let context = value;
                sources::package(db, context).await
            }
            .await,
        ),
        None => None,
    })
    .transpose()?;
    Ok(entries
        .into_iter()
        .filter(|stored| visible(stored, request) && readable(stored, request, package.as_ref()))
        .map(|stored| stored.collection)
        .collect())
}

/// Query plans must intersect these durable grants before reading a derived index.
pub(super) async fn allowed(db: &Connection, request: &Request) -> Result<Vec<Stored>, Fault> {
    let package = (match request.plugin.as_ref() {
        Some(value) => Some(
            async {
                let context = value;
                sources::package(db, context).await
            }
            .await,
        ),
        None => None,
    })
    .transpose()?;
    Ok(collections::load(db)
        .await?
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

pub(super) async fn execute(
    db: &Connection,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    match &request.command {
        Command::CreateKnowledgeCollection { definition, plugin } => {
            collections::create(db, request, definition, plugin.as_deref()).await
        }
        Command::UpdateKnowledgeCollection {
            id,
            expected_revision,
            definition,
        } => collections::update(db, request, *id, *expected_revision, definition).await,
        Command::DeleteKnowledgeCollection {
            id,
            expected_revision,
        } => collections::remove(db, request, *id, *expected_revision).await,
        Command::RebuildKnowledgeCollection {
            id,
            expected_revision,
        } => collections::rebuild(db, request, *id, *expected_revision).await,
        Command::PauseKnowledgeCollection {
            id,
            expected_revision,
            paused,
        } => collections::pause(db, request, *id, *expected_revision, *paused).await,
        _ => Err(Fault::invalid("knowledge mutation expected")),
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

pub(super) async fn validate_stored(db: &Connection) -> Result<(), Fault> {
    collections::load(db).await?;
    Ok(())
}

pub(super) async fn progress(
    db: &Connection,
    id: KnowledgeId,
    generation: u64,
    progress: &knowledge::Progress,
) -> Result<Option<Event>, Fault> {
    let mut stored = collections::load(db)
        .await?
        .into_iter()
        .find(|stored| stored.collection.id == id)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "knowledge collection no longer exists"))?;
    if stored.collection.progress.generation != generation || progress.generation != generation {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "knowledge collection generation changed",
        ));
    }
    let settings = super::local_models::settings(db).await?;
    let model = stored.collection.definition.model.clone().or_else(|| {
        settings
            .selected(sailry_protocol::models::Kind::Embedding)
            .map(str::to_owned)
    });
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
    collections::persist(db, &stored).await?;
    Ok(Some(Event::KnowledgeCollectionsChanged))
}

pub(in crate::store) async fn selection_changed(db: &Connection) -> Result<(), Fault> {
    let mut entries = collections::load(db).await?;
    for stored in &mut entries {
        if stored.collection.definition.model.is_none() {
            collections::advance(stored)?;
        }
    }
    for stored in &entries {
        if stored.collection.definition.model.is_none() {
            collections::persist(db, stored).await?;
        }
    }
    Ok(())
}
