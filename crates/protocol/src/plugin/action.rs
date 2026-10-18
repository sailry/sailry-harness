//! Host capabilities declared by plugin extensions.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Action {
    #[serde(rename = "artifacts.read")]
    ReadArtifacts,
    #[serde(rename = "artifacts.write")]
    WriteArtifacts,
    #[serde(rename = "code.read")]
    ReadCode,
    #[serde(rename = "code.manage")]
    ManageCode,
    #[serde(rename = "code.settings.read")]
    ReadCodeSettings,
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
    #[serde(rename = "roles.write")]
    WriteRoles,
    #[serde(rename = "models.read")]
    ReadModels,
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
    /// Presentation classification only; execution and permission checks retain their owners.
    pub fn access(&self) -> Access {
        match self {
            Self::ReadArtifacts
            | Self::ReadCode
            | Self::ReadCodeSettings
            | Self::ReadSearchSettings
            | Self::ReadActivity
            | Self::ReadUsage
            | Self::ReadDatabases
            | Self::ReadSsh
            | Self::ReadRoles
            | Self::ReadModels
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
            | Self::ReadProjects
            | Self::ReadWeb
            | Self::ReadStorage
            | Self::ReadKnowledge => Access::Read,
            Self::WriteArtifacts
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
                | Self::ReadArtifacts
                | Self::ReadCodeSettings
                | Self::ReadSearchSettings
                | Self::ReadActivity
                | Self::ReadUsage
                | Self::ReadDatabases
                | Self::ReadSsh
                | Self::ReadRoles
                | Self::ReadModels
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
    fn preserves_supported_identifiers() {
        for (name, action) in [
            ("files.read", Action::ReadFiles),
            ("models.read", Action::ReadModels),
            ("models.install", Action::InstallModels),
            ("models.generate", Action::GenerateText),
            ("knowledge.read", Action::ReadKnowledge),
            ("knowledge.write", Action::WriteKnowledge),
        ] {
            assert_eq!(serde_json::to_value(&action).unwrap(), json!(name));
            assert_eq!(
                serde_json::from_value::<Action>(json!(name)).unwrap(),
                action
            );
        }
    }

    #[test]
    fn rejects_unknown_identifiers() {
        for name in ["unknown.action", "models.decide"] {
            assert!(serde_json::from_value::<Action>(json!(name)).is_err());
        }
    }

    #[test]
    fn rejects_malformed_identifiers() {
        for value in [
            json!(null),
            json!(1),
            json!(true),
            json!([]),
            json!({"files.read": true}),
        ] {
            assert!(serde_json::from_value::<Action>(value).is_err());
        }
    }

    #[test]
    fn excludes_unknown_identifiers_from_authoring() {
        let schema = serde_json::to_value(schemars::schema_for!(Action)).unwrap();
        let variants = schema["oneOf"].as_array().unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0]["type"], "string");
        let names = variants[0]["enum"].as_array().unwrap();
        assert!(names.contains(&json!("files.read")));
        assert!(names.contains(&json!("models.read")));
        assert!(names.contains(&json!("models.install")));
        assert!(names.contains(&json!("models.generate")));
        assert!(names.contains(&json!("knowledge.read")));
        assert!(names.contains(&json!("knowledge.write")));
        assert!(!names.contains(&json!("unknown.action")));
        assert!(!names.contains(&json!("Unsupported")));
        assert!(!names.is_empty());
        for name in names {
            assert!(serde_json::from_value::<Action>(name.clone()).is_ok());
        }
        assert_eq!(variants[1]["type"], "string");
        assert_eq!(variants[1]["const"], "sessions.start");
        assert_eq!(
            variants[1]["description"],
            "Host-wide creation; does not grant access to existing conversations."
        );
        assert_eq!(
            serde_json::from_value::<Action>(variants[1]["const"].clone()).unwrap(),
            Action::StartSessions
        );
    }

    #[test]
    fn keeps_access_distinct_from_read_only() {
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
                "{name}"
            );
        }
        assert_eq!(Action::ReadFiles.access(), Access::Read);
        assert!(Action::ReadFiles.read_only());
        assert_eq!(Action::WriteFiles.access(), Access::Modify);
        assert!(!Action::WriteFiles.read_only());
        assert_eq!(Action::ControlConversation.access(), Access::Operate);
        assert!(!Action::ControlConversation.read_only());
        assert_eq!(Action::InspectCredentials.access(), Access::Operate);
        assert!(!Action::InspectCredentials.read_only());
    }

    #[test]
    fn installation_is_not_read_only() {
        assert!(Action::ReadModels.read_only());
        assert!(!Action::InstallModels.read_only());
    }
}
