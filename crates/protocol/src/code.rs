//! Bounded read-only language-service requests on the execution Node.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Startup {
    pub worktree: crate::WorktreeId,
    pub root: String,
    pub server: String,
    pub executable: String,
    /// Binds the executable, arguments, configuration and environment without exposing secrets.
    pub revision: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Starting,
    Indexing,
    Ready,
    Failed,
    Stopped,
    NotInstalled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub startup: Startup,
    pub status: Status,
    pub progress: Option<String>,
    /// Changes only when the server publishes different diagnostics.
    pub diagnostics_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Query {
    Diagnostics {
        path: String,
        severity: Option<Severity>,
    },
    Definition {
        path: String,
        line: u32,
        symbol: String,
    },
    References {
        path: String,
        line: u32,
        symbol: String,
    },
    Hover {
        path: String,
        line: u32,
        symbol: String,
    },
    DefinitionAt {
        path: String,
        position: Position,
    },
    HoverAt {
        path: String,
        position: Position,
    },
    Completion {
        path: String,
        position: Position,
    },
    Symbols {
        path: Option<String>,
        query: Option<String>,
    },
}

impl Query {
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Diagnostics { path, .. }
            | Self::Definition { path, .. }
            | Self::References { path, .. }
            | Self::Hover { path, .. }
            | Self::DefinitionAt { path, .. }
            | Self::HoverAt { path, .. }
            | Self::Completion { path, .. } => Some(path),
            Self::Symbols { path, .. } => path.as_deref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    pub path: String,
    pub expected_revision: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
    pub start: Position,
    pub end: Position,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub location: Location,
    pub severity: Severity,
    pub message: String,
    pub source: Option<String>,
    pub code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: u32,
    pub location: Location,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Content {
    Diagnostics(Vec<Diagnostic>),
    Locations(Vec<Location>),
    Hover(String),
    Symbols(Vec<Symbol>),
    Completions(Vec<Completion>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub range: Option<Location>,
    pub kind: Option<u32>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result {
    pub status: Status,
    pub server: Option<String>,
    pub revision: Option<String>,
    pub content: Content,
    pub partial: bool,
    pub notice: Option<String>,
    /// A package suggestion does not install anything or grant installation consent.
    pub package: Option<String>,
    pub package_info: Option<Package>,
    pub startup: Option<Startup>,
    pub startup_approved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub editor_enabled: bool,
    pub after_edit: bool,
    pub servers: std::collections::BTreeMap<String, Override>,
    pub languages: std::collections::BTreeMap<String, Vec<String>>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            enabled: true,
            editor_enabled: false,
            after_edit: true,
            servers: Default::default(),
            languages: Default::default(),
        }
    }
}
fn enabled() -> bool {
    true
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    pub disabled: bool,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub config: Option<serde_json::Value>,
    pub environment: Option<std::collections::BTreeMap<String, String>>,
}

/// Exact installation consent derived from the execution Node's current registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub package: String,
    pub version: String,
    pub purl: String,
    pub urls: Vec<String>,
    pub licenses: Vec<String>,
    pub command: Option<Vec<String>>,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub homepage: Option<String>,
    pub licenses: Vec<String>,
    pub install: Option<Install>,
    pub unavailable: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub server: String,
    pub command: String,
    pub disabled: bool,
    pub status: Status,
    pub package: Option<Package>,
    pub startup: Option<Startup>,
    pub startup_approved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Language {
    pub name: String,
    pub id: String,
    pub servers: Vec<Candidate>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub worktree: crate::WorktreeId,
    pub languages: Vec<Language>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub url: String,
    pub sha256: String,
    /// True only when an upstream checksum was found and matched.
    pub verified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub package: String,
    pub version: String,
    pub purl: String,
    pub revision: String,
    pub downloads: Vec<Download>,
    pub installed_at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Preparing,
    Downloading,
    Installing,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installation {
    pub package: String,
    pub status: InstallStatus,
    pub progress: Option<String>,
    pub receipt: Option<Receipt>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    pub helix_commit: String,
    pub mason_tag: String,
    pub retrieved: String,
    pub cached: bool,
}

#[cfg(test)]
mod tests;
