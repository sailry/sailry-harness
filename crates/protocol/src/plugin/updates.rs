//! Read-only update inspection; installation remains an explicit durable command.
use super::{Info, Summary};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub current: Summary,
    pub available: Option<Info>,
    /// Uploaded packages without a reusable source require another explicit selection.
    pub manual: bool,
}
