use super::*;

pub(super) fn read(db: &Connection) -> Result<knowledge::Settings, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM knowledge_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let settings = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?
        .unwrap_or_default();
    validate(&settings)?;
    models::require(db, settings.model)?;
    Ok(settings)
}

fn validate(settings: &knowledge::Settings) -> Result<(), Fault> {
    if settings.revision > i64::MAX as u64 {
        return Err(Fault::invalid("invalid knowledge settings revision"));
    }
    if let Some(mirror) = &settings.mirror {
        let url = url::Url::parse(mirror).map_err(|_| Fault::invalid("invalid model mirror"))?;
        if mirror.len() > 4096
            || url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Fault::invalid(
                "model mirror requires an HTTPS URL without credentials",
            ));
        }
    }
    Ok(())
}

pub(super) fn save(
    db: &Connection,
    settings: &knowledge::Settings,
) -> Result<(Output, Option<Event>), Fault> {
    let previous = read(db)?;
    super::super::commands::check_revision(previous.revision, settings.revision)?;
    validate(settings)?;
    models::require(db, settings.model)?;
    let mut updated = settings.clone();
    updated.revision = next(updated.revision)?;
    if previous.model != settings.model {
        let mut collections = collections::load(db)?;
        for stored in &mut collections {
            if stored.collection.definition.model.is_none() {
                collections::advance(stored)?;
            }
        }
        for stored in &collections {
            if stored.collection.definition.model.is_none() {
                collections::persist(db, stored)?;
            }
        }
    }
    db.execute("INSERT INTO knowledge_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&updated)?])
        .map_err(storage_error)?;
    Ok((
        Output::KnowledgeSettings(updated.clone()),
        Some(Event::KnowledgeSettingsChanged {
            revision: updated.revision,
        }),
    ))
}
