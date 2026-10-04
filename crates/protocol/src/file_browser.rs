//! Host file selection and management, independent of registered projects.
use crate::{Directory, Fault};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    Home,
    Desktop,
    Documents,
    Downloads,
    Volume,
    Root,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub kind: LocationKind,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub separator: char,
    pub locations: Vec<Location>,
    pub parent: Option<String>,
    pub directory: Directory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    Stop,
    Replace,
    KeepBoth,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    CreateDirectory {
        path: String,
    },
    Rename {
        from: String,
        to: String,
    },
    Transfer {
        paths: Vec<String>,
        destination: String,
        cut: bool,
        conflict: Conflict,
    },
    Trash {
        paths: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub path: String,
    pub destination: Option<String>,
    pub error: Option<Fault>,
}
