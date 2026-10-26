//! Classify existing database handlers; execution owners keep their own entry points.
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Request};

/// Service dispatch stays with the Node; durability remains a protocol property.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Route {
    Read,
    Store,
    ReadNetwork,
    ReadInstallations,
    DismissInstallation,
    ReadBackup,
    ReadArtifact,
    FetchWeb,
    SearchWeb,
    ListChannelPresets,
    ModelCatalog,
    ReadStorageSearch,
    ReadKnowledge,
    Decide,
    ReadLocalModels,
    ReadCodeIndex,
    ReadCommand,
    Port,
    ReadPluginMcp,
    ReadPluginView,
    CheckPluginUpdate,
    PluginCatalog,
    DiscoverSkills,
    RequestProviderHttp,
    DiscoverModels,
    RefreshCatalog,
    Terminal,
    Transfer,
    InspectWorktree,
    Host,
    Browser,
}

pub(super) fn validate(node: NodeId, request: &Request) -> Result<(), Fault> {
    if request.target != node || request.version != sailry_protocol::VERSION {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "Node or protocol version mismatch",
        ));
    }
    Ok(())
}

pub(super) fn route(command: &Command) -> Route {
    if read(command) {
        return Route::Read;
    }
    match command {
        Command::CompleteBrowser { .. } => Route::Browser,
        Command::ReadNetworkSettings => Route::ReadNetwork,
        Command::ReadInstallations { .. } => Route::ReadInstallations,
        Command::DismissInstallation { .. } => Route::DismissInstallation,
        Command::ListBackups => Route::ReadBackup,
        Command::ReadArtifact { .. } => Route::ReadArtifact,
        Command::FetchWeb(_) | Command::CancelWebFetch { .. } => Route::FetchWeb,
        Command::SearchWeb(_)
        | Command::CancelWebSearch { .. }
        | Command::TestSearchDriver { .. } => Route::SearchWeb,
        Command::ListChannelPresets { .. } => Route::ListChannelPresets,
        Command::SearchModels(_)
        | Command::ReadModelVersions { .. }
        | Command::ResolveModel { .. }
        | Command::RefreshLocalModel { .. } => Route::ModelCatalog,
        Command::SearchPluginValues(_) => Route::ReadStorageSearch,
        Command::QueryKnowledge(_) | Command::ReadKnowledgeCitation(_) => Route::ReadKnowledge,
        Command::Decide(_) => Route::Decide,
        Command::ReadLocalModels { .. } | Command::CancelLocalModel { .. } => {
            Route::ReadLocalModels
        }
        Command::SearchCode { .. }
        | Command::OutlineCode { .. }
        | Command::CancelCodeIndexQuery { .. } => Route::ReadCodeIndex,
        Command::ListCommands { .. } | Command::ReadCommand { .. } => Route::ReadCommand,
        Command::OpenPort { .. } | Command::CancelPort { .. } => Route::Port,
        Command::ReadPluginMcp { .. } => Route::ReadPluginMcp,
        Command::ReadPluginView { .. } => Route::ReadPluginView,
        Command::CheckPluginUpdate { .. } | Command::CheckPluginUpdates { .. } => {
            Route::CheckPluginUpdate
        }
        Command::SearchPluginCatalog { .. }
        | Command::SearchSkillCatalog { .. }
        | Command::ReadCatalogPlugin { .. }
        | Command::ReadCatalogPluginInfo { .. }
        | Command::InspectPluginSource { .. } => Route::PluginCatalog,
        Command::ReadSkillDocument { .. }
        | Command::DiscoverSkills { .. }
        | Command::DiscoverRegistrySkill { .. } => Route::DiscoverSkills,
        Command::RequestProviderHttp { .. } => Route::RequestProviderHttp,
        Command::DiscoverModels(_) | Command::ValidateProvider { .. } => Route::DiscoverModels,
        Command::RefreshModelCatalog => Route::RefreshCatalog,
        Command::InspectTerminal { .. }
        | Command::ClaimTerminal { .. }
        | Command::InputTerminal { .. }
        | Command::ResizeTerminal { .. }
        | Command::SetTerminalAppearance { .. } => Route::Terminal,
        Command::DownloadFile { .. }
        | Command::DownloadBackup { .. }
        | Command::UploadBackup { .. }
        | Command::UploadPlugin(_)
        | Command::DiscoverSkillUpload { .. }
        | Command::DiscoverLocalSkills { .. }
        | Command::InspectPluginUpload { .. }
        | Command::UploadFile(_)
        | Command::StageSshUpload(_)
        | Command::UploadAttachment(_)
        | Command::DownloadAttachment { .. }
        | Command::DownloadImage { .. }
        | Command::CancelFileTransfer { .. } => Route::Transfer,
        Command::ListDirectory { .. }
        | Command::BrowseFiles { .. }
        | Command::ListTerminalTools { .. }
        | Command::ReadFile { .. }
        | Command::ReadFileLines { .. }
        | Command::FindFiles { .. }
        | Command::ReadOffice { .. }
        | Command::ReadNotebook { .. }
        | Command::SearchFiles { .. }
        | Command::InspectGit { .. }
        | Command::ReadGitStash { .. }
        | Command::ReadGitOutput { .. }
        | Command::ListGitRemoteTags { .. }
        | Command::ListGitBranches { .. }
        | Command::ResolveGitRevision { .. }
        | Command::ListWorktrees { .. }
        | Command::ReadGitLog { .. }
        | Command::ReadGitShow { .. }
        | Command::ReadGitBlame { .. }
        | Command::ListGitChanges { .. }
        | Command::ReadGitChange { .. }
        | Command::ReadGitContext { .. }
        | Command::ReadGitCommit { .. }
        | Command::ReadGitDiff { .. } => Route::InspectWorktree,
        Command::InspectHost
        | Command::ReadHostMetrics
        | Command::ReadComputerPermissions
        | Command::RequestComputerPermission { .. }
        | Command::ListShells => Route::Host,
        _ => Route::Store,
    }
}

fn read(command: &Command) -> bool {
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
            | Command::ReadCodeIndexSettings
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
            | Command::SetPluginEnabled { enabled: false, .. }
            | Command::RemovePlugin { .. }
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

/// External effects are admitted once; only their captured outcomes may be retried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Effect {
    Network,
    Login,
    Ssh,
    Database,
    Plugin,
    Resources,
    Process,
    Session,
    Mutation,
    Inline,
}

pub(super) fn external(command: &Command) -> bool {
    effect(command).is_some()
}

pub(super) fn effect(command: &Command) -> Option<Effect> {
    if materializes(command) {
        return Some(Effect::Inline);
    }
    let effect = match command {
        Command::ReadNetworkSettings | Command::SaveNetworkSettings { .. } => Effect::Network,
        Command::BeginProviderLogin { .. }
        | Command::CompleteProviderLogin { .. }
        | Command::CancelProviderLogin { .. }
        | Command::StartConnectorLogin { .. }
        | Command::SubmitConnectorInput { .. }
        | Command::CancelConnectorLogin { .. }
        | Command::ProbeConnection { .. }
        | Command::InspectConnectorCli { .. } => Effect::Login,
        Command::InstallHost { .. }
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
        | Command::CancelSsh { .. } => Effect::Ssh,
        Command::BrowseDatabase { .. }
        | Command::CheckDatabase { .. }
        | Command::TestDatabase { .. }
        | Command::QueryDatabase { .. }
        | Command::CancelDatabase { .. } => Effect::Database,
        Command::GeneratePluginText { .. }
        | Command::RequestPluginHttp(_)
        | Command::ExecuteProviderHttp { .. }
        | Command::CallPlugin { .. }
        | Command::ReadPluginCallback { .. }
        | Command::CancelPluginCall { .. } => Effect::Plugin,
        Command::CloseTerminal { .. }
        | Command::CheckPluginMcp { .. }
        | Command::RestoreBackupFile { .. }
        | Command::ExportBackup { .. }
        | Command::RestoreBackup { .. }
        | Command::InstallLocalModel { .. }
        | Command::DeleteLocalModel { .. }
        | Command::UseExternalBrowser { .. }
        | Command::UseBrowser { .. }
        | Command::UseMedia { .. } => Effect::Resources,
        Command::RunCommand { .. } => Effect::Process,
        Command::CreateSessionAt { .. } => Effect::Session,
        Command::WriteArtifact(_)
        | Command::ImportSession(_)
        | Command::BindConnectionSession { .. } => Effect::Inline,
        _ if super::mutations::target(command).is_some() => Effect::Mutation,
        _ => return None,
    };
    Some(effect)
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

pub(super) fn launches(command: &Command) -> bool {
    matches!(
        command,
        Command::SubmitTurn { .. }
            | Command::SubmitTask { .. }
            | Command::ResolveQuestion {
                response: sailry_protocol::conversation::question::Response::StartCoding { .. },
                ..
            }
            | Command::ContinueTurn { .. }
            | Command::PluginTransaction { .. }
            | Command::StartSession(_)
            | Command::ReplaceTurn { .. }
            | Command::CompactContext { .. }
            | Command::StartQueuedTurn { .. }
            | Command::SendQueuedTurn { .. }
            | Command::GuideQueuedTurn { .. }
            | Command::GuideTurn { .. }
            | Command::SetQueuePaused { .. }
            | Command::SavePluginSettings { .. }
    )
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
            assert_eq!(route(&command), Route::Read, "{command:?}");
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
            assert_ne!(route(&command), Route::Read, "{command:?}");
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
            assert_ne!(route(&command), Route::Read, "{command:?}");
        }
    }
}
