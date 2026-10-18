//! Node-owned source collections and rebuildable local retrieval indexes.
use crate::{Fault, KnowledgeId, WorktreeId, plugin::storage::Scope};
use serde::{Deserialize, Serialize};

pub const DEFAULT_LIMIT: u16 = 5;
pub const PAGE_LIMIT: u16 = 100;
pub const MAX_COLLECTIONS: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Model {
    #[default]
    GraniteMulti,
    BgeSmallZh,
    PotionMulti,
    Registered {
        id: crate::ModelId,
    },
}

impl Model {
    pub const ALL: [Self; 3] = [Self::GraniteMulti, Self::BgeSmallZh, Self::PotionMulti];

    pub fn key(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::GraniteMulti => "granite-multi".into(),
            Self::BgeSmallZh => "bge-small-zh".into(),
            Self::PotionMulti => "potion-multi".into(),
            Self::Registered { id } => format!("model-{id}").into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub model: Option<Model>,
    pub mirror: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            model: Some(Model::default()),
            mirror: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelState {
    Missing,
    Downloading { percent: u8, bytes_per_second: u64 },
    Installed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub model: Model,
    pub repository: String,
    pub revision: String,
    pub dimension: u16,
    pub size_bytes: u64,
    pub license: String,
    pub state: ModelState,
    pub definition: crate::models::Definition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub model: Model,
    pub definition: crate::models::Definition,
}

/// Plugin sources remain within their captured worktree or private storage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Worktree { worktree: WorktreeId, path: String },
    Directory { path: String },
    File { path: String },
    Storage { scope: Scope, prefix: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Owner {
    Controller,
    Plugin { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub name: String,
    pub source: Source,
    #[serde(default)]
    pub model: Option<Model>,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Exact package names allowed to query; the collection owner is always allowed.
    #[serde(default)]
    pub shared_with: Vec<String>,
}

/// Plugin-facing creation arguments never select an owner or another worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub name: String,
    pub source: DraftSource,
    #[serde(default)]
    pub model: Option<Model>,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftSource {
    Worktree { path: String },
    Storage { scope: Scope, prefix: String },
}

impl Draft {
    pub fn into_definition(self, worktree: Option<WorktreeId>) -> Result<Definition, Fault> {
        let source = match self.source {
            DraftSource::Worktree { path } => Source::Worktree {
                worktree: worktree.ok_or_else(|| {
                    Fault::invalid("knowledge source requires a captured worktree")
                })?,
                path,
            },
            DraftSource::Storage { scope, prefix } => Source::Storage { scope, prefix },
        };
        let definition = Definition {
            name: self.name,
            source,
            model: self.model,
            globs: self.globs,
            tags: self.tags,
            shared_with: Vec::new(),
        };
        definition.validate()?;
        Ok(definition)
    }
}

impl Definition {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.name.trim().is_empty()
            || self.name.contains('\0')
            || !filters(&self.tags)
            || self
                .globs
                .iter()
                .any(|glob| glob.is_empty() || glob.contains('\0'))
            || self.shared_with.iter().any(|name| {
                name.is_empty()
                    || !name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
        {
            return Err(Fault::invalid("invalid knowledge collection definition"));
        }
        let path = match &self.source {
            Source::Worktree { path, .. } | Source::Directory { path } | Source::File { path } => {
                path
            }
            Source::Storage { prefix, .. } => {
                if !self.globs.is_empty()
                    || prefix.len() > crate::plugin::storage::MAX_KEY_BYTES
                    || prefix.contains('\0')
                {
                    return Err(Fault::invalid("invalid knowledge storage prefix"));
                }
                return Ok(());
            }
        };
        if path.contains('\0') {
            return Err(Fault::invalid("invalid knowledge source path"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Pending,
    Indexing,
    Ready,
    Paused,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    pub status: Status,
    pub sources: u64,
    pub completed: u64,
    pub chunks: u64,
    pub skipped: u64,
    pub model: Option<Model>,
    pub generation: u64,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            status: Status::Pending,
            sources: 0,
            completed: 0,
            chunks: 0,
            skipped: 0,
            model: None,
            generation: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub id: KnowledgeId,
    pub revision: u64,
    #[serde(default)]
    pub rebuild: u64,
    pub owner: Owner,
    pub definition: Definition,
    #[serde(default)]
    pub paused: bool,
    pub progress: Progress,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Keyword,
    Semantic,
    #[default]
    Hybrid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub query: String,
    #[serde(default)]
    pub collections: Vec<KnowledgeId>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub modified_after_ms: Option<u64>,
    #[serde(default)]
    pub modified_before_ms: Option<u64>,
    #[serde(default = "default_limit")]
    pub limit: u16,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u16 {
    DEFAULT_LIMIT
}

fn filters(values: &[String]) -> bool {
    values
        .iter()
        .all(|value| !value.is_empty() && !value.contains('\0'))
}

impl Query {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.query.trim().is_empty()
            || self.query.contains('\0')
            || !filters(&self.tags)
            || self.limit == 0
            || self.path.as_ref().is_some_and(|path| path.contains('\0'))
            || matches!((self.modified_after_ms, self.modified_before_ms), (Some(after), Some(before)) if after > before)
        {
            return Err(Fault::invalid("invalid knowledge query or filters"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Position {
    Lines {
        start: u32,
        end: u32,
    },
    Page {
        page: u32,
    },
    Cells {
        sheet: String,
        start: String,
        end: String,
    },
    Slide {
        slide: u32,
    },
    Section {
        name: String,
    },
    Value {
        key: String,
        pointer: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Citation {
    pub collection: KnowledgeId,
    pub chunk: String,
    pub revision: String,
    pub path: String,
    pub position: Position,
    /// Generated by the execution Node from the indexed source identity.
    pub link: String,
}

impl Citation {
    pub fn url(&self) -> String {
        format!(
            "sailry-knowledge://{}/{}?revision={}",
            self.collection, self.chunk, self.revision
        )
    }
}

fn citation_limit() -> u32 {
    16 * 1024
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitationRequest {
    pub collection: KnowledgeId,
    pub chunk: String,
    pub revision: String,
    /// Return the complete source as paged context instead of only its cited position.
    #[serde(default)]
    pub context: bool,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "citation_limit")]
    pub limit: u32,
}

impl CitationRequest {
    pub fn validate(&self) -> Result<(), Fault> {
        let hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        if !hash(&self.chunk) || !hash(&self.revision) || self.limit == 0 {
            return Err(Fault::invalid("invalid knowledge citation request"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationPage {
    pub citation: Citation,
    pub title: String,
    pub text: String,
    pub offset: u32,
    pub next_offset: Option<u32>,
    pub worktree: Option<WorktreeId>,
    pub context: bool,
    pub untrusted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    pub title: String,
    pub text: String,
    /// Retrieval score in millionths; ordering does not depend on JSON float handling.
    pub score: u32,
    pub citation: Citation,
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub hits: Vec<Hit>,
    pub next_offset: Option<u32>,
    pub mode: Mode,
    pub indexing: bool,
    pub model: Option<Model>,
    pub untrusted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confines_plugin_drafts_to_captured_sources() {
        let draft: Draft =
            serde_json::from_str(r#"{"name":"Notes","source":{"kind":"worktree","path":"notes"}}"#)
                .unwrap();
        assert!(draft.clone().into_definition(None).is_err());
        let worktree = WorktreeId::new();
        let definition = draft.into_definition(Some(worktree)).unwrap();
        assert_eq!(
            definition.source,
            Source::Worktree {
                worktree,
                path: "notes".into()
            }
        );
        assert!(definition.shared_with.is_empty());
        for value in [
            serde_json::json!({"name":"Notes","source":{"kind":"directory","path":"/tmp"}}),
            serde_json::json!({"name":"Notes","source":{"kind":"file","path":"/tmp/note"}}),
            serde_json::json!({"name":"Notes","source":{"kind":"worktree","path":"notes","worktree":worktree}}),
            serde_json::json!({"name":"Notes","source":{"kind":"storage","scope":"node","prefix":"","owner":"other"}}),
            serde_json::json!({"name":"Notes","source":{"kind":"storage","scope":"node","prefix":""},"shared_with":["other"]}),
        ] {
            assert!(serde_json::from_value::<Draft>(value).is_err());
        }
        let draft: Draft = serde_json::from_str(
            r#"{"name":"Notes","source":{"kind":"storage","scope":"conversation","prefix":"notes/"}}"#,
        ).unwrap();
        let definition = draft.into_definition(None).unwrap();
        assert_eq!(
            definition.source,
            Source::Storage {
                scope: Scope::Conversation,
                prefix: "notes/".into()
            }
        );
    }

    #[test]
    fn preserves_explicit_filters_and_rejects_unknown_fields() {
        let query: Query = serde_json::from_str(r#"{"query":"local source"}"#).unwrap();
        assert_eq!(query.mode, Mode::Hybrid);
        assert_eq!(query.limit, DEFAULT_LIMIT);
        query.validate().unwrap();
        assert!(serde_json::from_str::<Query>(r#"{"query":"local","owner":"other"}"#).is_err());
        let mut expanded = query.clone();
        expanded.limit = u16::MAX;
        expanded.offset = 10_001;
        expanded.query = "q".repeat(8193);
        expanded.tags = vec!["tag".repeat(129); 33];
        expanded.validate().unwrap();
        let mut invalid = query.clone();
        invalid.limit = 0;
        assert!(invalid.validate().is_err());
        let mut invalid = query;
        invalid.modified_after_ms = Some(20);
        invalid.modified_before_ms = Some(10);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn keeps_source_positions_separate_from_answer_offsets() {
        let mut citation = Citation {
            collection: KnowledgeId::new(),
            chunk: "1".repeat(64),
            revision: "2".repeat(64),
            path: "report.xlsx".into(),
            position: Position::Cells {
                sheet: "Data".into(),
                start: "A1".into(),
                end: "B5".into(),
            },
            link: String::new(),
        };
        citation.link = citation.url();
        let json = serde_json::to_string(&citation).unwrap();
        assert_eq!(serde_json::from_str::<Citation>(&json).unwrap(), citation);
        assert!(json.contains("sailry-knowledge://"));
        assert!(!citation.link.contains("report.xlsx"));
    }

    #[test]
    fn validates_citation_identity_and_page_bounds() {
        let valid = CitationRequest {
            context: false,
            collection: KnowledgeId::new(),
            chunk: "1".repeat(64),
            revision: "2".repeat(64),
            offset: 0,
            limit: 64 * 1024,
        };
        valid.validate().unwrap();
        let mut invalid = valid.clone();
        invalid.chunk = "../private".into();
        assert!(invalid.validate().is_err());
        invalid = valid.clone();
        invalid.revision = "A".repeat(64);
        assert!(invalid.validate().is_err());
        invalid = valid.clone();
        invalid.limit += 1;
        invalid.validate().unwrap();
        invalid.limit = 0;
        assert!(invalid.validate().is_err());
        let mut value = serde_json::to_value(valid).unwrap();
        value["path"] = serde_json::json!("/private/source");
        assert!(serde_json::from_value::<CitationRequest>(value).is_err());
    }

    #[test]
    fn defaults_optional_collection_bindings() {
        let definition: Definition = serde_json::from_str(
            r#"{"name":"Notes","source":{"kind":"storage","scope":"node","prefix":""}}"#,
        )
        .unwrap();
        assert_eq!(definition.model, None);
        assert!(definition.globs.is_empty());
        definition.validate().unwrap();
        let mut invalid = definition.clone();
        invalid.globs = vec!["**/*.md".into()];
        assert!(invalid.validate().is_err());
        assert!(serde_json::from_str::<Definition>(r#"{"name":"Notes","source":{"kind":"storage","scope":"node","prefix":""},"owner":"other"}"#).is_err());
        let command = crate::Command::CreateKnowledgeCollection {
            definition,
            plugin: None,
        };
        assert!(command.durable());
        assert!(!crate::Command::ListKnowledgeCollections.durable());
        assert!(!crate::Command::ReadKnowledgeSettings.durable());
        let encoded = serde_json::to_string(&command).unwrap();
        assert_eq!(
            serde_json::from_str::<crate::Command>(&encoded).unwrap(),
            command
        );
    }
}
