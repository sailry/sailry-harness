//! Host capabilities declared by plugin extensions.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Action {
    #[serde(rename = "artifacts.read")]
    ReadArtifacts,
    #[serde(rename = "artifacts.write")]
    WriteArtifacts,
    #[serde(rename = "code.index.read")]
    ReadCodeIndex,
    #[serde(rename = "code.index.settings.read")]
    ReadCodeIndexSettings,
    #[serde(rename = "code.index.settings.write")]
    SaveCodeIndexSettings,
    // Retained only when reading immutable historical package declarations.
    #[schemars(skip)]
    #[serde(rename = "code.read")]
    ReadCode,
    #[schemars(skip)]
    #[serde(rename = "code.manage")]
    ManageCode,
    #[schemars(skip)]
    #[serde(rename = "code.settings.read")]
    ReadCodeSettings,
    #[schemars(skip)]
    #[serde(rename = "code.settings.write")]
    SaveCodeSettings,
    #[serde(rename = "web.settings.read")]
    ReadSearchSettings,
    #[serde(rename = "web.settings.write")]
    SaveSearchSettings,
    #[serde(rename = "activity.read")]
    ReadActivity,
    #[serde(rename = "usage.read")]
    ReadUsage,
    #[serde(rename = "databases.read")]
    ReadDatabases,
    #[serde(rename = "databases.control")]
    ControlDatabases,
    #[serde(rename = "databases.manage")]
    ManageDatabases,
    #[serde(rename = "ssh.read")]
    ReadSsh,
    #[serde(rename = "ssh.control")]
    ControlSsh,
    #[serde(rename = "ssh.manage")]
    ManageSsh,
    #[serde(rename = "agents.delegate")]
    DelegateAgents,
    #[serde(rename = "roles.read")]
    ReadRoles,
    #[serde(rename = "teams.read")]
    ReadTeams,
    #[serde(rename = "roles.write")]
    WriteRoles,
    #[serde(rename = "models.read")]
    ReadModels,
    #[serde(rename = "decisions.run")]
    RunDecisions,
    #[serde(rename = "models.provide")]
    ProvideModels,
    #[serde(rename = "models.authenticate")]
    AuthenticateModels,
    #[serde(rename = "credentials.inspect")]
    InspectCredentials,
    #[serde(rename = "models.install")]
    InstallModels,
    #[serde(rename = "media.inspect")]
    InspectMedia,
    #[serde(rename = "media.image")]
    GenerateImage,
    #[serde(rename = "media.video")]
    GenerateVideo,
    #[serde(rename = "media.settings.read")]
    ReadMediaSettings,
    #[serde(rename = "media.settings.write")]
    SaveMediaSettings,
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
    #[serde(rename = "notifications.publish")]
    Notify,
    #[serde(rename = "dispatch.manage")]
    Dispatch,
    #[serde(rename = "files.read")]
    ReadFiles,
    #[serde(rename = "files.write")]
    WriteFiles,
    #[serde(rename = "git.read")]
    ReadGit,
    #[serde(rename = "git.write")]
    WriteGit,
    #[serde(rename = "worktrees.read")]
    ReadWorktrees,
    #[serde(rename = "worktrees.write")]
    WriteWorktrees,
    #[serde(rename = "process.read")]
    ReadCommands,
    #[serde(rename = "process.control")]
    ControlCommands,
    #[serde(rename = "terminals.read")]
    ReadTerminals,
    #[serde(rename = "terminals.control")]
    ControlTerminals,
    #[serde(rename = "conversation.read")]
    ReadConversation,
    #[serde(rename = "conversation.control")]
    ControlConversation,
    #[serde(rename = "conversation.message.read")]
    ReadMessage,
    /// Host-wide creation; does not grant access to existing conversations.
    #[serde(rename = "sessions.start")]
    StartSessions,
    #[serde(rename = "projects.read")]
    ReadProjects,
    #[serde(rename = "http.request")]
    Http,
    #[serde(rename = "web.read")]
    ReadWeb,
    #[serde(rename = "models.generate")]
    GenerateText,
    #[serde(rename = "storage.read")]
    ReadStorage,
    #[serde(rename = "storage.write")]
    WriteStorage,
    #[serde(rename = "knowledge.read")]
    ReadKnowledge,
    #[serde(rename = "knowledge.write")]
    WriteKnowledge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Modify,
    Operate,
}

impl Action {
    pub fn current(&self) -> bool {
        !matches!(
            self,
            Self::ReadCode | Self::ManageCode | Self::ReadCodeSettings | Self::SaveCodeSettings
        )
    }

    /// Presentation classification only; execution and permission checks retain their owners.
    pub fn access(&self) -> Access {
        match self {
            Self::ReadArtifacts
            | Self::ReadCodeIndex
            | Self::ReadCodeIndexSettings
            | Self::ReadCode
            | Self::ReadCodeSettings
            | Self::ReadSearchSettings
            | Self::ReadActivity
            | Self::ReadUsage
            | Self::ReadDatabases
            | Self::ReadSsh
            | Self::ReadRoles
            | Self::ReadTeams
            | Self::ReadModels
            | Self::RunDecisions
            | Self::InspectMedia
            | Self::ReadMediaSettings
            | Self::ReadComputer
            | Self::ReadBrowser
            | Self::ReadExternalBrowser
            | Self::ReadFiles
            | Self::ReadGit
            | Self::ReadWorktrees
            | Self::ReadCommands
            | Self::ReadTerminals
            | Self::ReadConversation
            | Self::ReadMessage
            | Self::ReadProjects
            | Self::ReadWeb
            | Self::ReadStorage
            | Self::ReadKnowledge => Access::Read,
            Self::WriteArtifacts
            | Self::SaveCodeIndexSettings
            | Self::ManageCode
            | Self::SaveCodeSettings
            | Self::SaveSearchSettings
            | Self::ManageDatabases
            | Self::ManageSsh
            | Self::WriteRoles
            | Self::InstallModels
            | Self::SaveMediaSettings
            | Self::WriteFiles
            | Self::WriteGit
            | Self::WriteWorktrees
            | Self::WriteStorage
            | Self::WriteKnowledge => Access::Modify,
            Self::ControlDatabases
            | Self::ControlSsh
            | Self::DelegateAgents
            | Self::ProvideModels
            | Self::AuthenticateModels
            | Self::InspectCredentials
            | Self::GenerateImage
            | Self::GenerateVideo
            | Self::ControlComputer
            | Self::ControlBrowser
            | Self::ControlExternalBrowser
            | Self::Notify
            | Self::Dispatch
            | Self::ControlCommands
            | Self::ControlTerminals
            | Self::ControlConversation
            | Self::StartSessions
            | Self::Http
            | Self::GenerateText => Access::Operate,
        }
    }

    pub fn read_only(&self) -> bool {
        matches!(
            self,
            Self::ReadCode
                | Self::ReadCodeIndex
                | Self::ReadCodeIndexSettings
                | Self::ReadArtifacts
                | Self::ReadCodeSettings
                | Self::ReadSearchSettings
                | Self::ReadActivity
                | Self::ReadUsage
                | Self::ReadDatabases
                | Self::ReadSsh
                | Self::ReadRoles
                | Self::ReadTeams
                | Self::ReadModels
                | Self::RunDecisions
                | Self::InspectMedia
                | Self::ReadMediaSettings
                | Self::ReadComputer
                | Self::ReadBrowser
                | Self::ReadExternalBrowser
                | Self::ReadFiles
                | Self::ReadGit
                | Self::ReadWorktrees
                | Self::ReadCommands
                | Self::ReadTerminals
                | Self::ReadConversation
                | Self::ReadMessage
                | Self::ReadProjects
                | Self::ReadWeb
                | Self::ReadStorage
                | Self::ReadKnowledge
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_wire_and_validation() {
        for (scenario, check) in [
            (
                "preserves_supported_identifiers",
                (|scenario: &'static str| {
                    for (name, action) in [
                        ("files.read", Action::ReadFiles),
                        ("models.read", Action::ReadModels),
                        ("decisions.run", Action::RunDecisions),
                        ("models.install", Action::InstallModels),
                        ("models.generate", Action::GenerateText),
                        ("knowledge.read", Action::ReadKnowledge),
                        ("knowledge.write", Action::WriteKnowledge),
                        ("conversation.message.read", Action::ReadMessage),
                        ("code.index.read", Action::ReadCodeIndex),
                        ("code.index.settings.read", Action::ReadCodeIndexSettings),
                        ("code.index.settings.write", Action::SaveCodeIndexSettings),
                    ] {
                        assert_eq!(
                            serde_json::to_value(&action).unwrap(),
                            json!(name),
                            "{scenario}"
                        );
                        assert_eq!(
                            serde_json::from_value::<Action>(json!(name)).unwrap(),
                            action,
                            "{scenario}"
                        );
                    }
                }) as fn(&'static str),
            ),
            (
                "rejects_unknown_or_malformed_identifiers",
                (|scenario: &'static str| {
                    for (case, value) in [
                        ("unknown", json!("unknown.action")),
                        ("retired decision", json!("models.decide")),
                        ("null", json!(null)),
                        ("number", json!(1)),
                        ("boolean", json!(true)),
                        ("array", json!([])),
                        ("object", json!({"files.read":true})),
                    ] {
                        assert!(
                            serde_json::from_value::<Action>(value).is_err(),
                            "{scenario}: {case}"
                        );
                    }
                }),
            ),
            (
                "excludes_unknown_identifiers_from_authoring",
                (|scenario: &'static str| {
                    let schema = serde_json::to_value(schemars::schema_for!(Action)).unwrap();
                    let variants = schema["oneOf"].as_array().unwrap();
                    assert_eq!(variants.len(), 2, "{scenario}");
                    assert_eq!(variants[0]["type"], "string", "{scenario}");
                    let names = variants[0]["enum"].as_array().unwrap();
                    assert!(names.contains(&json!("files.read")), "{scenario}");
                    assert!(names.contains(&json!("models.read")), "{scenario}");
                    assert!(names.contains(&json!("models.install")), "{scenario}");
                    assert!(names.contains(&json!("models.generate")), "{scenario}");
                    assert!(names.contains(&json!("knowledge.read")), "{scenario}");
                    assert!(names.contains(&json!("knowledge.write")), "{scenario}");
                    assert!(names.contains(&json!("code.index.read")), "{scenario}");
                    for name in [
                        "code.read",
                        "code.manage",
                        "code.settings.read",
                        "code.settings.write",
                    ] {
                        assert!(!names.contains(&json!(name)), "{scenario}");
                        let action: Action = serde_json::from_value(json!(name)).unwrap();
                        assert!(!action.current(), "{scenario}");
                    }
                    assert!(!names.contains(&json!("unknown.action")), "{scenario}");
                    assert!(!names.contains(&json!("Unsupported")), "{scenario}");
                    assert!(!names.is_empty(), "{scenario}");
                    for name in names {
                        assert!(
                            serde_json::from_value::<Action>(name.clone()).is_ok(),
                            "{scenario}"
                        );
                    }
                    assert_eq!(variants[1]["type"], "string", "{scenario}");
                    assert_eq!(variants[1]["const"], "sessions.start", "{scenario}");
                    assert_eq!(
                        variants[1]["description"],
                        "Host-wide creation; does not grant access to existing conversations.",
                        "{scenario}"
                    );
                    assert_eq!(
                        serde_json::from_value::<Action>(variants[1]["const"].clone()).unwrap(),
                        Action::StartSessions,
                        "{scenario}"
                    );
                }),
            ),
            (
                "keeps_access_distinct_from_read_only",
                (|scenario: &'static str| {
                    let schema = serde_json::to_value(schemars::schema_for!(Action)).unwrap();
                    for name in schema["oneOf"][0]["enum"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .chain(std::iter::once(&schema["oneOf"][1]["const"]))
                    {
                        let action: Action = serde_json::from_value(name.clone()).unwrap();
                        assert_eq!(
                            action.access() == Access::Read,
                            action.read_only(),
                            "{scenario}: {name}"
                        );
                    }
                    assert_eq!(Action::ReadFiles.access(), Access::Read, "{scenario}");
                    assert!(Action::ReadFiles.read_only(), "{scenario}");
                    assert_eq!(Action::WriteFiles.access(), Access::Modify, "{scenario}");
                    assert!(!Action::WriteFiles.read_only(), "{scenario}");
                    assert_eq!(
                        Action::ControlConversation.access(),
                        Access::Operate,
                        "{scenario}"
                    );
                    assert!(!Action::ControlConversation.read_only(), "{scenario}");
                    assert_eq!(
                        Action::InspectCredentials.access(),
                        Access::Operate,
                        "{scenario}"
                    );
                    assert!(!Action::InspectCredentials.read_only(), "{scenario}");
                }),
            ),
            (
                "installation_is_not_read_only",
                (|scenario: &'static str| {
                    assert!(Action::ReadModels.read_only(), "{scenario}");
                    assert!(!Action::InstallModels.read_only(), "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
