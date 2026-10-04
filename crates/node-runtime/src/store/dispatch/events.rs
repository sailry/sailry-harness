use super::*;

pub(super) fn publish(
    db: &Connection,
    event: &Event,
) -> Result<(Vec<protocol::JobId>, Vec<String>), Fault> {
    let mut query = db.prepare("SELECT body,reference FROM dispatch_handlers WHERE source=?1 AND topic=?2 ORDER BY rowid").map_err(storage_error)?;
    let handlers = query
        .query_map(params![event.source.package, event.source.topic], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let mut jobs = Vec::new();
    let mut packages = vec![event.source.package.clone()];
    for (body, reference) in handlers {
        let handler: Handler = serde_json::from_slice(&body).map_err(storage_error)?;
        let reference: Reference = serde_json::from_slice(&reference).map_err(storage_error)?;
        if handler.enabled
            && let Some(installed) = super::super::plugins::get(db, &reference.name)?
            && installed.summary.enabled
        {
            let reference = installed.summary.reference();
            jobs.push(enqueue(db, &reference, &handler, event)?.id);
            packages.push(reference.name);
        }
    }
    db.execute(
        "INSERT INTO dispatch_events(id,body) VALUES(?1,?2)",
        params![event.id.to_string(), encode(event)?],
    )
    .map_err(storage_error)?;
    // Each delivery owns its immutable envelope; a bounded source log is not
    // required for replaying already admitted callbacks.
    db.execute("DELETE FROM dispatch_events WHERE sequence NOT IN (SELECT sequence FROM dispatch_events ORDER BY sequence DESC LIMIT 2048)", []).map_err(storage_error)?;
    packages.sort();
    packages.dedup();
    Ok((jobs, packages))
}

pub(super) fn enqueue(
    db: &Connection,
    package: &Reference,
    handler: &Handler,
    event: &Event,
) -> Result<Job, Fault> {
    let pending: i64 = db
        .query_row(
            "SELECT count(*) FROM dispatch_jobs WHERE status='queued'",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if pending >= MAX_PENDING as i64 {
        return Err(Fault::new(
            ErrorCode::Busy,
            "pending dispatch capacity exhausted",
        ));
    }
    let job = Job {
        id: protocol::JobId::new(),
        event: event.clone(),
        handler: handler.name.clone(),
        queue: handler.queue.clone(),
        request: protocol::RequestId::new(),
        status: Status::Queued,
        created_ms: now(),
        started_ms: None,
        finished_ms: None,
        error: None,
    };
    db.execute("INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference) VALUES(?1,?2,?3,'queued',?4,?5,?6)", params![job.id.to_string(), package.name, job.queue, encode(&job)?, encode(&handler.callback)?, encode(package)?]).map_err(storage_error)?;
    Ok(job)
}
