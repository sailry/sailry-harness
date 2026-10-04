use super::*;
use chrono::{DateTime, Utc};
use sailry_protocol::usage::{
    Cost, Dimension, Group, Key, Metrics, PAGE_SIZE, Page, Point, Position, Query, Report, Request,
};
use std::collections::{BTreeMap, BTreeSet};

mod pricing;
mod query;
mod session;
pub(super) use session::read as session;
#[cfg(test)]
mod tests;

const DAY: i64 = 86_400_000;
const HALF_HOUR: i64 = 1_800_000;
const MAX_ROWS: usize = 100_000;
const MAX_GROUPS: usize = 4096;

pub(in crate::store) fn read(
    db: &Connection,
    node: NodeId,
    query: &Query,
) -> Result<Report, Fault> {
    let (start, end) = query::validate(query)?;
    let cursor = db
        .query_row("SELECT coalesce(max(cursor),0) FROM events", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(storage_error)? as u64;
    let mut days = buckets(query.start_ms, query.end_ms, DAY);
    let recent_start = query.start_ms.max(query.end_ms - 2 * DAY);
    let mut recent = buckets(recent_start, query.end_ms, HALF_HOUR);
    let mut totals = Metrics::default();
    let mut groups = BTreeMap::<Key, Metrics>::new();
    let mut resources = BTreeSet::new();
    let mut prices = pricing::Catalog::default();
    let mut requests = BTreeMap::new();
    let mut has_more = false;
    let mut statement = db.prepare(query::ROWS).map_err(storage_error)?;
    let mut rows = statement
        .query(params![
            start.format("%Y-%m-%dT%H:%M:%S").to_string(),
            end.format("%Y-%m-%dT%H:%M:%S").to_string(),
            serde_json::to_string(&query.projects).map_err(storage_error)?,
            serde_json::to_string(&query.worktrees).map_err(storage_error)?,
            serde_json::to_string(&query.providers).map_err(storage_error)?,
            serde_json::to_string(&query.models).map_err(storage_error)?,
            MAX_ROWS as i64 + 1,
        ])
        .map_err(storage_error)?;
    let mut count = 0;
    while let Some(row) = rows.next().map_err(storage_error)? {
        count += 1;
        if count > MAX_ROWS {
            return Err(invalid(
                "usage report exceeds the row limit; narrow the time range or filters",
            ));
        }
        let timestamp: String = row.get(0).map_err(storage_error)?;
        // Indexed seconds bound the scan; Chrono preserves exact millisecond boundaries.
        let timestamp = DateTime::parse_from_rfc3339(&timestamp)
            .map_err(storage_error)?
            .timestamp_millis();
        if timestamp < query.start_ms || timestamp >= query.end_ms {
            continue;
        }
        let tokens = Usage {
            input: row.get::<_, i64>(1).map_err(storage_error)? as u64,
            output: row.get::<_, i64>(2).map_err(storage_error)? as u64,
            cached_input: row.get::<_, i64>(3).map_err(storage_error)? as u64,
            reasoning: row.get::<_, i64>(4).map_err(storage_error)? as u64,
        };
        let provider = query::id(row, 5)?;
        let model: String = row.get(6).map_err(storage_error)?;
        let cost = prices.estimate(db, row)?;
        let elapsed = row
            .get::<_, Option<i64>>(13)
            .map_err(storage_error)?
            .filter(|value| *value > 0)
            .map(|value| value as u64);
        let project: Option<ProjectId> = row
            .get::<_, Option<String>>(7)
            .map_err(storage_error)?
            .map(|id| id.parse())
            .transpose()
            .map_err(storage_error)?;
        let connection = row
            .get::<_, Option<String>>(21)
            .map_err(storage_error)?
            .map(|value| serde_json::from_str::<sailry_protocol::connection::Resource>(&value))
            .transpose()
            .map_err(storage_error)?;
        let (owner, location) = match (project, connection) {
            (Some(project), None) => (
                Key::Project(project),
                Key::Worktree {
                    project,
                    worktree: query::id(row, 8)?,
                },
            ),
            (None, Some(resource)) => (Key::Connection(resource), Key::Connection(resource)),
            (None, None) => {
                let assistant = row
                    .get::<_, Option<String>>(22)
                    .map_err(storage_error)?
                    .map(|value| {
                        serde_json::from_str::<sailry_protocol::plugin::conversation::Binding>(
                            &value,
                        )
                    })
                    .transpose()
                    .map_err(storage_error)?;
                let owner = match assistant {
                    Some(assistant) => Key::Assistant {
                        package: assistant.package.name,
                        id: assistant.id,
                    },
                    None => Key::Session(query::id(row, 16)?),
                };
                (owner.clone(), owner)
            }
            _ => {
                return Err(storage_error(
                    "usage response has inconsistent conversation ownership",
                ));
            }
        };
        let keys = [
            Key::Provider(provider),
            Key::Model {
                provider,
                model: model.clone(),
            },
            owner,
            location,
        ];
        let key = keys[match query.dimension {
            Dimension::Provider => 0,
            Dimension::Model => 1,
            Dimension::Project => 2,
            Dimension::Worktree => 3,
        }]
        .clone();
        resources.extend(keys);
        if resources.len() > MAX_GROUPS * 4 {
            return Err(invalid(
                "usage report exceeds the resource limit; narrow the time range or filters",
            ));
        }
        add(
            groups.entry(key).or_default(),
            &tokens,
            cost.as_ref(),
            elapsed,
        )?;
        if groups.len() > MAX_GROUPS {
            return Err(invalid(
                "usage report exceeds the group limit; narrow the time range or filters",
            ));
        }
        add(&mut totals, &tokens, cost.as_ref(), elapsed)?;
        let day = ((timestamp.div_euclid(DAY) * DAY - days[0].start_ms) / DAY) as usize;
        add(&mut days[day].metrics, &tokens, cost.as_ref(), elapsed)?;
        add_model(&mut days[day], &model, &tokens);
        if timestamp >= recent_start {
            let bucket = ((timestamp.div_euclid(HALF_HOUR) * HALF_HOUR - recent[0].start_ms)
                / HALF_HOUR) as usize;
            add(&mut recent[bucket].metrics, &tokens, cost.as_ref(), elapsed)?;
            add_model(&mut recent[bucket], &model, &tokens);
        }
        let position = Position {
            timestamp_ms: timestamp,
            node,
            sequence: row.get::<_, i64>(15).map_err(storage_error)? as u64,
        };
        if query.before.is_none_or(|before| position < before) {
            requests.insert(
                position,
                Request {
                    position,
                    session: query::id(row, 16)?,
                    turn: query::id(row, 17)?,
                    project,
                    worktree: query::id(row, 8)?,
                    provider,
                    model,
                    tokens,
                    usd_micros: cost.map(|cost| cost.usd_micros),
                    elapsed_us: elapsed,
                    first_token_us: row
                        .get::<_, Option<i64>>(18)
                        .map_err(storage_error)?
                        .filter(|value| {
                            *value >= 0 && elapsed.is_some_and(|elapsed| *value as u64 <= elapsed)
                        })
                        .map(|value| value as u64),
                    provider_name: row
                        .get::<_, Option<String>>(19)
                        .map_err(storage_error)?
                        .unwrap_or_else(|| provider.to_string()),
                    scope_name: row.get(20).map_err(storage_error)?,
                },
            );
            if requests.len() > PAGE_SIZE {
                requests.pop_first();
                has_more = true;
            }
        }
    }
    let report = Report {
        node,
        cursor,
        pricing_updated_at_ms: crate::store::providers::catalog::status(db)?.updated_at_ms,
        query: query.clone(),
        totals,
        days,
        recent,
        groups: groups
            .into_iter()
            .map(|(key, metrics)| Group { key, metrics })
            .collect(),
        resources: resources.into_iter().collect(),
        requests: Page {
            items: requests.into_values().rev().collect(),
            has_more,
        },
    };
    if serde_json::to_vec(&report).map_err(storage_error)?.len() > MAX_FRAME_BYTES - 64 * 1024 {
        return Err(invalid(
            "usage report exceeds the response limit; narrow the filters",
        ));
    }
    Ok(report)
}

fn buckets(start: i64, end: i64, width: i64) -> Vec<Point> {
    let first = start.div_euclid(width) * width;
    (first..end)
        .step_by(width as usize)
        .map(|start_ms| Point {
            start_ms,
            metrics: Metrics::default(),
            models: BTreeMap::new(),
        })
        .collect()
}

fn add_model(point: &mut Point, model: &str, tokens: &Usage) {
    point
        .models
        .entry(model.to_owned())
        .and_modify(|total| {
            total.input += tokens.input;
            total.output += tokens.output;
            total.cached_input += tokens.cached_input;
            total.reasoning += tokens.reasoning;
        })
        .or_insert_with(|| tokens.clone());
}

fn add(
    metrics: &mut Metrics,
    tokens: &Usage,
    cost: Option<&Cost>,
    elapsed: Option<u64>,
) -> Result<(), Fault> {
    // At most MAX_ROWS nonnegative i32 counters are read from canonical ADK events.
    metrics.responses += 1;
    let total = metrics.tokens.get_or_insert(Usage {
        input: 0,
        output: 0,
        cached_input: 0,
        reasoning: 0,
    });
    total.input += tokens.input;
    total.output += tokens.output;
    total.cached_input += tokens.cached_input;
    total.reasoning += tokens.reasoning;
    if let Some(elapsed) = elapsed {
        let generation = metrics.generation.get_or_insert_default();
        generation.elapsed_us = generation
            .elapsed_us
            .checked_add(elapsed)
            .ok_or_else(|| invalid("usage duration exceeds the counter limit"))?;
        generation.output_tokens += tokens.output;
        generation.responses += 1;
    }
    if let Some(value) = cost {
        metrics.cost = Some(match &metrics.cost {
            Some(total) => total
                .checked_add(value)
                .ok_or_else(|| invalid("usage cost exceeds the counter limit"))?,
            None => value.clone(),
        });
    }
    Ok(())
}
