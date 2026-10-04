//! Execution-Node plugin inventory and versioned desktop resources.
use serde::{Deserialize, Serialize};

mod action;
pub use action::Action;
pub mod authorization;
pub mod catalog;
pub mod conversation;
pub mod desktop;
pub mod host;
pub mod http;
pub mod mcp;
pub mod model_tools;
pub mod models;
pub mod schema;
pub mod settings;
pub mod skills;
pub mod storage;
pub mod transaction;
pub mod ui;
pub mod updates;

pub const MAX_INSTALLED: usize = 32;
pub const API_VERSION: &str = "v1";
pub const NAMESPACE: &str = "dev.sailry.platform";
pub const MAX_PACKAGE_BYTES: u64 = 128 * 1024 * 1024;

/// ZIP bytes selected on the controller, independent of any project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadSpec {
    pub size: u64,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    pub stream: crate::StreamId,
    pub spec: UploadSpec,
}

/// Package provenance and the resource scope captured by a desktop extension.
/// This narrows an authenticated controller's request; it is not an authorization token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    /// A running host callback uses the package frozen in its original admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<crate::RequestId>,
    /// An admitted Agent turn uses its frozen package and configuration revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<crate::TurnId>,
    #[serde(default)]
    pub surface: desktop::Surface,
    pub package: Reference,
    pub worktree: Option<crate::WorktreeId>,
    pub session: Option<crate::SessionId>,
}

/// One tool-free model response; credentials remain on the execution Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    pub model: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Extension {
    #[schemars(extend("const" = API_VERSION))]
    pub api_version: String,
    /// Package-owned instructions supplied once with its available tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Localized catalog name, independent of optional UI entry points.
    pub display: Option<desktop::Navigation>,
    /// Package-relative PNG used by controller catalogs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(transform = schema::resource_path)]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<desktop::Navigation>,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub actions: Vec<Action>,
    pub desktop: Option<desktop::Manifest>,
    pub host: Option<host::Manifest>,
    #[schemars(transform = schema::resource_path)]
    pub settings_schema: Option<String>,
    /// Settings navigation and an optional custom desktop entry.
    pub settings_page: Option<desktop::Settings>,
    /// Inline schemas for package-owned Node and conversation values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<storage::Declaration>,
    /// Output presentation for tools provided by this package.
    #[serde(default)]
    pub tools: Vec<crate::tool::Declaration>,
    /// Provider-executed capabilities; these are not local function handlers.
    #[serde(default)]
    pub model_tools: Vec<model_tools::Declaration>,
    /// Controller-rendered entries with state and named action handlers.
    #[serde(default)]
    pub ui: Vec<ui::Contribution>,
}

impl Extension {
    /// Agent Plugins reserves client-specific files to their namespace directory.
    pub fn files_in_namespace(&self) -> bool {
        let contains = |path: &str| {
            let path = path.strip_prefix("./").unwrap_or(path);
            path.strip_prefix(NAMESPACE)
                .and_then(|suffix| suffix.strip_prefix('/'))
                .is_some_and(|suffix| !suffix.is_empty())
        };
        self.icon.as_deref().is_none_or(contains)
            && self.settings_schema.as_deref().is_none_or(contains)
            && self
                .settings_page
                .as_ref()
                .is_none_or(|page| page.entry.as_deref().is_none_or(contains))
            && self.desktop.as_ref().is_none_or(|desktop| {
                desktop.entry.as_deref().is_none_or(contains)
                    && desktop.ui_entry.as_deref().is_none_or(contains)
                    && desktop.resources.iter().all(|path| contains(path))
            })
            && self.host.as_ref().is_none_or(|host| {
                contains(&host.entry) && host.resources.iter().all(|path| contains(path))
            })
    }
}

/// Desktop contributions stay bound to the controller's local Node.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Host,
    Desktop,
}

/// An execution-Node package version frozen into the effective session revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub digest: String,
    /// Configuration revision on the execution Node; zero selects package defaults.
    pub settings_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub name: String,
    pub revision: u64,
    /// Content digest for installed packages; empty for compiled-in capabilities.
    pub digest: String,
    pub settings_revision: u64,
    #[serde(default)]
    pub enabled: bool,
    pub version: Option<String>,
    pub description: Option<String>,
}

impl Summary {
    pub fn reference(&self) -> Reference {
        Reference {
            name: self.name.clone(),
            digest: self.digest.clone(),
            settings_revision: self.settings_revision,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Info {
    pub summary: Summary,
    /// Installation provenance of the current inventory entry, not package content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Bounded base64 PNG read from the declared package icon.
    pub icon: Option<String>,
    #[serde(default)]
    pub skill: Option<skills::Provenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_source: Option<mcp::Definition>,
    pub extension: Option<Extension>,
    #[serde(default)]
    pub skills: Vec<Skill>,
    /// Valid declarations, not proof that an execution dependency is running.
    #[serde(default)]
    pub mcp: Vec<Mcp>,
    /// Validated form metadata, never configured values or credentials.
    pub settings: Option<settings::Schema>,
    #[serde(default)]
    pub issues: Vec<Issue>,
}

/// Controller uploads record their source kind, never a controller filesystem path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadSource {
    Directory,
    Archive,
}

/// Derived by the execution Node from the admitted installation command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    Bundled,
    Directory,
    Archive,
    Online {
        source: skills::Resolved,
        path: String,
    },
    Worktree {
        worktree: crate::WorktreeId,
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub description: String,
    /// Relative to the immutable package root, not a controller filesystem path.
    pub path: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mcp {
    pub name: String,
    pub transport: McpTransport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpTransport {
    Stdio,
    StreamableHttp,
    Sse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    IgnoredManifestField,
    InvalidExtensions,
    UnsupportedExtension,
    UnavailablePath,
    InvalidSkills,
    InvalidSkill,
    InvalidMcp,
    InvalidMcpServer,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub path: String,
    pub kind: IssueKind,
}

#[cfg(test)]
mod tests;

/// Icons use the same confined resource paths as desktop bundles.
pub fn valid_icon(path: &Option<String>) -> bool {
    path.as_deref()
        .is_none_or(|path| path.ends_with(".png") && desktop::valid_paths(std::iter::once(path)))
}
