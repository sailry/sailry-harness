use super::*;

#[cfg(test)]
mod tests;

pub(crate) struct Work {
    pub job: protocol::JobId,
    pub request: protocol::Request,
    pub completion: Completion,
}

pub(super) async fn list(
    db: &Connection,
    package: &str,
    before: Option<u64>,
    limit: u16,
) -> Result<Page, Fault> {
    if !(1..=100).contains(&limit) || before.is_some_and(|n| n == 0 || n > i64::MAX as u64) {
        return Err(sailry_protocol::Fault::invalid(
            "invalid dispatch page bounds",
        ));
    }
    let mut query = db.prepare("SELECT sequence,body FROM dispatch_jobs WHERE package=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT ?3").await.map_err(storage_error)?;
    let records = {
        let mut rows = query
            .query(params![
                package,
                before.unwrap_or(i64::MAX as u64) as i64,
                i64::from(limit) + 1
            ])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?)))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    let next_before =
        (records.len() > usize::from(limit)).then(|| records[usize::from(limit) - 1].0 as u64);
    let jobs = records
        .into_iter()
        .take(usize::from(limit))
        .map(|(_, body)| serde_json::from_slice(&body).map_err(storage_error))
        .collect::<Result<_, _>>()?;
    Ok(Page { jobs, next_before })
}

pub(super) async fn cancel(
    db: &Connection,
    package: &str,
    id: protocol::JobId,
) -> Result<Job, Fault> {
    let mut job: Job = required(db, "dispatch_jobs", package, "id", &id.to_string()).await?;
    if job.status != Status::Queued {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "only queued callbacks can be cancelled",
        ));
    }
    job.status = Status::Cancelled;
    job.finished_ms = Some(now());
    put(db, &job).await?;
    prune(db).await?;
    Ok(job)
}

pub(super) async fn cancel_pending(
    db: &Connection,
    package: &str,
    handler: &str,
) -> Result<(), Fault> {
    let mut query = db.prepare("SELECT body FROM dispatch_jobs WHERE package=?1 AND status='queued' AND json_extract(body,'$.handler')=?2").await.map_err(storage_error)?;
    let records = {
        let mut rows = query
            .query(params![package, handler])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    let timestamp = now();
    for body in records {
        let mut job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
        job.status = Status::Cancelled;
        job.finished_ms = Some(timestamp);
        put(db, &job).await?;
    }
    prune(db).await
}

pub(super) async fn result(
    db: &Connection,
    package: &str,
    id: protocol::JobId,
) -> Result<protocol::RequestOutcome, Fault> {
    let job: Job = required(db, "dispatch_jobs", package, "id", &id.to_string()).await?;
    let receipt: Option<(String, Option<String>)> = {
        let mut statement = db.prepare("SELECT status,result FROM requests WHERE id=?1 AND caller=(SELECT identity FROM node WHERE singleton=1)").await.map_err(storage_error)?;
        match statement.query_row([job.request.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    match receipt {
        Some((status, result)) => super::super::outcomes::resolve(&status, result),
        None => Ok(protocol::RequestOutcome::NotAdmitted),
    }
}

pub(in crate::store) async fn turn(
    db: &Connection,
    package: &str,
    id: protocol::JobId,
) -> Result<protocol::TurnId, Fault> {
    if let protocol::RequestOutcome::Completed(output) = result(db, package, id).await?
        && let Ok(protocol::Output::QueuedTurn(turn)) = *output
    {
        return Ok(turn.id);
    }
    Err(Fault::new(
        ErrorCode::Conflict,
        "callback has not admitted a turn",
    ))
}

async fn put(db: &Connection, job: &Job) -> Result<(), Fault> {
    let status = match job.status {
        Status::Queued => "queued",
        Status::Running => "running",
        Status::Completed => "completed",
        Status::Failed => "failed",
        Status::Cancelled => "cancelled",
        Status::Unknown => "unknown",
    };
    db.execute(
        "UPDATE dispatch_jobs SET status=?2,body=?3 WHERE id=?1",
        params![job.id.to_string(), status, encode(job)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn claim(
    db: &Connection,
    node: protocol::NodeId,
    timestamp: i64,
) -> Result<(Vec<Work>, Vec<String>), Fault> {
    // Bound each store transaction, not the number of callbacks running.
    let mut query = db.prepare("SELECT package,id FROM dispatch_jobs WHERE status='queued' ORDER BY sequence LIMIT 128").await.map_err(storage_error)?;
    let records = {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<String>(0)?, row.get::<String>(1)?)))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .collect::<Result<Vec<_>, _>>()
    .map_err(storage_error)?;
    let mut work = Vec::new();
    let mut changed = Vec::new();
    for (package, id) in records {
        let (body, callback, reference): (Vec<u8>, Vec<u8>, Vec<u8>) = {
            let mut statement = db
                .prepare("SELECT body,callback,reference FROM dispatch_jobs WHERE id=?1")
                .await
                .map_err(storage_error)?;
            let row = statement.query_row([id]).await.map_err(storage_error)?;
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
            mapped.map_err(storage_error)?
        };
        let mut job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
        let callback: Callback = serde_json::from_slice(&callback).map_err(storage_error)?;
        let reference: Option<Reference> =
            serde_json::from_slice(&reference).map_err(storage_error)?;
        let completion = callback.completion;
        let request = match &reference {
            Some(reference) => package_version(db, reference).await.map(|_| ()),
            None => native::callback(&package, &job, &callback),
        }
        .and_then(|()| request(node, &job, callback, reference));
        changed.push(package);
        match request {
            Ok(request) => {
                job.status = Status::Running;
                job.started_ms = Some(timestamp);
                put(db, &job).await?;
                work.push(Work {
                    job: job.id,
                    request,
                    completion,
                });
            }
            Err(error) => {
                job.status = Status::Failed;
                job.error = Some(error);
                job.finished_ms = Some(timestamp);
                put(db, &job).await?;
            }
        }
    }
    if !changed.is_empty() {
        prune(db).await?;
    }
    Ok((work, changed))
}

async fn package_version(
    db: &Connection,
    reference: &Reference,
) -> Result<protocol::plugin::Info, Fault> {
    current(db, &reference.name).await?;
    super::super::agent::plugins::packages(db, std::slice::from_ref(reference))
        .await?
        .pop()
        .ok_or_else(|| sailry_protocol::Fault::invalid("dispatch plugin version is unavailable"))
}

/// Only the Node's exact claimed callback can use a queued immutable version.
/// Controller requests cannot borrow the job's authority or replace its input.
pub(in crate::store) async fn package(
    db: &Connection,
    node: protocol::NodeId,
    caller: protocol::NodeId,
    submitted: &protocol::Request,
) -> Result<Option<protocol::plugin::Info>, Fault> {
    if caller != node {
        return Ok(None);
    }
    let record: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = {
        let mut statement = db.prepare("SELECT body,callback,reference FROM dispatch_jobs WHERE status='running' AND json_extract(body,'$.request')=?1").await.map_err(storage_error)?;
        match statement.query_row([submitted.id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    let Some((body, callback, reference)) = record else {
        return Ok(None);
    };
    let job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
    let callback: Callback = serde_json::from_slice(&callback).map_err(storage_error)?;
    let reference: Option<Reference> = serde_json::from_slice(&reference).map_err(storage_error)?;
    if request(node, &job, callback.clone(), reference.clone())? != *submitted {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "dispatch callback does not match its claimed job",
        ));
    }
    match reference {
        Some(reference) => package_version(db, &reference).await.map(Some),
        None => {
            native::callback(native::OWNER, &job, &callback)?;
            Ok(None)
        }
    }
}

fn request(
    node: protocol::NodeId,
    job: &Job,
    callback: Callback,
    package: Option<Reference>,
) -> Result<protocol::Request, Fault> {
    let event = serde_json::to_value(&job.event).map_err(storage_error)?;
    let mut command = serde_json::to_value(callback.command).map_err(storage_error)?;
    for (target, source) in callback.bindings {
        let value = event
            .pointer(&source)
            .ok_or_else(|| {
                sailry_protocol::Fault::invalid("event field required by callback is missing")
            })?
            .clone();
        *command.pointer_mut(&target).ok_or_else(|| {
            sailry_protocol::Fault::invalid("callback command field is missing")
        })? = value;
    }
    let command: protocol::Command = serde_json::from_value(command).map_err(|_| {
        sailry_protocol::Fault::invalid("event data does not match the callback command")
    })?;
    let mut request = protocol::Request::new(node, command);
    request.id = job.request;
    request.plugin = package.map(|package| protocol::plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package,
        worktree: callback.scope.worktree,
        session: callback.scope.session,
    });
    Ok(request)
}

pub(super) async fn finish(
    db: &Connection,
    id: protocol::JobId,
    result: Result<(), Fault>,
) -> Result<String, Fault> {
    let (package, body): (String, Vec<u8>) = {
        let mut statement = db
            .prepare("SELECT package,body FROM dispatch_jobs WHERE id=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
        mapped.map_err(storage_error)?
    };
    let mut job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
    if job.status != Status::Running {
        return Err(Fault::new(ErrorCode::Conflict, "callback is not running"));
    }
    job.status = match &result {
        Ok(()) => Status::Completed,
        Err(error) if error.code == ErrorCode::OutcomeUnknown => Status::Unknown,
        Err(error) if error.code == ErrorCode::Cancelled => Status::Cancelled,
        Err(_) => Status::Failed,
    };
    job.error = result.err();
    job.finished_ms = Some(now());
    put(db, &job).await?;
    prune(db).await?;
    Ok(package)
}

async fn prune(db: &Connection) -> Result<(), Fault> {
    db.execute("DELETE FROM dispatch_jobs WHERE status NOT IN ('queued','running') AND sequence NOT IN (SELECT sequence FROM dispatch_jobs ORDER BY sequence DESC LIMIT 2048)", ()).await.map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn recover(
    db: &Connection,
    node: protocol::NodeId,
) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT body,callback FROM dispatch_jobs WHERE status='running'")
        .await
        .map_err(storage_error)?;
    let records = {
        let mut rows = query.query(()).await.map_err(storage_error)?;
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
    for (body, callback) in records {
        let job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
        let callback: Callback = serde_json::from_slice(&callback).map_err(storage_error)?;
        let result: Option<Option<Vec<u8>>> = {
            let mut statement = db
                .prepare("SELECT CAST(result AS BLOB) FROM requests WHERE id=?1 AND caller=?2")
                .await
                .map_err(storage_error)?;
            match statement
                .query_row(params![job.request.to_string(), &node.0[..]])
                .await
            {
                Ok(row) => {
                    let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error).into()),
            }
        };
        let result = match result.flatten() {
            Some(body) => {
                let output = crate::store::outcomes::decode(
                    std::str::from_utf8(&body).map_err(storage_error)?,
                )?;
                match (callback.completion, output) {
                    (Completion::Turn, Ok(protocol::Output::QueuedTurn(turn))) => {
                        match super::super::agent::runs::get(db, turn.id).await {
                            Err(error) => Err(error),
                            Ok(run) => {
                                if let Some(result) = crate::dispatch::turn_result(run.status) {
                                    result.map_err(|error| run.error.unwrap_or(error))
                                } else {
                                    // Re-read a known completed admission using its original ID.
                                    // This resumes observation, never replays an uncertain effect.
                                    continue;
                                }
                            }
                        }
                    }
                    (Completion::Turn, Ok(_)) => Err(sailry_protocol::Fault::invalid(
                        "turn callback did not admit a turn",
                    )),
                    (_, output) => output.map(|_| ()),
                }
            }
            None => Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "callback was interrupted; its side effects were not replayed",
            )),
        };
        finish(db, job.id, result).await?;
    }
    Ok(())
}

/// Once per Node startup, resume only observations with a committed admission.
pub(in crate::store) async fn observers(
    db: &Connection,
    node: protocol::NodeId,
) -> Result<Vec<Work>, Fault> {
    let mut query = db.prepare("SELECT j.body,j.callback,r.body FROM dispatch_jobs j JOIN requests r ON r.id=json_extract(j.body,'$.request') AND r.caller=?1 WHERE j.status='running' AND r.status='completed'").await.map_err(storage_error)?;
    {
        let mut rows = query.query([&node.0[..]]).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| {
                Ok((
                    row.get::<Vec<u8>>(0)?,
                    row.get::<Vec<u8>>(1)?,
                    row.get::<Vec<u8>>(2)?,
                ))
            })(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| {
        let (job, callback, request) = row.map_err(storage_error)?;
        let job: Job = serde_json::from_slice(&job).map_err(storage_error)?;
        let callback: Callback = serde_json::from_slice(&callback).map_err(storage_error)?;
        Ok(Work {
            job: job.id,
            request: serde_json::from_slice(&request).map_err(storage_error)?,
            completion: callback.completion,
        })
    })
    .collect()
}
