//! Execution-Node plugin inventory and versioned desktop resources.
use serde::{Deserialize, Serialize};

mod action;
pub use action::{Access, Action};
pub mod card;
pub mod catalog;
pub mod connection;
pub mod connector;
pub mod conversation;
pub mod desktop;
pub mod host;
pub mod http;
pub mod mcp;
pub mod model_adapter;
pub mod model_auth;
pub mod model_tools;
pub mod models;
pub mod modes;
pub mod schema;
pub mod settings;
pub mod skillcatalog;
pub mod skills;
pub mod storage;
pub mod transaction;
pub mod ui;
pub mod updates;

pub const MAX_INSTALLED: usize = 64;
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

impl Context {
    /// A controller's Node-wide settings surface, never an Agent or callback scope.
    pub fn controller_settings(&self) -> bool {
        self.surface == desktop::Surface::Settings
            && self.invocation.is_none()
            && self.turn.is_none()
            && self.session.is_none()
            && self.worktree.is_none()
    }
}

/// One tool-free model response; credentials remain on the execution Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    pub model: String,
    pub tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Development,
    Documents,
    Data,
    Automation,
    Entertainment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tint {
    Blue,
    Violet,
    Teal,
    Amber,
    Gray,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Extension {
    #[schemars(extend("const" = API_VERSION))]
    pub api_version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<crate::permissions::Contribution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<connector::Declaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<Tint>,
    /// Package-owned instructions supplied once with its available tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Localized controller marker for this package's automatic continuations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation_marker: Option<desktop::Navigation>,
    /// Shared HTML resource surfaces load this package-owned presentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_view: Option<crate::artifact::View>,
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
    /// Optional exclusive binding between package enablement and a Node service.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_service: Option<Service>,
    pub desktop: Option<desktop::Manifest>,
    pub host: Option<host::Manifest>,
    #[schemars(transform = schema::resource_path)]
    pub settings_schema: Option<String>,
    /// Public package-owned options frozen with each conversation turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_options: Option<serde_json::Value>,
    /// Package-owned work modes bound to declared conversation options.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversation_modes: Vec<modes::Declaration>,
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
    /// Channel declarations supplied by this package.
    #[serde(default)]
    pub model_providers: Vec<crate::conversation::presets::Preset>,
    #[serde(default)]
    pub model_auth: Vec<model_auth::Declaration>,
    /// Named read-only resources for native provider accounts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_resources: Vec<model_auth::Resource>,
    #[serde(default)]
    pub model_adapters: Vec<model_adapter::Declaration>,
    /// Controller-rendered entries with state and named action handlers.
    #[serde(default)]
    pub ui: Vec<ui::Contribution>,
    /// Inline controllers are captured package resources, not model-authored UI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cards: Vec<card::Declaration>,
}

/// A service binding grants no command capabilities by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    CodeIndex,
    // Retained only when reading immutable historical package declarations.
    #[schemars(skip)]
    Language,
}

impl Service {
    pub fn current(self) -> bool {
        self == Self::CodeIndex
    }
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
            && self
                .connectors
                .iter()
                .flat_map(|connector| &connector.auth)
                .all(|auth| match auth {
                    connector::Auth::Application {
                        registration: Some(registration),
                        ..
                    } => match &registration.flow {
                        model_auth::Flow::Script { script, .. } => {
                            contains(&script.entry)
                                && script.resources.iter().all(|path| contains(path))
                        }
                        _ => true,
                    },
                    _ => true,
                })
            && self.cards.iter().all(|card| contains(&card.entry))
            && self
                .continuation_marker
                .as_ref()
                .is_none_or(desktop::Navigation::valid)
            && self.artifact_view.as_ref().is_none_or(|view| view.valid())
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<catalog::ReleaseNotes>,
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
    Registry {
        source: skills::RegistryResolved,
    },
    Worktree {
        worktree: crate::WorktreeId,
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub display_locales: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub description: String,
    /// Relative to the immutable package root, not a controller filesystem path.
    pub path: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
}

impl Skill {
    pub fn display_name(&self, locale: &str) -> &str {
        self.display_locales
            .get(locale)
            .map(String::as_str)
            .or(self.display_name.as_deref())
            .unwrap_or(&self.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mcp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub name: String,
    pub transport: McpTransport,
    #[serde(default = "mcp::default_allowed")]
    pub allowed: bool,
    #[serde(default)]
    pub availability: mcp::Availability,
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
