use super::*;
use sailry_protocol::{
    conversation::{compaction::Record, observation::Summary},
    usage::Metrics,
};
use std::collections::BTreeSet;

mod cache;
mod compaction;
mod observations;
mod publication;
pub(in crate::store) use publication::{publish, request};
pub(in crate::store) mod report;
pub(in crate::store) use cache::Cache;

/// Event-derived values, reusable while the session's canonical stamp is unchanged.
/// Queue, run and configuration state is read again for every publication.
#[derive(Clone)]
struct Canonical {
    observations: Summary,
    observation_state: observations::State,
    phased: BTreeSet<TurnId>,
    finished: BTreeSet<TurnId>,
    metrics: Metrics,
    delegated: Metrics,
    observation: Option<Observation>,
    context_estimate: Option<u64>,
    last_compaction: Option<Record>,
}

#[cfg(test)]
pub(super) async fn read(db: &Connection, session: SessionId) -> Result<Statistics, Fault> {
    check_session(db, session).await?;
    let canonical = canonical(db, session).await?;
    Ok(finish(db, session, &canonical).await?.0)
}

/// Reuses unchanged canonical aggregates. Callers retain the entry once its snapshot is committed.
pub(super) async fn prepare(
    cache: &Cache,
    db: &Connection,
    session: SessionId,
) -> Result<(Statistics, cache::Entry), Fault> {
    check_session(db, session).await?;
    let stamp = cache::stamp(db, session).await?;
    let mut canonical = match cache.find(session, &stamp) {
        Some(canonical) => canonical,
        None => std::sync::Arc::new(match cache.previous(session, &stamp) {
            Some((after, previous)) => advance(db, session, after, Some(&previous)).await?,
            None => canonical(db, session).await?,
        }),
    };
    let (statistics, finished) = finish(db, session, &canonical).await?;
    if canonical.finished != finished {
        std::sync::Arc::make_mut(&mut canonical).finished = finished;
    }
    Ok((statistics, cache::Entry::new(session, stamp, canonical)))
}

/// Reads statistics from a committed snapshot and retains its canonical aggregates.
#[cfg(test)]
pub(super) async fn current(
    cache: &Cache,
    db: &Connection,
    session: SessionId,
) -> Result<Statistics, Fault> {
    let (statistics, entry) = prepare(cache, db, session).await?;
    cache.retain(entry);
    Ok(statistics)
}

async fn canonical(db: &Connection, session: SessionId) -> Result<Canonical, Fault> {
    advance(db, session, 0, None).await
}

async fn advance(
    db: &Connection,
    session: SessionId,
    after: i64,
    previous: Option<&Canonical>,
) -> Result<Canonical, Fault> {
    let (metrics, delegated) = report::session::advance(
        db,
        session,
        after,
        previous
            .map(|value| value.metrics.clone())
            .unwrap_or_default(),
        previous
            .map(|value| value.delegated.clone())
            .unwrap_or_default(),
    )
    .await?;
    let observation = advance_observation(
        db,
        session,
        after,
        previous.and_then(|value| value.observation.clone()),
    )
    .await?;
    let context_estimate = if let Some(observation) = observation
        .as_ref()
        .filter(|value| value.summary || value.estimate.is_some())
    {
        context_usage_from(db, session, observation)
            .await?
            .map(|usage| {
                usage
                    .tokens
                    .saturating_add(crate::agent::estimate_context(&usage.pending))
            })
    } else {
        None
    };
    let last_compaction = compaction::since(db, session, after)
        .await?
        .or_else(|| previous.and_then(|value| value.last_compaction.clone()));
    let (observations, phased, observation_state) = observations::advance(
        db,
        session,
        after,
        previous
            .map(|value| value.observation_state.clone())
            .unwrap_or_default(),
    )
    .await?;
    Ok(Canonical {
        observations,
        observation_state,
        phased,
        finished: previous
            .map(|value| value.finished.clone())
            .unwrap_or_default(),
        metrics,
        delegated,
        observation,
        context_estimate,
        last_compaction,
    })
}

async fn finish(
    db: &Connection,
    session: SessionId,
    canonical: &Canonical,
) -> Result<(Statistics, BTreeSet<TurnId>), Fault> {
    let turns = {
        let mut statement = db.prepare("SELECT count(*) FROM conversation_turns h JOIN turns t ON t.id=h.turn WHERE h.session=?1 AND t.kind='task'
         AND EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=h.turn)").await.map_err(storage_error)?;
        statement
            .query_row([session.to_string()])
            .await
            .and_then(|row| row.get::<i64>(0))
            .map_err(storage_error)?
    };
    let Canonical {
        observations,
        observation_state: _,
        phased,
        finished,
        metrics,
        delegated,
        observation,
        context_estimate,
        last_compaction,
    } = canonical;
    let budget = compaction::budget(db, session).await?;
    let mut finished = finished.clone();
    let observations =
        observations::complete(db, session, observations.clone(), phased, &mut finished).await?;
    Ok((
        Statistics {
            observations,
            turns: turns as u64,
            responses: metrics.responses,
            usage: metrics.tokens.clone(),
            context_tokens: observation.as_ref().and_then(|value| value.tokens),
            context_estimate: *context_estimate,
            context_limit: observation.as_ref().and_then(|value| value.context_limit),
            context_model: observation.as_ref().and_then(|value| value.model.clone()),
            context_timestamp_ms: observation.as_ref().and_then(|value| value.timestamp_ms),
            budget,
            last_compaction: last_compaction.clone(),
            first_token_us: observation.as_ref().and_then(|value| value.first_token_us),
            cost: metrics.cost.clone(),
            generation: metrics.generation.clone(),
            delegated: delegated.clone(),
        },
        finished,
    ))
}

pub(super) async fn parents(db: &Connection, turn: TurnId) -> Result<Vec<SessionId>, Fault> {
    let mut query = db
        .prepare(
            "SELECT DISTINCT h.session FROM agent_delegations d
         JOIN conversation_turns h ON h.turn=d.parent_turn JOIN sessions s ON s.id=h.session
         WHERE d.turn=?1 AND s.state!='removed'",
        )
        .await
        .map_err(storage_error)?;
    let mut rows = query
        .query([turn.to_string()])
        .await
        .map_err(storage_error)?;
    let mut parents = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let session = row.get::<String>(0).map_err(storage_error)?;
        parents.push(session.parse().map_err(storage_error)?);
    }
    Ok(parents)
}

#[derive(Clone)]
struct Observation {
    summary: bool,
    estimate: Option<u64>,
    sequence: i64,
    tokens: Option<u64>,
    context_limit: Option<u32>,
    model: Option<String>,
    timestamp_ms: Option<i64>,
    first_token_us: Option<u64>,
}

#[cfg(test)]
pub(super) async fn context_usage(
    db: &Connection,
    session: SessionId,
) -> Result<Option<ContextUsage>, Fault> {
    let Some(observation) = observation(db, session).await? else {
        return Ok(None);
    };
    context_usage_from(db, session, &observation).await
}

async fn context_usage_from(
    db: &Connection,
    session: SessionId,
    observation: &Observation,
) -> Result<Option<ContextUsage>, Fault> {
    let (sequence, tokens, summary) = match observation.tokens {
        Some(tokens) => (observation.sequence, tokens, None),
        None if observation.estimate.is_some() => {
            (observation.sequence, observation.estimate.unwrap(), None)
        }
        None if observation.summary => {
            let Some((boundary, summary)) =
                super::compaction::read(db, session).await.map_err(|_| {
                    Fault::new(ErrorCode::Internal, "context summary could not be read")
                })?
            else {
                return Ok(None);
            };
            (boundary, 0, Some(summary))
        }
        None => return Ok(None),
    };
    // Both reads use the caller's transaction. Only new canonical content is decoded;
    // the observation already includes its model response's output tokens.
    let mut query = db
        .prepare(&format!(
            "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND h.sequence>?2
           AND json_extract(e.body,'$.kind')!='compaction'
           AND json_extract(e.body,'$.metadata.execution.media') IS NULL
           AND coalesce(json_extract(e.body,'$.metadata.execution.purpose'),'') NOT IN ('compaction','search')
           AND {} ORDER BY h.sequence",
            super::history::VISIBLE_EVENT
        ))
        .await
        .map_err(storage_error)?;
    let mut rows = query
        .query(params![session.to_string(), sequence])
        .await
        .map_err(storage_error)?;
    let mut pending = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body = row.get::<Vec<u8>>(0).map_err(storage_error)?;
        let record: crate::agent::canonical::Record =
            serde_json::from_slice(&body).map_err(storage_error)?;
        pending.push(record);
    }
    if let Some(summary) = summary {
        pending.insert(0, summary);
    }
    Ok(Some(ContextUsage {
        #[cfg(test)]
        observed: observation.tokens.is_some(),
        tokens,
        pending,
    }))
}

#[cfg(test)]
async fn observation(db: &Connection, session: SessionId) -> Result<Option<Observation>, Fault> {
    advance_observation(db, session, 0, None).await
}

async fn advance_observation(
    db: &Connection,
    session: SessionId,
    after: i64,
    previous: Option<Observation>,
) -> Result<Option<Observation>, Fault> {
    let latest = read_observation(db, session, false, after)
        .await?
        .or(previous.clone());
    if latest
        .as_ref()
        .is_some_and(|value| value.tokens.is_none() && !value.summary && value.estimate.is_none())
        && let Some(reset) = read_observation(db, session, true, after)
            .await?
            .or(previous)
        && (reset.summary || reset.estimate.is_some())
    {
        // An unmetered response cannot replace a post-compaction estimate with
        // a real observation. Keep estimating the growing canonical window.
        return Ok(Some(reset));
    }
    Ok(latest)
}

async fn read_observation(
    db: &Connection,
    session: SessionId,
    measured: bool,
    after: i64,
) -> Result<Option<Observation>, Fault> {
    // Read the last model observation in canonical order. A summary invalidates
    // the old window measurement; its own prompt is not the resumed context.
    let mut statement = db.prepare("SELECT h.sequence, CASE WHEN json_extract(e.body,'$.kind')!='compaction'
          AND json_extract(e.body,'$.usage.input') >= 0
          AND json_extract(e.body,'$.usage.output') >= 0
         THEN json_extract(e.body,'$.usage.input')
            + json_extract(e.body,'$.usage.output') END,
         CASE WHEN json_extract(e.body,'$.kind')!='compaction'
          AND json_extract(e.body,'$.usage.input') >= 0
          AND json_extract(e.body,'$.usage.output') >= 0
          AND json_type(e.body,'$.metadata.execution.timing.first_token_us')='integer'
          AND json_extract(e.body,'$.metadata.execution.timing.first_token_us') >= 0
         THEN json_extract(e.body,'$.metadata.execution.timing.first_token_us') END,
         (SELECT json_extract(model.value,'$.context') FROM json_each(r.provider,'$.models') model
          WHERE json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1),
         coalesce(json_extract(e.body,'$.kind')='compaction',0),
         json_extract(revision.config,'$.model'),json_extract(e.body,'$.created'),
         CASE WHEN json_type(e.body,'$.metadata.execution.context_tokens')='integer'
           AND json_extract(e.body,'$.metadata.execution.context_tokens')>=0
         THEN json_extract(e.body,'$.metadata.execution.context_tokens') END
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         LEFT JOIN agent_runs r ON r.turn=e.turn LEFT JOIN turns t ON t.id=e.turn
         LEFT JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision
         WHERE h.session=?1 AND h.sequence>?3 AND json_extract(e.body,'$.metadata.execution.media') IS NULL
           AND coalesce(json_extract(e.body,'$.metadata.execution.purpose'),'')!='search'
           AND (coalesce(json_extract(e.body,'$.metadata.execution.purpose'),'')!='compaction'
             OR json_extract(e.body,'$.kind')='compaction')
           AND ((json_extract(e.body,'$.role')='assistant' AND json_extract(e.body,'$.part_count')>0)
           OR json_type(e.body,'$.usage')='object'
           OR json_extract(e.body,'$.kind')='compaction'
           OR json_type(e.body,'$.metadata.execution.context_tokens')='integer')
           AND (?2=0 OR json_extract(e.body,'$.kind')='compaction'
             OR json_type(e.body,'$.metadata.execution.context_tokens')='integer'
             OR (json_extract(e.body,'$.usage.input')>=0
                 AND json_extract(e.body,'$.usage.output')>=0))
         ORDER BY h.sequence DESC LIMIT 1").await.map_err(storage_error)?;
    let row = match statement
        .query_row(params![session.to_string(), measured, after])
        .await
    {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let tokens = row
        .get::<Option<i64>>(1)
        .map_err(storage_error)?
        .map(|value| value as u64);
    let summary = row.get::<bool>(4).map_err(storage_error)?;
    let estimate = row
        .get::<Option<i64>>(7)
        .map_err(storage_error)?
        .map(|value| value as u64);
    let measured = tokens.is_some() || summary || estimate.is_some();
    Ok(Some(Observation {
        summary,
        estimate,
        sequence: row.get(0).map_err(storage_error)?,
        context_limit: if measured {
            row.get::<Option<u32>>(3)
                .map_err(storage_error)?
                .filter(|limit| *limit > 0)
        } else {
            None
        },
        model: if measured {
            row.get(5).map_err(storage_error)?
        } else {
            None
        },
        timestamp_ms: if measured {
            row.get::<Option<String>>(6)
                .map_err(storage_error)?
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
                .map(|value| value.timestamp_millis())
        } else {
            None
        },
        tokens,
        first_token_us: row
            .get::<Option<i64>>(2)
            .map_err(storage_error)?
            .map(|value| value as u64),
    }))
}

#[cfg(test)]
mod tests;
