use super::*;

pub(super) async fn publish(
    db: &Connection,
    event: &Event,
) -> Result<(Vec<protocol::JobId>, Vec<String>), Fault> {
    let mut query = db.prepare("SELECT body,reference FROM dispatch_handlers WHERE source=?1 AND topic=?2 ORDER BY rowid").await.map_err(storage_error)?;
    let handlers = {
        let mut rows = query
            .query(params![
                event.source.package.as_str(),
                event.source.topic.as_str()
            ])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?)))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    let mut jobs = Vec::new();
    let mut packages = vec![event.source.package.clone()];
    for (body, reference) in handlers {
        let handler: Handler = serde_json::from_slice(&body).map_err(storage_error)?;
        let reference: Option<Reference> =
            serde_json::from_slice(&reference).map_err(storage_error)?;
        if !handler.enabled {
            continue;
        }
        match reference {
            Some(reference) => {
                if let Some(installed) = super::super::plugins::get(db, &reference.name).await?
                    && installed.summary.enabled
                {
                    let reference = installed.summary.reference();
                    jobs.push(enqueue(db, Some(&reference), &handler, event).await?.id);
                    packages.push(reference.name);
                }
            }
            None => {
                native::handler(&handler)?;
                jobs.push(enqueue(db, None, &handler, event).await?.id);
            }
        }
    }
    db.execute(
        "INSERT INTO dispatch_events(id,body,sequence) VALUES(?1,?2,?3)",
        params![
            event.id.to_string(),
            encode(event)?,
            crate::store::database::counters::next(
                db,
                crate::store::database::counters::Namespace::DispatchEvents
            )
            .await? as i64
        ],
    )
    .await
    .map_err(storage_error)?;
    // Each delivery owns its immutable envelope; a bounded source log is not
    // required for replaying already admitted callbacks.
    db.execute("DELETE FROM dispatch_events WHERE sequence NOT IN (SELECT sequence FROM dispatch_events ORDER BY sequence DESC LIMIT 2048)", ()).await.map_err(storage_error)?;
    packages.sort();
    packages.dedup();
    Ok((jobs, packages))
}

pub(super) async fn enqueue(
    db: &Connection,
    package: Option<&Reference>,
    handler: &Handler,
    event: &Event,
) -> Result<Job, Fault> {
    let pending: i64 = {
        let mut statement = db
            .prepare("SELECT count(*) FROM dispatch_jobs WHERE status='queued'")
            .await
            .map_err(storage_error)?;
        let row = statement.query_row(()).await.map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
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
    db.execute("INSERT INTO dispatch_jobs(id,package,queue,status,body,callback,reference,sequence) VALUES(?1,?2,?3,'queued',?4,?5,?6,?7)", params![job.id.to_string(), package.map_or(native::OWNER, |reference| reference.name.as_str()), job.queue.as_str(), encode(&job)?, encode(&handler.callback)?, encode(&package)?, crate::store::database::counters::next(db, crate::store::database::counters::Namespace::DispatchJobs).await? as i64]).await.map_err(storage_error)?;
    Ok(job)
}
