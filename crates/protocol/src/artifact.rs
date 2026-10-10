//! Session-owned, versioned HTML fragments; presentation belongs to plugins.
use crate::{ArtifactId, SessionId, TurnId, plugin};
use serde::{Deserialize, Serialize};

pub const MAX_HTML_BYTES: usize = 512 * 1024;
pub const MAX_PAGE_LIMIT: u16 = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub id: ArtifactId,
    pub session: SessionId,
    pub turn: TurnId,
    pub package: plugin::Reference,
    pub title: String,
    pub summary: String,
    pub revision: u32,
    pub size: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Summary {
    pub fn valid(&self) -> bool {
        self.revision > 0
            && self.size > 0
            && self.size <= MAX_HTML_BYTES as u64
            && label(&self.title, 120, false)
            && label(&self.summary, 280, true)
            && self.created_at <= self.updated_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub artifact: Summary,
    pub latest_revision: u32,
    pub html: String,
    pub warnings: Vec<String>,
    pub view: Option<ViewContent>,
}

pub const MAX_VIEW_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub template: String,
    #[serde(default)]
    pub styles: Vec<String>,
    #[serde(default)]
    pub scripts: Vec<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Library {
    Mermaid,
    Echarts,
    Mathjax,
}

impl View {
    pub fn valid(&self) -> bool {
        let paths: Vec<_> = std::iter::once(&self.template)
            .chain(&self.styles)
            .chain(&self.scripts)
            .collect();
        self.template.ends_with(".html")
            && self.styles.len() <= 8
            && self.scripts.len() <= 8
            && self.styles.iter().all(|path| path.ends_with(".css"))
            && self.scripts.iter().all(|path| path.ends_with(".js"))
            && self.libraries.len() <= 3
            && self
                .libraries
                .iter()
                .enumerate()
                .all(|(index, library)| !self.libraries[..index].contains(library))
            && plugin::desktop::valid_paths(paths.iter().map(|path| path.as_str()))
            && paths
                .iter()
                .all(|path| path.starts_with("dev.sailry.platform/"))
    }
}

/// Trusted code is frozen package content, never model-authored markup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewContent {
    pub template: String,
    pub styles: Vec<String>,
    pub scripts: Vec<String>,
    pub libraries: Vec<Library>,
}

impl ViewContent {
    pub fn valid(&self) -> bool {
        !self.template.is_empty()
            && self.styles.len() <= 8
            && self.scripts.len() <= 8
            && self.libraries.len() <= 3
            && self
                .libraries
                .iter()
                .enumerate()
                .all(|(index, library)| !self.libraries[..index].contains(library))
            && self.template.len()
                + self.styles.iter().map(String::len).sum::<usize>()
                + self.scripts.iter().map(String::len).sum::<usize>()
                <= MAX_VIEW_BYTES
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub id: ArtifactId,
    pub revision: u32,
    pub turn: TurnId,
    pub title: String,
    pub summary: String,
    pub size: u64,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub items: Vec<Summary>,
    pub next_after: Option<ArtifactId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionPage {
    pub items: Vec<Revision>,
    pub next_before: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Write {
    pub id: Option<ArtifactId>,
    /// Required for an existing artifact; absent for creation.
    pub expected_revision: Option<u32>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub html: String,
}

pub fn label(value: &str, limit: usize, empty: bool) -> bool {
    (empty || !value.trim().is_empty())
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
}
