//! Usage reported by canonical model responses on one execution Node.
use crate::{NodeId, ProjectId, ProviderId, SessionId, TurnId, WorktreeId, conversation::Usage};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// Inclusive UTC milliseconds; reports cover at most 366 days.
    pub start_ms: i64,
    /// Exclusive UTC milliseconds.
    pub end_ms: i64,
    pub dimension: Dimension,
    /// Empty filters include all resources; different filters are intersected.
    pub projects: Vec<ProjectId>,
    pub worktrees: Vec<WorktreeId>,
    pub providers: Vec<ProviderId>,
    pub models: Vec<String>,
    /// Exclusive descending request cursor; does not restrict totals or chart buckets.
    pub before: Option<Position>,
}

pub const PAGE_SIZE: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub timestamp_ms: i64,
    pub node: NodeId,
    /// Stable canonical event sequence on this Node, including ties within a millisecond.
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub position: Position,
    pub session: SessionId,
    pub turn: TurnId,
    pub project: Option<ProjectId>,
    pub scope_name: String,
    pub worktree: WorktreeId,
    pub provider: ProviderId,
    pub provider_name: String,
    pub model: String,
    pub tokens: Usage,
    pub usd_micros: Option<u64>,
    pub elapsed_us: Option<u64>,
    /// Time until the first observed nonempty text chunk, when streamed.
    pub first_token_us: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    /// At most PAGE_SIZE canonical responses, ordered by descending position.
    pub items: Vec<Request>,
    pub has_more: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Provider,
    Model,
    Project,
    Worktree,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    /// Responses with reported usage, not transport attempts or billed requests.
    pub responses: u64,
    /// Absent when no response reported usage.
    pub tokens: Option<Usage>,
    /// Estimated USD for the covered responses; missing prices are not zero.
    pub cost: Option<Cost>,
    /// Observed model request time and output for responses with timing metadata.
    pub generation: Option<Generation>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cost {
    pub usd_micros: u64,
    pub responses: u64,
    /// Present only when every priced response has a category breakdown.
    pub breakdown: Option<CostBreakdown>,
}

/// Disjoint estimated charges in USD micros. Input/output include their audio
/// categories; output also includes reasoning. Cached input is charged separately.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Cost {
    pub fn checked_add(&self, other: &Self) -> Option<Self> {
        Some(Self {
            usd_micros: self.usd_micros.checked_add(other.usd_micros)?,
            responses: self.responses.checked_add(other.responses)?,
            breakdown: match (&self.breakdown, &other.breakdown) {
                (Some(left), Some(right)) => Some(CostBreakdown {
                    input: left.input.checked_add(right.input)?,
                    output: left.output.checked_add(right.output)?,
                    cache_read: left.cache_read.checked_add(right.cache_read)?,
                    cache_write: left.cache_write.checked_add(right.cache_write)?,
                }),
                _ => None,
            },
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generation {
    pub elapsed_us: u64,
    pub output_tokens: u64,
    pub responses: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub start_ms: i64,
    pub metrics: Metrics,
    /// Frozen model IDs and their reported tokens within this time bucket.
    pub models: std::collections::BTreeMap<String, Usage>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Key {
    Provider(ProviderId),
    Model {
        provider: ProviderId,
        model: String,
    },
    Project(ProjectId),
    Session(SessionId),
    Connection(crate::connection::Resource),
    Assistant {
        package: String,
        id: String,
    },
    Worktree {
        project: ProjectId,
        worktree: WorktreeId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub key: Key,
    pub metrics: Metrics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub node: NodeId,
    /// Node event cursor captured with this report, used for subscription refresh.
    pub cursor: u64,
    /// Timestamp of the Node's current reference price catalog, not a historical bill.
    pub pricing_updated_at_ms: Option<u64>,
    pub query: Query,
    pub totals: Metrics,
    /// UTC day buckets, including empty days in the requested range.
    pub days: Vec<Point>,
    /// Half-hour buckets in the last 48 hours of the requested range.
    pub recent: Vec<Point>,
    pub groups: Vec<Group>,
    /// Distinct owners and frozen models in the matching canonical responses.
    /// Available independently of the selected grouping and current inventory.
    pub resources: Vec<Key>,
    pub requests: Page,
}

#[cfg(test)]
mod tests;
