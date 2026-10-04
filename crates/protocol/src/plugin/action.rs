//! Supported capabilities and inert identifiers retained in stored declarations.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Action {
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
    #[serde(rename = "models.generate")]
    GenerateText,
    #[serde(rename = "storage.read")]
    ReadStorage,
    #[serde(rename = "storage.write")]
    WriteStorage,
    /// Retained for round-tripping; never matches a supported capability.
    #[serde(untagged)]
    #[schemars(skip)]
    Unsupported(String),
}

impl Action {
    pub fn supported(&self) -> bool {
        !matches!(self, Self::Unsupported(_))
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
            ("models.generate", Action::GenerateText),
        ] {
            assert!(action.supported());
            assert_eq!(serde_json::to_value(&action).unwrap(), json!(name));
            assert_eq!(
                serde_json::from_value::<Action>(json!(name)).unwrap(),
                action
            );
        }
    }

    #[test]
    fn retains_unknown_identifiers_without_grants() {
        for name in ["unknown.action", "models.decide"] {
            let action: Action = serde_json::from_value(json!(name)).unwrap();
            assert_eq!(action, Action::Unsupported(name.into()));
            assert!(!action.supported());
            assert_ne!(action, Action::ReadModels);
            assert_ne!(action, Action::GenerateText);
            assert_eq!(serde_json::to_value(&action).unwrap(), json!(name));
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
        let variants = schema["anyOf"].as_array().unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0]["type"], "string");
        let names = variants[0]["enum"].as_array().unwrap();
        assert!(names.contains(&json!("files.read")));
        assert!(names.contains(&json!("models.read")));
        assert!(names.contains(&json!("models.generate")));
        assert!(!names.contains(&json!("unknown.action")));
        assert!(!names.contains(&json!("Unsupported")));
        assert!(!names.is_empty());
        for name in names {
            let action: Action = serde_json::from_value(name.clone()).unwrap();
            assert!(
                action.supported(),
                "unsupported authoring identifier {name}"
            );
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
}
