//! Tool-owned output presentation shared by every controller and plugin origin.
use serde::{Deserialize, Serialize};
mod content;
pub use content::{Block, Content, Diff, File, Table, Text};
mod display;
mod result;
pub use display::{Display, Field, Input, projection};
pub use result::{Diagnostic, LocalText, ResultDisplay};

/// Controls availability of successful result details, not automatic expansion state.
/// Failures remain inspectable regardless of the declared presentation.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Presentation {
    Summary,
    /// A validated progress result, with raw details retained as a fallback.
    Progress,
    /// Bounded standard content, with the original result retained for inspection.
    Content,
    /// Unknown presentation modes keep the original result inspectable.
    #[default]
    #[serde(other)]
    Details,
}

/// Whether a tool joins consecutive activity or occupies its own transcript row.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    #[default]
    Sequence,
    Standalone,
}

/// A plugin can configure only its own named tool; native tools omit the MCP server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Declaration {
    pub name: String,
    pub server: Option<String>,
    #[serde(default)]
    pub presentation: Presentation,
    #[serde(default)]
    pub grouping: Grouping,
    /// Optional native host operation; MCP remains an independent provider.
    pub operation: Option<Operation>,
    /// A named export from the package's bounded headless entry.
    pub handler: Option<Handler>,
    pub description: Option<String>,
    /// Captured with tool calls so history does not depend on an active plugin view.
    pub display: Option<Display>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
    /// Empty means all session contexts; restrictions follow the captured resource.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<crate::plugin::conversation::Resource>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub platform_descriptions: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Handler {
    pub name: String,
    pub parameters: serde_json::Value,
    /// Plain-script read-only declaration; the Node still enforces frozen Plan write restrictions.
    #[serde(default)]
    pub read_only: bool,
    /// The export prepares arguments; the Node executes and waits outside the VM.
    pub operation: Option<Operation>,
    /// A bounded sequence of explicit operations, resumed outside each fresh VM.
    pub flow: Option<Flow>,
    /// Optional export receiving the original arguments and completed Node output.
    pub result: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Flow {
    pub operations: Vec<Operation>,
    /// Maximum Unicode scalar values requested from the frozen user message.
    pub message_chars: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Operation {
    #[serde(rename = "git.status")]
    InspectGit,
    #[serde(rename = "git.diff")]
    ReadGitDiff,
    #[serde(rename = "git.log")]
    ReadGitLog,
    #[serde(rename = "worktrees.list")]
    ListWorktrees,
    #[serde(rename = "worktrees.create")]
    CreateManagedWorktree,
    #[serde(rename = "worktrees.register")]
    RegisterWorktree,
    #[serde(rename = "worktrees.remove")]
    RemoveWorktree,
    #[serde(rename = "databases.catalog")]
    BrowseDatabase,
    #[serde(rename = "databases.query")]
    QueryDatabase,
    #[serde(rename = "databases.execute")]
    ExecuteDatabase,
    #[serde(rename = "ssh.run")]
    RunSsh,
    #[serde(rename = "ssh.transfer")]
    TransferSsh,
    #[serde(rename = "agents.delegate")]
    DelegateAgent,
    #[serde(rename = "files.list")]
    ListDirectory,
    #[serde(rename = "files.read")]
    ReadFile,
    #[serde(rename = "files.write")]
    WriteFile,
    #[serde(rename = "files.search")]
    SearchFiles,
    #[serde(rename = "office.runtime")]
    OfficeRuntime,
    #[serde(rename = "office.read")]
    ReadOffice,
    #[serde(rename = "office.export")]
    ExportPdf,
    #[serde(rename = "media.inspect")]
    InspectMedia,
    #[serde(rename = "media.image")]
    GenerateImage,
    #[serde(rename = "media.video")]
    GenerateVideo,
    #[serde(rename = "computer.read")]
    ReadComputer,
    #[serde(rename = "computer.control")]
    ControlComputer,
    #[serde(rename = "browser.read")]
    ReadBrowser,
    #[serde(rename = "browser.control")]
    ControlBrowser,
    #[serde(rename = "external_browser.read")]
    ReadExternalBrowser,
    #[serde(rename = "external_browser.control")]
    ControlExternalBrowser,
    #[serde(rename = "http.request")]
    Http,
    #[serde(rename = "progress.update")]
    Progress,
    #[serde(rename = "storage.get")]
    GetValue,
    #[serde(rename = "storage.list")]
    ListKeys,
    #[serde(rename = "storage.set")]
    SetValue,
    #[serde(rename = "storage.delete")]
    DeleteValue,
    #[serde(rename = "storage.transaction")]
    StorageTransaction,
    #[serde(rename = "settings.read")]
    ReadSettings,
    #[serde(rename = "process.run")]
    RunCommand,
    #[serde(rename = "process.read")]
    ReadCommand,
    #[serde(rename = "process.stop")]
    StopCommand,
    /// Unknown operations are inert, including inside an otherwise supported flow.
    #[serde(other)]
    #[schemars(skip)]
    Unsupported,
}

impl Operation {
    pub fn action(self) -> Option<crate::plugin::Action> {
        use crate::plugin::Action;
        match self {
            Self::InspectGit | Self::ReadGitDiff | Self::ReadGitLog => Some(Action::ReadGit),
            Self::ListWorktrees => Some(Action::ReadWorktrees),
            Self::CreateManagedWorktree | Self::RegisterWorktree | Self::RemoveWorktree => {
                Some(Action::WriteWorktrees)
            }
            Self::BrowseDatabase | Self::QueryDatabase => Some(Action::ReadDatabases),
            Self::ExecuteDatabase => Some(Action::ControlDatabases),
            Self::RunSsh | Self::TransferSsh => Some(Action::ControlSsh),
            Self::DelegateAgent => Some(Action::DelegateAgents),
            Self::ListDirectory
            | Self::ReadFile
            | Self::SearchFiles
            | Self::OfficeRuntime
            | Self::ReadOffice => Some(Action::ReadFiles),
            Self::WriteFile | Self::ExportPdf => Some(Action::WriteFiles),
            Self::InspectMedia => Some(Action::InspectMedia),
            Self::GenerateImage => Some(Action::GenerateImage),
            Self::GenerateVideo => Some(Action::GenerateVideo),
            Self::ReadComputer => Some(Action::ReadComputer),
            Self::ControlComputer => Some(Action::ControlComputer),
            Self::Http => Some(Action::Http),
            Self::ReadBrowser => Some(Action::ReadBrowser),
            Self::ControlBrowser => Some(Action::ControlBrowser),
            Self::ReadExternalBrowser => Some(Action::ReadExternalBrowser),
            Self::ControlExternalBrowser => Some(Action::ControlExternalBrowser),
            Self::Progress | Self::ReadSettings | Self::Unsupported => None,
            Self::GetValue | Self::ListKeys => Some(Action::ReadStorage),
            Self::SetValue | Self::DeleteValue | Self::StorageTransaction => {
                Some(Action::WriteStorage)
            }
            Self::ReadCommand => Some(Action::ReadCommands),
            Self::RunCommand | Self::StopCommand => Some(Action::ControlCommands),
        }
    }

    pub fn read_only(self) -> bool {
        !matches!(
            self,
            Self::Unsupported
                | Self::DelegateAgent
                | Self::CreateManagedWorktree
                | Self::RegisterWorktree
                | Self::RemoveWorktree
                | Self::ExecuteDatabase
                | Self::RunSsh
                | Self::TransferSsh
                | Self::WriteFile
                | Self::ExportPdf
                | Self::GenerateImage
                | Self::GenerateVideo
                | Self::Http
                | Self::ControlComputer
                | Self::SetValue
                | Self::DeleteValue
                | Self::StorageTransaction
                | Self::RunCommand
                | Self::StopCommand
                | Self::ControlExternalBrowser
                | Self::ControlBrowser
        )
    }
}

impl Declaration {
    pub fn available_in(
        &self,
        project: Option<crate::ProjectId>,
        resource: Option<crate::connection::Resource>,
    ) -> bool {
        self.contexts.is_empty()
            || self
                .contexts
                .iter()
                .any(|context| context.matches(project, resource))
    }

    pub fn available_on(&self, platform: &str) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|value| value == platform)
    }

    pub fn description_on(&self, platform: &str) -> Option<&str> {
        self.platform_descriptions
            .get(platform)
            .map(String::as_str)
            .or(self.description.as_deref())
    }

    pub fn valid(&self) -> bool {
        self.operation != Some(Operation::Unsupported)
            && self.handler.as_ref().is_none_or(|handler| {
                crate::plugin::host::valid_handler(&handler.name)
                    && handler.flow.as_ref().is_none_or(|flow| {
                        handler.operation.is_none()
                            && handler.result.is_none()
                            && flow.message_chars.is_none_or(|count| {
                                (1..=crate::plugin::host::MAX_DATA_BYTES / 4).contains(&count)
                            })
                            && !flow.operations.is_empty()
                            && flow.operations.len() <= 8
                            && flow
                                .operations
                                .iter()
                                .enumerate()
                                .all(|(index, operation)| {
                                    // One file checkpoint consumes one exact approved write.
                                    !matches!(
                                        operation,
                                        Operation::Unsupported
                                            | Operation::Progress
                                            | Operation::DelegateAgent
                                            | Operation::ReadSettings
                                            | Operation::WriteFile
                                    ) && !flow.operations[..index].contains(operation)
                                })
                    })
                    && handler.operation.is_none_or(|operation| {
                        !matches!(
                            operation,
                            Operation::Unsupported | Operation::Progress | Operation::ReadSettings
                        )
                    })
                    && handler.result.as_ref().is_none_or(|name| {
                        handler.operation.is_some() && crate::plugin::host::valid_handler(name)
                    })
                    && self.operation.is_none()
                    && self.server.is_none()
                    && self.description.is_some()
                    && handler.parameters.is_object()
                    && handler.parameters["type"] == "object"
                    && serde_json::to_vec(&handler.parameters)
                        .is_ok_and(|bytes| bytes.len() <= 16 * 1024)
            })
            && self.contexts.len() <= 4
            && self
                .contexts
                .iter()
                .enumerate()
                .all(|(index, context)| !self.contexts[..index].contains(context))
            && self.platforms.len() <= 3
            && self
                .platforms
                .iter()
                .all(|platform| matches!(platform.as_str(), "macos" | "windows" | "linux"))
            && self.platform_descriptions.len() <= 3
            && self
                .platform_descriptions
                .iter()
                .all(|(platform, description)| {
                    matches!(platform.as_str(), "macos" | "windows" | "linux")
                        && !description.trim().is_empty()
                        && description.len() <= 4096
                })
            && self.display.as_ref().is_none_or(Display::valid)
            && !self.name.is_empty()
            && self.name.len() <= 512
            && !self.name.chars().any(char::is_control)
            && self.server.as_ref().is_none_or(|server| {
                !server.is_empty() && server.len() <= 256 && !server.chars().any(char::is_control)
            })
            && self
                .description
                .as_ref()
                .is_none_or(|text| !text.trim().is_empty() && text.len() <= 4096)
            && (self.operation.is_none() && self.handler.is_none() || {
                self.server.is_none()
                    && self.name.len() <= 40
                    && self
                        .name
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    && (self.operation != Some(Operation::Progress)
                        || self.presentation == Presentation::Progress)
            })
    }
}

#[cfg(test)]
mod handler_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn declares_read_only() {
        let mut value = json!({"name":"read","parameters":{"type":"object"}});
        let handler: Handler = serde_json::from_value(value.clone()).unwrap();
        assert!(!handler.read_only);
        value["read_only"] = json!(true);
        let handler: Handler = serde_json::from_value(value.clone()).unwrap();
        assert!(handler.read_only);
        value["read_only"] = json!("true");
        assert!(serde_json::from_value::<Handler>(value).is_err());
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn unknown_operations_disable_the_whole_declaration() {
        let operation: Operation = serde_json::from_value(json!("future.operation")).unwrap();
        assert_eq!(operation, Operation::Unsupported);
        assert_eq!(operation.action(), None);
        assert!(!operation.read_only());
        for value in [
            json!({"name":"fixture","operation":"future.operation"}),
            json!({"name":"fixture","description":"Fixture","handler":{"name":"fixture","parameters":{"type":"object"},"operation":"future.operation"}}),
            json!({"name":"fixture","description":"Fixture","handler":{"name":"fixture","parameters":{"type":"object"},"flow":{"operations":["files.read","future.operation"]}}}),
        ] {
            let declaration: Declaration = serde_json::from_value(value).unwrap();
            assert!(!declaration.valid());
        }
        assert!(serde_json::from_value::<Operation>(json!(1)).is_err());
    }

    #[test]
    fn extra_metadata_preserves_supported_operations() {
        let value = json!({"name":"fixture","description":"Fixture","presentation":"future_presentation","future_metadata":true,"handler":{"name":"fixture","parameters":{"type":"object"},"future_metadata":{},"flow":{"operations":["files.read"],"future_metadata":[]}}});
        let declaration: Declaration = serde_json::from_value(value).unwrap();
        assert!(declaration.valid());
        assert_eq!(declaration.presentation, Presentation::Details);
        assert!(serde_json::from_value::<Presentation>(json!(true)).is_err());
        let schema = serde_json::to_value(schemars::schema_for!(Operation)).unwrap();
        let variants = schema["oneOf"].as_array().unwrap();
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0]["type"], "string");
        let names = variants[0]["enum"].as_array().unwrap();
        assert!(!names.is_empty());
        assert!(names.contains(&json!("files.read")));
        assert!(!names.contains(&json!("Unsupported")));
        for name in names {
            assert_ne!(
                serde_json::from_value::<Operation>(name.clone()).unwrap(),
                Operation::Unsupported
            );
        }
    }
    #[test]
    fn validates_declared_operations_and_platforms() {
        let value = json!({"name":"choose","description":"Select an offered action","handler":{"name":"choose","parameters":{"type":"object"},"flow":{"operations":["computer.read","computer.control"]}},"platforms":["macos"],"platform_descriptions":{"macos":"Select a window action"}});
        let declaration: Declaration = serde_json::from_value(value.clone()).unwrap();
        assert!(declaration.valid());
        assert!(declaration.available_on("macos"));
        assert!(!declaration.available_on("linux"));
        assert_eq!(
            declaration.description_on("macos"),
            Some("Select a window action")
        );
        for change in [
            json!([]),
            json!(["computer.read", "computer.read"]),
            json!(["settings.read"]),
            json!(["files.write"]),
        ] {
            let mut malformed = value.clone();
            malformed["handler"]["flow"]["operations"] = change;
            assert!(
                serde_json::from_value::<Declaration>(malformed)
                    .map_or(true, |declaration| !declaration.valid())
            );
        }
        let mut conflicting = value;
        conflicting["handler"]["operation"] = json!("computer.read");
        assert!(
            !serde_json::from_value::<Declaration>(conflicting)
                .unwrap()
                .valid()
        );
    }
    #[test]
    fn preserves_handler_platforms() {
        let extension: crate::plugin::Extension = serde_json::from_value(json!({
            "api_version":"v1",
            "instructions":"Fixture instructions",
            "tools":[
                {"name":"read","description":"Read fixture","handler":{
                    "name":"read","parameters":{"type":"object"},"operation":"computer.read"
                }},
                {"name":"open","description":"Open fixture","platforms":["macos"],"handler":{
                    "name":"open","parameters":{"type":"object"},"operation":"computer.control"
                }}
            ]
        }))
        .unwrap();
        assert_eq!(extension.tools.len(), 2);
        assert!(extension.tools.iter().all(Declaration::valid));
        assert!(
            extension
                .instructions
                .as_ref()
                .is_some_and(|instructions| !instructions.is_empty())
        );
        let open = extension
            .tools
            .iter()
            .find(|tool| tool.name == "open")
            .unwrap();
        assert!(open.available_on("macos"));
        assert!(!open.available_on("windows"));
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;
    use crate::{DatabaseId, ProjectId, SshId, connection, plugin::conversation::Resource};
    use serde_json::json;

    #[test]
    fn selects_captured_resources() {
        let mut declaration: Declaration = serde_json::from_value(json!({
            "name":"read","operation":"files.read","description":"Read a file",
            "contexts":["plugin","workspace"]
        }))
        .unwrap();
        assert!(declaration.valid());
        assert!(declaration.available_in(None, None));
        assert!(declaration.available_in(Some(ProjectId::new()), None));
        assert!(!declaration.available_in(
            None,
            Some(connection::Resource::Database(DatabaseId::new()))
        ));
        assert!(!declaration.available_in(None, Some(connection::Resource::Ssh(SshId::new()))));
        declaration.contexts.push(Resource::Plugin);
        assert!(!declaration.valid());
        declaration.contexts.clear();
        assert!(declaration.available_in(None, Some(connection::Resource::Ssh(SshId::new()))));
    }
}
