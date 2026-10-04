use super::*;

#[cfg(test)]
mod tests;

pub(crate) struct Work {
    pub job: protocol::JobId,
    pub request: protocol::Request,
    pub completion: Completion,
}

pub(super) fn list(
    db: &Connection,
    package: &str,
    before: Option<u64>,
    limit: u16,
) -> Result<Page, Fault> {
    if !(1..=100).contains(&limit) || before.is_some_and(|n| n == 0 || n > i64::MAX as u64) {
        return Err(invalid("invalid dispatch page bounds"));
    }
    let mut query = db.prepare("SELECT sequence,body FROM dispatch_jobs WHERE package=?1 AND sequence<?2 ORDER BY sequence DESC LIMIT ?3").map_err(storage_error)?;
    let records = query
        .query_map(
            params![
                package,
                before.unwrap_or(i64::MAX as u64) as i64,
                i64::from(limit) + 1
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .map_err(storage_error)?
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

pub(super) fn cancel(db: &Connection, package: &str, id: protocol::JobId) -> Result<Job, Fault> {
    let mut job: Job = required(db, "dispatch_jobs", package, "id", &id.to_string())?;
    if job.status != Status::Queued {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "only queued callbacks can be cancelled",
        ));
    }
    job.status = Status::Cancelled;
    job.finished_ms = Some(now());
    put(db, &job)?;
    prune(db)?;
    Ok(job)
}

pub(super) fn cancel_pending(db: &Connection, package: &str, handler: &str) -> Result<(), Fault> {
    let mut query = db.prepare("SELECT body FROM dispatch_jobs WHERE package=?1 AND status='queued' AND json_extract(body,'$.handler')=?2").map_err(storage_error)?;
    let records = query
        .query_map(params![package, handler], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let timestamp = now();
    for body in records {
        let mut job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
        job.status = Status::Cancelled;
        job.finished_ms = Some(timestamp);
        put(db, &job)?;
    }
    prune(db)
}

pub(super) fn result(
    db: &Connection,
    package: &str,
    id: protocol::JobId,
) -> Result<protocol::RequestOutcome, Fault> {
    let job: Job = required(db, "dispatch_jobs", package, "id", &id.to_string())?;
    let receipt: Option<(String, Option<String>)> = db.query_row(
        "SELECT status,result FROM requests WHERE id=?1 AND caller=(SELECT identity FROM node WHERE singleton=1)",
        [job.request.to_string()], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional().map_err(storage_error)?;
    match receipt {
        Some((status, result)) => super::super::outcomes::resolve(&status, result),
        None => Ok(protocol::RequestOutcome::NotAdmitted),
    }
}

pub(in crate::store) fn turn(
    db: &Connection,
    package: &str,
    id: protocol::JobId,
) -> Result<protocol::TurnId, Fault> {
    if let protocol::RequestOutcome::Completed(output) = result(db, package, id)?
        && let Ok(protocol::Output::QueuedTurn(turn)) = *output
    {
        return Ok(turn.id);
    }
    Err(Fault::new(
        ErrorCode::Conflict,
        "callback has not admitted a turn",
    ))
}

fn put(db: &Connection, job: &Job) -> Result<(), Fault> {
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
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn claim(
    db: &Connection,
    node: protocol::NodeId,
    timestamp: i64,
) -> Result<(Vec<Work>, Vec<String>), Fault> {
    // Bound each store transaction, not the number of callbacks running.
    let mut query = db.prepare("SELECT package,id FROM dispatch_jobs WHERE status='queued' ORDER BY sequence LIMIT 128").map_err(storage_error)?;
    let records = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    let mut work = Vec::new();
    let mut changed = Vec::new();
    for (package, id) in records {
        let (body, callback, reference): (Vec<u8>, Vec<u8>, Vec<u8>) = db
            .query_row(
                "SELECT body,callback,reference FROM dispatch_jobs WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(storage_error)?;
        let mut job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
        let callback: Callback = serde_json::from_slice(&callback).map_err(storage_error)?;
        let reference: Reference = serde_json::from_slice(&reference).map_err(storage_error)?;
        let completion = callback.completion;
        let request = package_version(db, &reference)
            .map(|_| ())
            .and_then(|()| request(node, &job, callback, reference));
        changed.push(package);
        match request {
            Ok(request) => {
                job.status = Status::Running;
                job.started_ms = Some(timestamp);
                put(db, &job)?;
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
                put(db, &job)?;
            }
        }
    }
    if !changed.is_empty() {
        prune(db)?;
    }
    Ok((work, changed))
}

fn package_version(
    db: &Connection,
    reference: &Reference,
) -> Result<protocol::plugin::Info, Fault> {
    current(db, &reference.name)?;
    super::super::agent::plugins::packages(db, std::slice::from_ref(reference))?
        .pop()
        .ok_or_else(|| invalid("dispatch plugin version is unavailable"))
}

/// Only the Node's exact claimed callback can use a queued immutable version.
/// Controller requests cannot borrow the job's authority or replace its input.
pub(in crate::store) fn package(
    db: &Connection,
    node: protocol::NodeId,
    caller: protocol::NodeId,
    submitted: &protocol::Request,
) -> Result<Option<protocol::plugin::Info>, Fault> {
    if caller != node {
        return Ok(None);
    }
    let record: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = db.query_row(
        "SELECT body,callback,reference FROM dispatch_jobs WHERE status='running' AND json_extract(body,'$.request')=?1",
        [submitted.id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional().map_err(storage_error)?;
    let Some((body, callback, reference)) = record else {
        return Ok(None);
    };
    let job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
    let callback: Callback = serde_json::from_slice(&callback).map_err(storage_error)?;
    let reference: Reference = serde_json::from_slice(&reference).map_err(storage_error)?;
    if request(node, &job, callback, reference.clone())? != *submitted {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "dispatch callback does not match its claimed job",
        ));
    }
    package_version(db, &reference).map(Some)
}

fn request(
    node: protocol::NodeId,
    job: &Job,
    callback: Callback,
    package: Reference,
) -> Result<protocol::Request, Fault> {
    let event = serde_json::to_value(&job.event).map_err(storage_error)?;
    let mut command = serde_json::to_value(callback.command).map_err(storage_error)?;
    for (target, source) in callback.bindings {
        let value = event
            .pointer(&source)
            .ok_or_else(|| invalid("event field required by callback is missing"))?
            .clone();
        *command
            .pointer_mut(&target)
            .ok_or_else(|| invalid("callback command field is missing"))? = value;
    }
    let command: protocol::Command = serde_json::from_value(command)
        .map_err(|_| invalid("event data does not match the callback command"))?;
    let mut request = protocol::Request::new(node, command);
    request.id = job.request;
    request.plugin = Some(protocol::plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package,
        worktree: callback.scope.worktree,
        session: callback.scope.session,
    });
    Ok(request)
}

pub(super) fn finish(
    db: &Connection,
    id: protocol::JobId,
    result: Result<(), Fault>,
) -> Result<String, Fault> {
    let (package, body): (String, Vec<u8>) = db
        .query_row(
            "SELECT package,body FROM dispatch_jobs WHERE id=?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(storage_error)?;
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
    put(db, &job)?;
    prune(db)?;
    Ok(package)
}

fn prune(db: &Connection) -> Result<(), Fault> {
    db.execute("DELETE FROM dispatch_jobs WHERE status NOT IN ('queued','running') AND sequence NOT IN (SELECT sequence FROM dispatch_jobs ORDER BY sequence DESC LIMIT 2048)", []).map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn recover(db: &Connection, node: protocol::NodeId) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT body,callback FROM dispatch_jobs WHERE status='running'")
        .map_err(storage_error)?;
    let records = query
        .query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    for (body, callback) in records {
        let job: Job = serde_json::from_slice(&body).map_err(storage_error)?;
        let callback: Callback = serde_json::from_slice(&callback).map_err(storage_error)?;
        let result: Option<Option<Vec<u8>>> = db
            .query_row(
                "SELECT CAST(result AS BLOB) FROM requests WHERE id=?1 AND caller=?2",
                params![job.request.to_string(), &node.0[..]],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let result = match result.flatten() {
            Some(body) => {
                let output = crate::store::outcomes::decode(
                    std::str::from_utf8(&body).map_err(storage_error)?,
                )?;
                match (callback.completion, output) {
                    (Completion::Turn, Ok(protocol::Output::QueuedTurn(turn))) => {
                        match super::super::agent::runs::get(db, turn.id) {
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
                    (Completion::Turn, Ok(_)) => Err(invalid("turn callback did not admit a turn")),
                    (_, output) => output.map(|_| ()),
                }
            }
            None => Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "callback was interrupted; its side effects were not replayed",
            )),
        };
        finish(db, job.id, result)?;
    }
    Ok(())
}

/// Once per Node startup, resume only observations with a committed admission.
pub(in crate::store) fn observers(
    db: &Connection,
    node: protocol::NodeId,
) -> Result<Vec<Work>, Fault> {
    let mut query = db.prepare("SELECT j.body,j.callback,r.body FROM dispatch_jobs j JOIN requests r ON r.id=json_extract(j.body,'$.request') AND r.caller=?1 WHERE j.status='running' AND r.status='completed'").map_err(storage_error)?;
    query
        .query_map([&node.0[..]], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(storage_error)?
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
