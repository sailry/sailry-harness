//! Classify existing database handlers; execution owners keep their own entry points.
use sailry_protocol::Command;

pub(super) fn read(command: &Command) -> bool {
    matches!(
        command,
        Command::Snapshot
            | Command::InspectRequest { .. }
            | Command::ReadProjectCatalog
            | Command::ReadWorktreeCatalog { .. }
            | Command::ReadSession { .. }
            | Command::ReadConversation { .. }
            | Command::ReadTurn { .. }
            | Command::ReadTurnCapture { .. }
            | Command::ReadQueuedTurn { .. }
            | Command::ListConversationAssets { .. }
            | Command::SearchConversation { .. }
            | Command::ListFileCheckpoints { .. }
            | Command::ReadFileCheckpoint { .. }
            | Command::ReadTurnDiff { .. }
            | Command::ReadTurnHunks { .. }
            | Command::ReadUsage(_)
            | Command::ReadActivityCatalog
            | Command::ReadActivity { .. }
            | Command::ListArtifacts { .. }
            | Command::ListArtifactRevisions { .. }
            | Command::ReadAttachment { .. }
            | Command::ListKnowledgeCollections
            | Command::ReadHooks { .. }
            | Command::ReadCodeSettings
            | Command::ReadPowerSettings
            | Command::ReadTerminalSettings
            | Command::ReadSandboxSettings
            | Command::ReadSearchSettings
            | Command::ReadBackupSettings
            | Command::ReadPythonSettings
            | Command::ReadMediaSettings
            | Command::ReadLocalModelSettings
            | Command::ListMediaModels
            | Command::ListDatabases
            | Command::ListSsh
            | Command::ListSshTerminals { .. }
            | Command::ListTerminals { .. }
            | Command::ListConnectorTypes
            | Command::ListConnections
            | Command::ReadPluginValue { .. }
            | Command::ReadPluginConversationValue { .. }
            | Command::ListPluginKeys { .. }
            | Command::ReadPluginSettings { .. }
            | Command::ReadPluginSecret { .. }
            | Command::ReadPluginConnection { .. }
            | Command::ListPlugins
            | Command::ReadPlugin { .. }
            | Command::ReadPluginVersion { .. }
            | Command::ListPluginModels
            | Command::ResolvePluginModel { .. }
            | Command::ListRoles
            | Command::ListProviders
            | Command::ListCredentials
            | Command::ReadProviderKey { .. }
            | Command::ReadCatalogStatus
            | Command::ReadModelCatalog(_)
    )
}

/// Lifecycle transitions also settle in-memory waiters or execution ownership.
pub(super) fn lifecycle(command: &Command) -> bool {
    if materializes(command) {
        return true;
    }
    if let Command::PluginTransaction { operations } = command {
        return operations.iter().any(|operation| {
            matches!(
                operation,
                sailry_protocol::plugin::transaction::Operation::QueuePause { .. }
                    | sailry_protocol::plugin::transaction::Operation::Submit { .. }
                    | sailry_protocol::plugin::transaction::Operation::Continue { .. }
                    | sailry_protocol::plugin::transaction::Operation::Stop { .. }
            )
        });
    }
    matches!(
        command,
        Command::SaveNetworkSettings { .. }
            | Command::StartSession(_)
            | Command::SubmitTurn { .. }
            | Command::SubmitTask { .. }
            | Command::ContinueTurn { .. }
            | Command::ReplaceTurn { .. }
            | Command::CompactContext { .. }
            | Command::StopTurn { .. }
            | Command::StopDispatchTurn { .. }
            | Command::WriteArtifact(_)
            | Command::ImportSession(_)
            | Command::BindConnectionSession { .. }
            | Command::GuideTurn { .. }
            | Command::ResolveApproval { .. }
            | Command::ResolveQuestion { .. }
            | Command::EditQueuedTurn { .. }
            | Command::RemoveQueuedTurn { .. }
            | Command::StartQueuedTurn { .. }
            | Command::SendQueuedTurn { .. }
            | Command::GuideQueuedTurn { .. }
            | Command::MoveQueuedTurn { .. }
            | Command::SetQueuePaused { .. }
    )
}

/// Execution owners run effects only after their own durable admission.
pub(super) fn external(command: &Command) -> bool {
    materializes(command)
        || super::mutations::target(command).is_some()
        || matches!(
            command,
            Command::ReadNetworkSettings
                | Command::WriteArtifact(_)
                | Command::ImportSession(_)
                | Command::BindConnectionSession { .. }
                | Command::SaveNetworkSettings { .. }
                | Command::BeginProviderLogin { .. }
                | Command::CompleteProviderLogin { .. }
                | Command::CancelProviderLogin { .. }
                | Command::StartConnectorLogin { .. }
                | Command::SubmitConnectorInput { .. }
                | Command::CancelConnectorLogin { .. }
                | Command::ProbeConnection { .. }
                | Command::InspectConnectorCli { .. }
                | Command::InstallHost { .. }
                | Command::UpdateSshHost { .. }
                | Command::ReadHostInstall { .. }
                | Command::CheckSsh { .. }
                | Command::OpenSshTerminal { .. }
                | Command::CloseSshTerminal { .. }
                | Command::RunSsh { .. }
                | Command::TransferSsh { .. }
                | Command::BrowseSshDirectory { .. }
                | Command::DownloadSshFile { .. }
                | Command::FinishSshUpload { .. }
                | Command::ModifySshFile { .. }
                | Command::CancelSsh { .. }
                | Command::BrowseDatabase { .. }
                | Command::CheckDatabase { .. }
                | Command::TestDatabase { .. }
                | Command::QueryDatabase { .. }
                | Command::CancelDatabase { .. }
                | Command::GeneratePluginText { .. }
                | Command::RequestPluginHttp(_)
                | Command::ExecuteProviderHttp { .. }
                | Command::CallPlugin { .. }
                | Command::ReadPluginCallback { .. }
                | Command::CancelPluginCall { .. }
                | Command::CloseTerminal { .. }
                | Command::CheckPluginMcp { .. }
                | Command::RestoreBackupFile { .. }
                | Command::ExportBackup { .. }
                | Command::RestoreBackup { .. }
                | Command::StartCodeServer { .. }
                | Command::StopCodeServer { .. }
                | Command::InstallLocalModel { .. }
                | Command::DeleteLocalModel { .. }
                | Command::InstallCodeServer { .. }
                | Command::UninstallCodeServer { .. }
                | Command::UpdateCodeRules
                | Command::UseExternalBrowser { .. }
                | Command::UseBrowser { .. }
                | Command::UseMedia { .. }
                | Command::RunCommand { .. }
                | Command::CreateSessionAt { .. }
        )
}

fn materializes(command: &Command) -> bool {
    let config = match command {
        Command::CreateSession {
            project: None,
            config,
            ..
        } => config,
        Command::StartSession(draft) if draft.project.is_none() => &draft.config,
        _ => return false,
    };
    config
        .as_ref()
        .is_none_or(|config| config.resource.is_some() || config.assistant.is_some())
}

pub(super) fn session(command: &Command) -> Option<sailry_protocol::SessionId> {
    match command {
        Command::RenameSession { session, .. }
        | Command::SetSessionRead { session, .. }
        | Command::SetSessionConfig { session, .. } => Some(*session),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{
        ArtifactId, AttachmentId, NotificationId, SessionId, WorktreeId, plugin,
    };

    #[test]
    fn includes_database_reads() {
        let session = SessionId::new();
        for command in [
            Command::Snapshot,
            Command::ReadSession { session },
            Command::ReadActivityCatalog,
            Command::ListKnowledgeCollections,
            Command::ListArtifacts {
                session,
                after: None,
                limit: 1,
            },
            Command::ReadAttachment {
                worktree: WorktreeId::new(),
                attachment: AttachmentId::new(),
            },
            Command::ListConnections,
            Command::ReadPluginSettings {
                package: plugin::Reference {
                    name: "fixture".into(),
                    settings_revision: 1,
                    digest: "a".repeat(64),
                },
            },
            Command::ReadPluginValue { key: "key".into() },
        ] {
            assert!(read(&command), "{command:?}");
        }
    }

    #[test]
    fn excludes_mutations() {
        for command in [
            Command::MarkNotificationsRead {
                ids: vec![NotificationId::new()],
            },
            Command::SetPluginEnabled {
                name: "fixture".into(),
                expected_revision: 1,
                enabled: true,
            },
            Command::WritePluginValue {
                key: "key".into(),
                value: serde_json::Value::Null,
                expected_revision: 0,
            },
        ] {
            assert!(!read(&command), "{command:?}");
        }
    }

    #[test]
    fn preserves_execution_owners() {
        for command in [
            Command::InspectHost,
            Command::ReadHostMetrics,
            Command::RefreshModelCatalog,
            Command::ReadNetworkSettings,
            Command::ReadInstallations {
                requests: Vec::new(),
            },
            Command::ListChannelPresets {
                refresh_partners: false,
            },
            Command::ReadArtifact {
                id: ArtifactId::new(),
                revision: None,
                session: None,
            },
            Command::SearchPluginValues(plugin::storage::Search {
                terms: vec!["literal".into()],
                query: None,
                mode: plugin::storage::Mode::Keyword,
                weights: [1, 1],
                all: Vec::new(),
                any: Vec::new(),
                offset: 0,
                limit: 1,
            }),
        ] {
            assert!(!read(&command), "{command:?}");
        }
    }
}
