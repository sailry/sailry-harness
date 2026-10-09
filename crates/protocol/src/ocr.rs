//! Shared OCR configuration and model installation snapshots.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub enabled: bool,
    pub model: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            enabled: true,
            model: None,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), crate::Fault> {
        if self.revision > i64::MAX as u64 || self.model.as_ref().is_some_and(|id| !valid_id(id)) {
            return Err(crate::Fault::invalid("invalid OCR settings"));
        }
        Ok(())
    }
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub definition: crate::models::Definition,
    pub state: State,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum State {
    Missing,
    Downloading { percent: u8, bytes_per_second: u64 },
    Installed,
}
