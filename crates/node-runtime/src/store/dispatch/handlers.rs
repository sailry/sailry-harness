use super::*;

pub(super) fn save(
    db: &Connection,
    reference: &Reference,
    handler: &Handler,
) -> Result<Handler, Fault> {
    let package = &reference.name;
    name(&handler.name)?;
    name(&handler.source.package)?;
    name(&handler.source.topic)?;
    name(&handler.queue)?;
    data(&handler.callback)?;
    if matches!(
        *handler.callback.command,
        protocol::Command::Dispatch { .. }
    ) {
        return Err(invalid(
            "dispatch callbacks cannot recursively schedule dispatch commands",
        ));
    }
    if handler.callback.bindings.len() > 32 {
        return Err(invalid("too many event bindings"));
    }
    let command = serde_json::to_value(&handler.callback.command).map_err(storage_error)?;
    for (target, source) in &handler.callback.bindings {
        if !target.starts_with("/data/")
            || !source.starts_with('/')
            || source.len() > 512
            || target.len() > 512
            || command.pointer(target).is_none()
        {
            return Err(invalid(
                "event binding must point to an existing command data field",
            ));
        }
    }
    let old: Option<Handler> = get(db, "dispatch_handlers", package, "name", &handler.name)?;
    check_revision(old.as_ref().map_or(0, |old| old.revision), handler.revision)?;
    if old.is_none() {
        capacity(db, "dispatch_handlers", package)?;
    }
    let mut saved = handler.clone();
    saved.revision += 1;
    budget(
        db,
        "dispatch_handlers",
        package,
        "name",
        &saved.name,
        &saved,
    )?;
    db.execute("INSERT INTO dispatch_handlers(package,name,source,topic,body,reference) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(package,name) DO UPDATE SET source=excluded.source,topic=excluded.topic,body=excluded.body,reference=excluded.reference", params![package, saved.name, saved.source.package, saved.source.topic, encode(&saved)?, encode(reference)?]).map_err(storage_error)?;
    Ok(saved)
}
