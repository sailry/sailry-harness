//! Public source coordinates for real skills in the official package inventory.
use super::{Skill, skills};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub package: String,
    pub skill: Skill,
    pub source: skills::Source,
    pub bundled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub unavailable: bool,
}
