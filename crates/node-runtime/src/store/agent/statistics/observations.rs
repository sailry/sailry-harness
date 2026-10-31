//! Read lightweight observations through the canonical conversation membership.
use super::*;
use sailry_protocol::conversation::observation::{
    Attempt, Category, Latency, RECENT_LIMIT, Request, Summary,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Mean {
    total: u128,
    count: u64,
}

impl Mean {
    fn observe(&mut self, value: Option<u64>) {
        if let Some(value) = value {
            self.total += u128::from(value);
            self.count += 1;
        }
    }

    fn finish(self) -> Option<u64> {
        (self.count > 0).then(|| {
            u64::try_from(self.total / u128::from(self.count)).expect("mean fits its sample range")
        })
    }
}

#[derive(Default)]
struct Latencies {
    response: Mean,
    first: Mean,
    waiting: Mean,
    elapsed: Mean,
}

impl Latencies {
    fn observe(&mut self, attempt: &Attempt) {
        let within = |value: Option<u64>| {
            value.filter(|value| attempt.elapsed_us.is_none_or(|elapsed| *value <= elapsed))
        };
        let response = within(attempt.response_us.or(if attempt.transport == "http" {
            attempt.headers_us
        } else {
            None
        }));
        // SDK receipt and end share a clock. Node may receive the first text
        // after that semantic terminal, so delivery is not bounded by SDK end.
        let first = attempt.first_text_us;
        self.response.observe(response);
        self.first.observe(first);
        self.waiting.observe(
            first
                .zip(response)
                .and_then(|(first, response)| first.checked_sub(response)),
        );
        self.elapsed.observe(attempt.elapsed_us);
    }

    fn finish(self) -> Latency {
        Latency {
            response_us: self.response.finish(),
            first_text_us: self.first.finish(),
            waiting_us: self.waiting.finish(),
            elapsed_us: self.elapsed.finish(),
        }
    }
}

const EVENTS: &str = "SELECT e.turn,json_object(
         'kind',json_extract(e.body,'$.kind'),
         'phases',json_extract(e.body,'$.metadata.execution.phases'),
         'usage',coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')),
         'has_reply',json_extract(e.body,'$.role')='assistant' AND EXISTS(
             SELECT 1 FROM json_each(e.body,'$.content') p WHERE length(json_extract(p.value,'$.text'))>0),
         'tools',(SELECT json_group_array(json_extract(p.value,'$.name'))
             FROM json_each(e.body,'$.content') p WHERE json_extract(p.value,'$.type')='tool_request'
             AND json_type(p.value,'$.name')='text' AND json_type(p.value,'$.arguments')='object')),
         EXISTS(SELECT 1 FROM conversation_events membership
             WHERE membership.session=?1 AND membership.sequence=e.sequence)
         FROM agent_events e WHERE e.sequence>?2 AND EXISTS(
             SELECT 1 FROM conversation_turns h WHERE h.session=?1 AND h.turn=e.turn)
         ORDER BY e.sequence";

#[derive(Clone, Default)]
pub(super) struct State {
    calls: BTreeMap<String, (u64, Request)>,
    phases: BTreeMap<TurnId, sailry_protocol::conversation::observation::Phases>,
    replies: BTreeSet<TurnId>,
    tools: BTreeMap<String, u64>,
    compactions: u64,
    usage_count: u64,
    cache_count: u64,
    cache_total: u64,
    reasoning_count: u64,
    reasoning_total: u64,
    #[cfg(test)]
    pub(super) examined: u64,
}

#[cfg(test)]
pub(super) async fn read(db: &Connection, session: SessionId) -> Result<Summary, Fault> {
    let (summary, phased) = canonical(db, session).await?;
    complete(db, session, summary, &phased).await
}

/// Phase totals stay valid only while every finished turn recorded phases.
/// Run states change without new events, so this check is never cached.
pub(super) async fn complete(
    db: &Connection,
    session: SessionId,
    mut summary: Summary,
    phased: &BTreeSet<TurnId>,
) -> Result<Summary, Fault> {
    let mut statement = db
        .prepare("SELECT h.turn FROM conversation_turns h JOIN agent_runs r ON r.turn=h.turn WHERE h.session=?1 AND json_extract(r.body,'$.status') IN ('completed','failed','cancelled','interrupted')")
        .await
        .map_err(storage_error)?;
    let mut rows = statement
        .query([session.to_string()])
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let turn: TurnId = row
            .get::<String>(0)
            .map_err(storage_error)?
            .parse()
            .map_err(storage_error)?;
        if !phased.contains(&turn) {
            summary.phases = Default::default();
            break;
        }
    }
    Ok(summary)
}

/// Event-derived observations and the turns that recorded phase totals.
#[cfg(test)]
pub(super) async fn canonical(
    db: &Connection,
    session: SessionId,
) -> Result<(Summary, BTreeSet<TurnId>), Fault> {
    let (summary, phased, _) = advance(db, session, 0, State::default()).await?;
    Ok((summary, phased))
}

pub(super) async fn advance(
    db: &Connection,
    session: SessionId,
    after: i64,
    mut state: State,
) -> Result<(Summary, BTreeSet<TurnId>, State), Fault> {
    let mut result = Summary::default();
    let mut phased = BTreeSet::new();
    let calls = &mut state.calls;
    let mut composition = BTreeMap::new();
    let mut query = db.prepare(
        "WITH rounds AS (
           SELECT t.id,row_number() OVER (ORDER BY t.rowid) AS turn_index
           FROM conversation_turns membership JOIN turns t ON t.id=membership.turn
           WHERE membership.session=?1 AND t.kind='task'
           AND EXISTS(SELECT 1 FROM agent_events event WHERE event.turn=t.id)
         )
         SELECT h.sequence,json_extract(e.body,'$.metadata.execution.observation'),
          (SELECT json_extract(model.value,'$.context') FROM agent_runs r
           JOIN turns t ON t.id=r.turn
           JOIN session_revisions revision ON revision.session=t.session AND revision.revision=t.revision,
           json_each(r.provider,'$.models') model WHERE r.turn=e.turn
           AND json_extract(model.value,'$.id')=json_extract(revision.config,'$.model') LIMIT 1),
          (SELECT turn_index FROM rounds WHERE rounds.id=e.turn)
         FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND h.sequence>?2 AND json_type(e.body,'$.metadata.execution.observation')='object'
         ORDER BY h.sequence"
    ).await.map_err(storage_error)?;
    let mut rows = query
        .query(params![session.to_string(), after])
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let sequence =
            u64::try_from(row.get::<i64>(0).map_err(storage_error)?).map_err(storage_error)?;
        let encoded = row.get::<String>(1).map_err(storage_error)?;
        let mut request: Request = serde_json::from_str(&encoded).map_err(storage_error)?;
        request.context_limit = row.get(2).map_err(storage_error)?;
        request.turn_index = row
            .get::<Option<i64>>(3)
            .map_err(storage_error)?
            .map(u64::try_from)
            .transpose()
            .map_err(storage_error)?;
        let (_, current) = calls.entry(request.id.clone()).or_insert_with(|| {
            let mut current = request.clone();
            current.attempts.clear();
            (sequence, current)
        });
        // Each event may contain a subset of this v1 request's attempts. The
        // same reducer also accepts repeated snapshots without double counting.
        for attempt in request.attempts {
            if let Some(previous) = current
                .attempts
                .iter_mut()
                .find(|value| value.index == attempt.index)
            {
                if previous.elapsed_us.is_none() || attempt.elapsed_us.is_some() {
                    *previous = attempt;
                }
            } else {
                current.attempts.push(attempt);
            }
        }
        current.attempts.sort_by_key(|attempt| attempt.index);
    }
    drop(rows);
    drop(query);
    let mut query = db.prepare(EVENTS).await.map_err(storage_error)?;
    let mut rows = query
        .query(params![session.to_string(), after])
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        #[cfg(test)]
        {
            state.examined += 1;
        }
        let turn: TurnId = row
            .get::<String>(0)
            .map_err(storage_error)?
            .parse()
            .map_err(storage_error)?;
        let body: serde_json::Value =
            serde_json::from_str(&row.get::<String>(1).map_err(storage_error)?)
                .map_err(storage_error)?;
        let visible = row.get::<bool>(2).map_err(storage_error)?;
        if let Some(phases) = body.get("phases").filter(|v| v.is_object()) {
            state.phases.insert(
                turn,
                serde_json::from_value(phases.clone()).map_err(storage_error)?,
            );
        }
        if let Some(usage) = body
            .get("usage")
            .filter(|v| !v.is_null())
            .or_else(|| body.pointer("/summary/usage"))
            .filter(|v| v.is_object())
        {
            state.usage_count += 1;
            if let Some(value) = usage["cache_read"].as_u64() {
                state.cache_count += 1;
                state.cache_total = state.cache_total.saturating_add(value);
            }
            if let Some(value) = usage["reasoning"].as_u64() {
                state.reasoning_count += 1;
                state.reasoning_total = state.reasoning_total.saturating_add(value);
            }
        }
        if !visible {
            continue;
        }
        if body["kind"] == "compaction" {
            state.compactions += 1;
        }
        if body["has_reply"] == 1 {
            state.replies.insert(turn);
        }
        for name in body["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
        {
            *state.tools.entry(name.to_owned()).or_default() += 1;
        }
    }
    drop(rows);
    drop(query);
    result.compactions = state.compactions;
    result.replies = state.replies.len() as u64;
    result.tools = state.tools.clone();
    result.cache_read = (state.usage_count > 0 && state.cache_count == state.usage_count)
        .then_some(state.cache_total);
    result.reasoning = (state.usage_count > 0 && state.reasoning_count == state.usage_count)
        .then_some(state.reasoning_total);
    for (index, (turn, value)) in state.phases.iter().enumerate() {
        phased.insert(*turn);
        if index == 0 {
            result.phases = value.clone();
        } else {
            for (sum, next) in [
                (&mut result.phases.active_us, value.active_us),
                (&mut result.phases.waiting_us, value.waiting_us),
                (&mut result.phases.output_us, value.output_us),
                (&mut result.phases.tools_us, value.tools_us),
                (&mut result.phases.other_us, value.other_us),
            ] {
                *sum = sum
                    .zip(next)
                    .map(|(left, right)| left.saturating_add(right));
            }
        }
    }
    result.calls = calls.len() as u64;
    let children: i64 = {
        let mut statement = db.prepare("SELECT count(DISTINCT d.turn) FROM agent_delegations d JOIN conversation_turns h ON h.turn=d.parent_turn WHERE h.session=?1").await.map_err(storage_error)?;
        statement
            .query_row([session.to_string()])
            .await
            .and_then(|row| row.get(0))
            .map_err(storage_error)?
    };
    result.children = u64::try_from(children).map_err(storage_error)?;
    let mut calls = calls.values().cloned().collect::<Vec<_>>();
    calls.sort_by_key(|(sequence, _)| *sequence);
    let mut latency = Latencies::default();
    for (_, call) in &calls {
        for attempt in &call.attempts {
            latency.observe(attempt);
            result.model_us = result
                .model_us
                .saturating_add(attempt.elapsed_us.unwrap_or_default());
            if attempt.reasoning_reported
                && let Some(usage) = &attempt.usage
                && let Some((elapsed, first)) = attempt.elapsed_us.zip(attempt.first_text_us)
                && elapsed > first
            {
                result.output_us = result.output_us.saturating_add(elapsed - first);
                result.output_tokens = result
                    .output_tokens
                    .saturating_add(usage.output.saturating_sub(usage.reasoning));
                result.output_samples += 1;
            }
            for row in &attempt.categories {
                let total = composition.entry(row.kind).or_insert_with(|| Category {
                    kind: row.kind,
                    items: Some(0),
                    tokens: Some(0),
                    method: row.method.clone(),
                });
                total.items = total
                    .items
                    .zip(row.items)
                    .map(|(sum, value)| sum.saturating_add(value));
                total.tokens = total
                    .tokens
                    .zip(row.tokens)
                    .map(|(sum, value)| sum.saturating_add(value));
            }
        }
    }
    result.latency = latency.finish();
    result.composition = composition.into_values().collect();
    // Published statistics carry one bounded snapshot per round; aggregates above use every attempt.
    let mut rounds = BTreeMap::<TurnId, (u64, Request)>::new();
    for (sequence, mut call) in calls {
        call.attempts = call.attempts.pop().into_iter().collect();
        rounds.insert(call.turn, (sequence, call));
    }
    let mut rounds = rounds.into_values().collect::<Vec<_>>();
    rounds.sort_by_key(|(sequence, _)| *sequence);
    result.recent = rounds
        .into_iter()
        .rev()
        .take(RECENT_LIMIT)
        .map(|(_, value)| value)
        .collect();
    result.recent.reverse();
    Ok((result, phased, state))
}

#[cfg(test)]
mod tests;
