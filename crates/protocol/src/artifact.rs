//! Session-owned HTML revisions; presentation belongs to plugins.
use crate::{ArtifactId, SessionId, TurnId, plugin};
use serde::{Deserialize, Serialize};

pub const MAX_HTML_BYTES: usize = 512 * 1024;
pub const MAX_PAGE_LIMIT: u16 = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Document,
    /// Self-contained code rendered without a business or same-origin bridge.
    Preview,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub id: ArtifactId,
    #[serde(default)]
    pub kind: Kind,
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
            && label(&self.title, false)
            && label(&self.summary, true)
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
    /// CSS classes accepted in this package's model-authored fragments.
    #[serde(default)]
    pub allowed_classes: Vec<String>,
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
            && self.allowed_classes.len() <= 64
            && self
                .allowed_classes
                .iter()
                .enumerate()
                .all(|(index, class)| {
                    !class.is_empty()
                        && class.len() <= 64
                        && class.as_bytes()[0].is_ascii_alphabetic()
                        && class
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
                        && !self.allowed_classes[..index].contains(class)
                })
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
    #[serde(default)]
    pub kind: Kind,
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
    #[serde(default)]
    pub kind: Kind,
    /// Required for an existing artifact; absent for creation.
    pub expected_revision: Option<u32>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub html: String,
}

pub fn label(value: &str, empty: bool) -> bool {
    (empty || !value.trim().is_empty()) && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_omitted_kind_to_document() {
        let mut value = serde_json::json!({"title":"Report","html":"<p>Body</p>"});
        let write: Write = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(write.kind, Kind::Document);
        value["kind"] = "preview".into();
        assert_eq!(
            serde_json::from_value::<Write>(value.clone()).unwrap().kind,
            Kind::Preview
        );
        for kind in [serde_json::json!("unknown"), serde_json::Value::Null] {
            value["kind"] = kind;
            assert!(serde_json::from_value::<Write>(value.clone()).is_err());
        }
    }
}
