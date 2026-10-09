use super::*;

pub(super) fn load(db: &Connection) -> Result<Vec<Stored>, Fault> {
    let mut query = db
        .prepare("SELECT id,revision,generation,body FROM knowledge_collections ORDER BY id")
        .map_err(storage_error)?;
    let mut rows = query.query([]).map_err(storage_error)?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next().map_err(storage_error)? {
        let body: Vec<u8> = row.get(3).map_err(storage_error)?;
        let stored: Stored = serde_json::from_slice(&body).map_err(storage_error)?;
        sources::check(&stored)?;
        models::require(db, stored.collection.definition.model)?;
        if row.get::<_, String>(0).map_err(storage_error)? != stored.collection.id.to_string()
            || row.get::<_, i64>(1).map_err(storage_error)? != stored.collection.revision as i64
            || row.get::<_, i64>(2).map_err(storage_error)?
                != stored.collection.progress.generation as i64
        {
            return Err(storage_error(
                "knowledge metadata does not match its stored definition",
            ));
        }
        entries.push(stored);
    }
    if entries.iter().filter(|stored| !stored.automatic).count() > knowledge::MAX_COLLECTIONS {
        return Err(storage_error("knowledge collection capacity exceeded"));
    }
    Ok(entries)
}

pub(super) fn persist(db: &Connection, stored: &Stored) -> Result<(), Fault> {
    sources::check(stored)?;
    models::require(db, stored.collection.definition.model)?;
    let collection = &stored.collection;
    db.execute("INSERT INTO knowledge_collections(id,revision,generation,body) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,generation=excluded.generation,body=excluded.body",
        params![collection.id.to_string(), collection.revision as i64, collection.progress.generation as i64, encode(stored)?]).map_err(storage_error)?;
    Ok(())
}

pub(super) fn advance(stored: &mut Stored) -> Result<(), Fault> {
    stored.collection.revision = next(stored.collection.revision)?;
    stored.collection.progress = knowledge::Progress {
        generation: next(stored.collection.progress.generation)?,
        ..Default::default()
    };
    Ok(())
}

fn required(
    db: &Connection,
    request: &Request,
    id: KnowledgeId,
    revision: u64,
) -> Result<Stored, Fault> {
    let mut stored = load(db)?
        .into_iter()
        .find(|stored| stored.collection.id == id)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "knowledge collection does not exist"))?;
    if let Some(context) = &request.plugin {
        if !matches!(&stored.collection.owner, Owner::Plugin { name } if name == &context.package.name)
        {
            return Err(denied("knowledge collection belongs to another owner"));
        }
        let package = sources::package(db, context)?;
        if !visible(&stored, request) || !sources::readable(&stored, &package) {
            return Err(denied("knowledge collection is outside the captured scope"));
        }
        stored.capture.package = Some(context.package.clone());
    }
    super::super::commands::check_revision(stored.collection.revision, revision)?;
    Ok(stored)
}

fn output(stored: Stored) -> (Output, Option<Event>) {
    (
        Output::KnowledgeCollection(stored.collection),
        Some(Event::KnowledgeCollectionsChanged),
    )
}

pub(super) fn create(
    db: &Connection,
    request: &Request,
    definition: &Definition,
    selected: Option<&str>,
) -> Result<(Output, Option<Event>), Fault> {
    if request.plugin.is_some() && !definition.shared_with.is_empty() {
        return Err(denied("collection sharing requires a controller"));
    }
    if load(db)?.iter().filter(|stored| !stored.automatic).count() >= knowledge::MAX_COLLECTIONS {
        return Err(Fault::new(
            ErrorCode::Busy,
            "knowledge collection capacity exhausted",
        ));
    }
    let (owner, mut capture) = sources::owner(db, request, selected)?;
    sources::validate(db, request, definition, &mut capture)?;
    let stored = Stored {
        collection: Collection {
            id: KnowledgeId::new(),
            revision: 1,
            rebuild: 0,
            owner,
            definition: definition.clone(),
            paused: false,
            progress: knowledge::Progress {
                generation: 1,
                ..Default::default()
            },
        },
        capture,
        automatic: false,
    };
    persist(db, &stored)?;
    Ok(output(stored))
}

pub(super) fn update(
    db: &Connection,
    request: &Request,
    id: KnowledgeId,
    revision: u64,
    definition: &Definition,
) -> Result<(Output, Option<Event>), Fault> {
    let mut stored = required(db, request, id, revision)?;
    if request.plugin.is_some()
        && stored.collection.definition.shared_with != definition.shared_with
    {
        return Err(denied("collection sharing requires a controller"));
    }
    sources::validate(db, request, definition, &mut stored.capture)?;
    stored.collection.definition = definition.clone();
    advance(&mut stored)?;
    persist(db, &stored)?;
    Ok(output(stored))
}

pub(super) fn rebuild(
    db: &Connection,
    request: &Request,
    id: KnowledgeId,
    revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let mut stored = required(db, request, id, revision)?;
    advance(&mut stored)?;
    stored.collection.rebuild = next(stored.collection.rebuild)?;
    persist(db, &stored)?;
    Ok(output(stored))
}

pub(super) fn pause(
    db: &Connection,
    request: &Request,
    id: KnowledgeId,
    revision: u64,
    paused: bool,
) -> Result<(Output, Option<Event>), Fault> {
    let mut stored = required(db, request, id, revision)?;
    if stored.collection.paused == paused {
        return Ok((Output::KnowledgeCollection(stored.collection), None));
    }
    stored.collection.revision = next(stored.collection.revision)?;
    stored.collection.progress.generation = next(stored.collection.progress.generation)?;
    stored.collection.paused = paused;
    stored.collection.progress.status = if paused {
        knowledge::Status::Paused
    } else {
        knowledge::Status::Pending
    };
    if paused {
        stored.collection.progress.model = None;
    }
    persist(db, &stored)?;
    Ok(output(stored))
}

pub(super) fn remove(
    db: &Connection,
    request: &Request,
    id: KnowledgeId,
    revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let stored = required(db, request, id, revision)?;
    db.execute(
        "DELETE FROM knowledge_collections WHERE id=?1",
        [stored.collection.id.to_string()],
    )
    .map_err(storage_error)?;
    Ok((
        Output::KnowledgeCollectionDeleted { id },
        Some(Event::KnowledgeCollectionsChanged),
    ))
}
