//! Complete task progress snapshots recorded as canonical tool results.
use crate::{ErrorCode, Fault};
use serde::{Deserialize, Serialize};

pub const MAX_STEPS: usize = 256;
pub const MAX_DESCRIPTION_BYTES: usize = 8192;
pub const MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub title: Option<String>,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub description: String,
    pub state: State,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Pending,
    InProgress,
    Completed,
    Skipped,
}

impl Progress {
    pub fn validate(&self) -> Result<(), Fault> {
        let valid = |text: &str, limit| !text.trim().is_empty() && text.len() <= limit;
        if self.title.as_ref().is_some_and(|title| !valid(title, 512))
            || self.steps.is_empty()
            || self.steps.len() > MAX_STEPS
            || self
                .steps
                .iter()
                .any(|step| !valid(&step.description, MAX_DESCRIPTION_BYTES))
            || self.title.as_ref().map_or(0, String::len)
                + self
                    .steps
                    .iter()
                    .map(|step| step.description.len())
                    .sum::<usize>()
                > MAX_BYTES
        {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "invalid task progress",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
