use super::*;
use chrono::{DateTime, Utc};
use sailry_protocol::usage::{
    Cost, Dimension, Group, Key, Metrics, PAGE_SIZE, Page, Point, Position, Query, Report, Request,
};
use std::collections::{BTreeMap, BTreeSet};

mod cache;
mod pricing;
mod query;
mod session;
pub(in crate::store) use cache::Cache;
#[cfg(test)]
pub(super) use session::ROWS as SESSION_ROWS;
pub(super) use session::read as session;
#[cfg(test)]
mod tests;

const DAY: i64 = 86_400_000;
const HALF_HOUR: i64 = 1_800_000;
const MAX_ROWS: usize = 100_000;
const MAX_GROUPS: usize = 4096;

pub(in crate::store) async fn read(
    db: &Connection,
    node: NodeId,
    query: &Query,
) -> Result<Report, Fault> {
    let (start, end) = query::validate(query)?;
    let cursor = {
        let mut statement = db
            .prepare("SELECT coalesce(max(cursor),0) FROM events")
            .await
            .map_err(storage_error)?;
        statement
            .query_row(())
            .await
            .and_then(|row| row.get::<i64>(0))
            .map_err(storage_error)?
    } as u64;
    let offset = i64::from(query.utc_offset_minutes) * 60_000;
    let mut days = buckets(query.start_ms, query.end_ms, DAY, offset);
    let recent_start = query.start_ms.max(query.end_ms - 2 * DAY);
    let mut recent = buckets(recent_start, query.end_ms, HALF_HOUR, offset);
    let mut totals = Metrics::default();
    let mut groups = BTreeMap::<Key, Metrics>::new();
    let mut names = BTreeMap::<Key, String>::new();
    let mut resources = BTreeSet::new();
    let mut prices = pricing::Catalog::default();
    let mut requests = BTreeMap::new();
    let mut has_more = false;
    let mut statement = db.prepare(query::ROWS).await.map_err(storage_error)?;
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
        .await
        .map_err(storage_error)?;
    let mut count = 0;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        count += 1;
        if count > MAX_ROWS {
            return Err(sailry_protocol::Fault::invalid(
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
            input: row.get::<i64>(1).map_err(storage_error)? as u64,
            output: row.get::<i64>(2).map_err(storage_error)? as u64,
            cached_input: row.get::<i64>(3).map_err(storage_error)? as u64,
            reasoning: row.get::<i64>(4).map_err(storage_error)? as u64,
        };
        let provider: sailry_protocol::ProviderId = query::id(&row, 5)?;
        let model: String = row.get(6).map_err(storage_error)?;
        let provider_name = row
            .get::<Option<String>>(19)
            .map_err(storage_error)?
            .unwrap_or_else(|| provider.to_string());
        let scope_name: String = row.get(20).map_err(storage_error)?;
        let cost = prices.estimate(db, &row).await?;
        let elapsed = row
            .get::<Option<i64>>(13)
            .map_err(storage_error)?
            .filter(|value| *value > 0)
            .map(|value| value as u64);
        let project: Option<ProjectId> = row
            .get::<Option<String>>(7)
            .map_err(storage_error)?
            .map(|id| id.parse())
            .transpose()
            .map_err(storage_error)?;
        let connection = row
            .get::<Option<String>>(21)
            .map_err(storage_error)?
            .map(|value| serde_json::from_str::<sailry_protocol::connection::Resource>(&value))
            .transpose()
            .map_err(storage_error)?;
        let (owner, location) = match (project, connection) {
            (Some(project), None) => (
                Key::Project(project),
                Key::Worktree {
                    project,
                    worktree: query::id(&row, 8)?,
                },
            ),
            (None, Some(resource)) => (Key::Connection(resource), Key::Connection(resource)),
            (None, None) => {
                let assistant = row
                    .get::<Option<String>>(22)
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
                    None => Key::Session(query::id(&row, 16)?),
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
        let (key, name) = match query.dimension {
            Dimension::Provider => (keys[0].clone(), Some(&provider_name)),
            Dimension::Model => (keys[1].clone(), Some(&provider_name)),
            Dimension::Project => (keys[2].clone(), Some(&scope_name)),
            Dimension::Worktree => (keys[3].clone(), None),
        };
        // Rows are ordered by time, so the latest frozen label wins.
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            names.insert(key.clone(), name.clone());
        }
        resources.extend(keys);
        if resources.len() > MAX_GROUPS * 4 {
            return Err(sailry_protocol::Fault::invalid(
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
            return Err(sailry_protocol::Fault::invalid(
                "usage report exceeds the group limit; narrow the time range or filters",
            ));
        }
        add(&mut totals, &tokens, cost.as_ref(), elapsed)?;
        let day = index(timestamp, days[0].start_ms, DAY, offset);
        add(&mut days[day].metrics, &tokens, cost.as_ref(), elapsed)?;
        add_model(&mut days[day], &model, &tokens);
        if timestamp >= recent_start {
            let bucket = index(timestamp, recent[0].start_ms, HALF_HOUR, offset);
            add(&mut recent[bucket].metrics, &tokens, cost.as_ref(), elapsed)?;
            add_model(&mut recent[bucket], &model, &tokens);
        }
        let position = Position {
            timestamp_ms: timestamp,
            node,
            sequence: row.get::<i64>(15).map_err(storage_error)? as u64,
        };
        if query.before.is_none_or(|before| position < before) {
            requests.insert(
                position,
                Request {
                    position,
                    session: query::id(&row, 16)?,
                    session_title: None,
                    turn: query::id(&row, 17)?,
                    project,
                    worktree: query::id(&row, 8)?,
                    provider,
                    model,
                    tokens,
                    usd_micros: cost.map(|cost| cost.usd_micros),
                    elapsed_us: elapsed,
                    first_token_us: row
                        .get::<Option<i64>>(18)
                        .map_err(storage_error)?
                        .filter(|value| {
                            *value >= 0 && elapsed.is_some_and(|elapsed| *value as u64 <= elapsed)
                        })
                        .map(|value| value as u64),
                    provider_name,
                    scope_name,
                },
            );
            if requests.len() > PAGE_SIZE {
                requests.pop_first();
                has_more = true;
            }
        }
    }
    drop(rows);
    drop(statement);
    let mut items: Vec<Request> = requests.into_values().rev().collect();
    let mut titles = BTreeMap::new();
    for item in &mut items {
        if let std::collections::btree_map::Entry::Vacant(entry) = titles.entry(item.session) {
            let title = crate::store::agent::activity::title(db, item.session).await?;
            entry.insert(Some(title).filter(|title| !title.trim().is_empty()));
        }
        item.session_title = titles[&item.session].clone();
    }
    let report = Report {
        node,
        cursor,
        pricing_updated_at_ms: crate::store::providers::catalog::status(db)
            .await?
            .updated_at_ms,
        query: query.clone(),
        totals,
        days,
        recent,
        groups: groups
            .into_iter()
            .map(|(key, metrics)| Group {
                name: names.remove(&key),
                key,
                metrics,
            })
            .collect(),
        resources: resources.into_iter().collect(),
        requests: Page { items, has_more },
    };
    if serde_json::to_vec(&report).map_err(storage_error)?.len() > MAX_FRAME_BYTES - 64 * 1024 {
        return Err(sailry_protocol::Fault::invalid(
            "usage report exceeds the response limit; narrow the filters",
        ));
    }
    Ok(report)
}

/// Start of the local bucket containing `timestamp`, in UTC milliseconds.
fn floor(timestamp: i64, width: i64, offset: i64) -> i64 {
    (timestamp + offset).div_euclid(width) * width - offset
}

fn index(timestamp: i64, first: i64, width: i64, offset: i64) -> usize {
    ((floor(timestamp, width, offset) - first) / width) as usize
}

fn buckets(start: i64, end: i64, width: i64, offset: i64) -> Vec<Point> {
    let first = floor(start, width, offset);
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
    // At most MAX_ROWS bounded counters are read from canonical records.
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
        generation.elapsed_us = generation.elapsed_us.checked_add(elapsed).ok_or_else(|| {
            sailry_protocol::Fault::invalid("usage duration exceeds the counter limit")
        })?;
        generation.output_tokens += tokens.output;
        generation.responses += 1;
    }
    if let Some(value) = cost {
        metrics.cost = Some(match &metrics.cost {
            Some(total) => total.checked_add(value).ok_or_else(|| {
                sailry_protocol::Fault::invalid("usage cost exceeds the counter limit")
            })?,
            None => value.clone(),
        });
    }
    Ok(())
}
