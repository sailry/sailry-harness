//! One bounded in-memory window; identities never survive an epoch change.
use super::{
    super::Metadata,
    Observation,
    types::{Counts, Dimensions, Table},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) const GROUP_LIMIT: usize = 256;

pub(super) struct Window {
    epoch: u64,
    metadata: Metadata,
    started: DateTime<Utc>,
    groups: BTreeMap<(Table, String), (Dimensions, Counts)>,
}

#[derive(Serialize)]
pub(super) struct Batch {
    pub table: &'static str,
    pub data: Vec<Value>,
}

#[derive(Serialize)]
struct Common<'a> {
    installation_id: &'a str,
    release: &'a str,
    channel: &'a str,
    application: &'a str,
    os: &'a str,
    arch: &'a str,
    window_start: DateTime<Utc>,
    window_end: DateTime<Utc>,
}

impl Window {
    pub(super) fn new(epoch: u64, metadata: Metadata, started: DateTime<Utc>) -> Self {
        Self {
            epoch,
            metadata,
            started,
            groups: BTreeMap::new(),
        }
    }

    pub(super) fn epoch(&self) -> u64 {
        self.epoch
    }

    pub(super) fn add(&mut self, observation: Observation) {
        let (table, dimensions, counts) = observation.sample();
        let Ok(key) = serde_json::to_string(&dimensions) else {
            return;
        };
        if let Some((_, total)) = self.groups.get_mut(&(table, key.clone())) {
            total.add(counts);
        } else if self.groups.len() < GROUP_LIMIT {
            self.groups.insert((table, key), (dimensions, counts));
        }
    }

    pub(super) fn finish(self, ended: DateTime<Utc>) -> Vec<Batch> {
        let common = Common {
            installation_id: &self.metadata.installation_id,
            release: &self.metadata.release,
            channel: self.metadata.channel,
            application: self.metadata.application,
            os: self.metadata.os,
            arch: self.metadata.arch,
            window_start: self.started,
            window_end: ended.max(self.started),
        };
        let Ok(Value::Object(common)) = serde_json::to_value(common) else {
            return Vec::new();
        };
        let mut tables: BTreeMap<Table, Vec<Value>> = BTreeMap::new();
        for ((table, _), (dimensions, counts)) in self.groups {
            let (Ok(Value::Object(dimensions)), Ok(Value::Object(counts))) = (
                serde_json::to_value(dimensions),
                serde_json::to_value(counts),
            ) else {
                continue;
            };
            let mut event = common.clone();
            event.insert(
                "event_id".into(),
                Value::String(uuid::Uuid::new_v4().to_string()),
            );
            event.extend(dimensions);
            event.extend(counts);
            tables.entry(table).or_default().push(Value::Object(event));
        }
        tables
            .into_iter()
            .map(|(table, data)| Batch {
                table: table.name(),
                data,
            })
            .collect()
    }
}
