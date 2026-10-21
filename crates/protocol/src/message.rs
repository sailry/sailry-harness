use crate::NotificationId;
use crate::WorktreeId;
use serde::{Deserialize, Serialize};

use crate::{
    Credential, CredentialId, Defaults, NodeId, ProjectId, ProviderId, QueuedTurn, RequestId,
    Secret, Session, SessionConfig, SessionId, SessionRevision, StreamId, TurnId,
};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    WrongTarget,
    NotFound,
    NotConfigured,
    Conflict,
    RevisionConflict,
    PermissionDenied,
    Expired,
    Busy,
    Unavailable,
    Cancelled,
    OutcomeUnknown,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<FaultDetails>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FaultDetails {
    /// The plugin value owner rejected a key's expected revision before commit.
    PluginValueConflict,
    VersionMismatch {
        controller: Option<String>,
        host: Option<String>,
        update_host: bool,
        unverified: bool,
    },
    ProjectRemoval {
        tasks_active: bool,
        terminals_may_be_closed: bool,
    },
}

impl Fault {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }

    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }
}

impl std::fmt::Display for Fault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for Fault {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub path: String,
    pub appearance: crate::projects::Appearance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub id: WorktreeId,
    /// None for a connection-owned workspace.
    pub project: Option<ProjectId>,
    pub path: String,
    pub main: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Command {
    WriteArtifact(crate::artifact::Write),
    ReadArtifact {
        id: crate::ArtifactId,
        revision: Option<u32>,
        /// A live conversation retaining the publication; absent for owner reads.
        session: Option<SessionId>,
    },
    ListArtifacts {
        session: SessionId,
        after: Option<crate::ArtifactId>,
        limit: u16,
    },
    ListArtifactRevisions {
        id: crate::ArtifactId,
        before: Option<u32>,
        limit: u16,
        session: Option<SessionId>,
    },
    PluginTransaction {
        operations: Vec<crate::plugin::transaction::Operation>,
    },
    /// Invoke a declared host export in the captured plugin package.
    CallPlugin {
        handler: String,
        input: serde_json::Value,
    },
    /// Invoke an explicitly read-only host export without a durable receipt.
    ReadPluginCallback {
        handler: String,
        input: serde_json::Value,
    },
    CancelPluginCall {
        request: RequestId,
    },
    PublishNotification {
        package: crate::plugin::Reference,
        content: crate::notification::Draft,
    },
    MarkNotificationRead {
        id: NotificationId,
    },
    DismissNotifications {
        ids: Vec<NotificationId>,
    },
    Dispatch {
        package: crate::plugin::Reference,
        action: crate::dispatch::Command,
    },
    /// Stops only the turn admitted by this package's dispatch callback.
    StopDispatchTurn {
        package: crate::plugin::Reference,
        job: crate::JobId,
    },
    UseBrowser {
        session: SessionId,
        worktree: WorktreeId,
        action: crate::browser::Action,
    },
    UseExternalBrowser {
        session: SessionId,
        worktree: WorktreeId,
        action: crate::external_browser::Action,
        arguments: serde_json::Value,
    },
    CompleteBrowser {
        id: RequestId,
        result: crate::browser::Result,
    },
    ReadPluginValue {
        key: String,
    },
    ReadPluginConversationValue {
        key: String,
    },
    ListPluginKeys {
        prefix: String,
        after: Option<String>,
        limit: u16,
    },
    WritePluginValue {
        key: String,
        value: serde_json::Value,
        expected_revision: u64,
    },
    WriteIndexedPluginValue {
        key: String,
        value: serde_json::Value,
        index: crate::plugin::storage::Index,
        expected_revision: u64,
    },
    SearchPluginValues(crate::plugin::storage::Search),
    RemovePluginValue {
        key: String,
        expected_revision: u64,
    },
    WritePluginConversationValue {
        key: String,
        value: serde_json::Value,
        expected_revision: u64,
    },
    RemovePluginConversationValue {
        key: String,
        expected_revision: u64,
    },
    ListPluginModels,
    /// Resolve a provider choice on its execution Node without starting a session.
    ResolvePluginModel {
        model: String,
        effort: Option<crate::Effort>,
        config: Option<SessionConfig>,
    },
    RequestPluginHttp(crate::plugin::http::Request),
    /// Read a named package-declared resource through its configured provider grant.
    RequestProviderHttp {
        provider: ProviderId,
        expected_revision: u64,
        request: String,
        #[serde(default)]
        input: std::collections::BTreeMap<String, String>,
    },
    /// Execute a named package-declared operation with ordinary durable receipts.
    ExecuteProviderHttp {
        provider: ProviderId,
        expected_revision: u64,
        request: String,
        #[serde(default)]
        input: std::collections::BTreeMap<String, String>,
    },
    FetchWeb(crate::web::Request),
    CancelWebFetch {
        request: RequestId,
    },
    SearchWeb(crate::search::Request),
    CancelWebSearch {
        request: RequestId,
    },
    TestSearchDriver {
        driver: crate::search::Driver,
    },
    /// A plugin's explicit, durable, tool-free request to a configured Node model.
    GeneratePluginText {
        prompt: String,
        model: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<crate::Effort>,
    },
    BrowseFiles {
        directory: Option<String>,
        after: Option<crate::DirectoryCursor>,
    },
    ManageFiles(crate::file_browser::Action),
    /// Reserves a single-use stream to a loopback TCP port on the execution Node.
    OpenPort {
        port: u16,
    },
    CancelPort {
        stream: crate::StreamId,
    },
    ListDatabases,
    SaveDatabase {
        profile: crate::database::Profile,
        expected_revision: u64,
        password: Option<crate::Secret>,
    },
    RemoveDatabase {
        profile: crate::DatabaseId,
        expected_revision: u64,
    },
    TestDatabase {
        profile: crate::database::Profile,
        expected_revision: u64,
        password: Option<Secret>,
    },
    CheckDatabase {
        profile: crate::DatabaseId,
        expected_revision: u64,
    },
    BrowseDatabase {
        profile: crate::DatabaseId,
        expected_revision: u64,
        database: Option<String>,
    },
    QueryDatabase {
        /// Force a read-only execution connection independently of the saved profile.
        read_only: bool,
        database: Option<String>,
        profile: crate::DatabaseId,
        expected_revision: u64,
        sql: String,
        row_limit: u32,
        timeout_ms: u64,
    },
    CancelDatabase {
        request: RequestId,
    },
    ModifySshFile {
        profile: crate::SshId,
        expected_revision: u64,
        path: String,
        action: crate::ssh::FileAction,
    },
    BrowseSshDirectory {
        profile: crate::SshId,
        expected_revision: u64,
        path: String,
        after: Option<crate::ssh::Cursor>,
    },
    DownloadSshFile {
        profile: crate::SshId,
        expected_revision: u64,
        path: String,
    },
    StageSshUpload(crate::ssh::UploadSpec),
    FinishSshUpload {
        profile: crate::SshId,
        expected_revision: u64,
        path: String,
        stream: crate::StreamId,
    },
    ListSsh,
    ListSshTerminals {
        profile: crate::SshId,
    },
    SaveSsh {
        profile: crate::ssh::Profile,
        expected_revision: u64,
        credential: Option<crate::ssh::Credential>,
    },
    RemoveSsh {
        profile: crate::SshId,
        expected_revision: u64,
    },
    TrustSsh {
        profile: crate::SshId,
        expected_revision: u64,
        key: crate::ssh::HostKey,
    },
    InstallHost {
        profile: crate::SshId,
        expected_revision: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        network: Option<crate::network::Selection>,
    },
    UpdateSshHost {
        profile: crate::SshId,
        expected_revision: u64,
        node: NodeId,
    },
    ReadHostInstall {
        request: RequestId,
    },
    CheckSsh {
        profile: crate::SshId,
        expected_revision: u64,
    },
    OpenSshTerminal {
        profile: crate::SshId,
        expected_revision: u64,
        launch: crate::ssh::TerminalLaunch,
    },
    CloseSshTerminal {
        terminal: crate::TerminalId,
    },
    RunSsh {
        profile: crate::SshId,
        expected_revision: u64,
        command: String,
        timeout_ms: u64,
    },
    CancelSsh {
        request: RequestId,
    },
    TransferSsh {
        profile: crate::SshId,
        expected_revision: u64,
        transfer: crate::ssh::Transfer,
        timeout_ms: u64,
    },
    ListPlugins,
    SearchSkillCatalog {
        source: crate::plugin::skillcatalog::Source,
        query: String,
        page: u32,
        force: bool,
    },
    SearchPluginCatalog {
        source: crate::plugin::catalog::Source,
        query: String,
        page: u32,
        /// Refresh the official index instead of reusing its TTL cache.
        force: bool,
    },
    ReadCatalogPlugin {
        source: crate::plugin::catalog::Source,
        id: String,
    },
    ReadCatalogPluginInfo {
        source: crate::plugin::catalog::Source,
        id: String,
        bundled: bool,
    },
    InspectPluginSource {
        source: crate::plugin::skills::Source,
    },
    CheckPluginUpdate {
        name: String,
        expected_revision: u64,
        /// Refresh the official index instead of reusing its TTL cache.
        force: bool,
    },
    CheckPluginUpdates {
        packages: Vec<crate::plugin::updates::Request>,
        /// Refresh the official index instead of reusing its TTL cache.
        force: bool,
    },
    /// Dismiss only a terminal transient row, without changing its durable request.
    DismissInstallation {
        request: RequestId,
    },
    ReadInstallations {
        requests: Vec<RequestId>,
    },
    InstallPluginSource {
        source: crate::plugin::skills::Resolved,
        path: String,
        name: String,
        expected_revision: u64,
    },
    InstallBundledPlugin {
        name: String,
        expected_revision: u64,
    },
    ReadSkillDocument {
        source: crate::plugin::skills::DocumentSource,
    },
    DiscoverSkills {
        source: crate::plugin::skills::Source,
    },
    DiscoverSkillUpload {
        stream: crate::StreamId,
        format: crate::plugin::skills::ArchiveFormat,
        source: crate::plugin::UploadSource,
    },
    DiscoverLocalSkills {
        worktree: crate::WorktreeId,
        path: String,
    },
    InstallSkillUpload {
        stream: crate::StreamId,
        source: crate::plugin::skills::Imported,
        name: String,
        expected_revision: u64,
    },
    DiscoverRegistrySkill {
        source: crate::plugin::skills::RegistrySource,
    },
    InstallRegistrySkill {
        source: crate::plugin::skills::RegistryResolved,
        name: String,
        expected_revision: u64,
    },
    InstallSkill {
        source: crate::plugin::skills::Resolved,
        path: String,
        name: String,
        expected_revision: u64,
    },
    InstallMcp {
        name: String,
        expected_revision: u64,
        definition: crate::plugin::mcp::Definition,
        secrets: std::collections::BTreeMap<String, crate::plugin::settings::SecretUpdate>,
    },
    UploadPlugin(crate::plugin::UploadSpec),
    InspectPluginUpload {
        stream: crate::StreamId,
    },
    InstallPluginUpload {
        stream: crate::StreamId,
        source: crate::plugin::UploadSource,
        name: String,
        expected_revision: u64,
    },
    ReadPlugin {
        name: String,
    },
    /// Reads metadata for an immutable package revision without loading or executing skills.
    ReadPluginVersion {
        package: crate::plugin::Reference,
    },
    ReadPluginSettings {
        package: crate::plugin::Reference,
    },
    ReadPluginMcp {
        package: crate::plugin::Reference,
    },
    ListConnectorTypes,
    InspectConnectorCli {
        package: crate::plugin::Reference,
        connector: String,
    },
    ListConnections,
    StartConnectorLogin {
        package: crate::plugin::Reference,
        connector: String,
        connection: Option<crate::plugin::connector::Id>,
        expected_revision: Option<u64>,
        method: crate::plugin::connector::Kind,
        input: crate::plugin::connector::Input,
    },
    SubmitConnectorInput {
        attempt: RequestId,
        input: Secret,
    },
    CancelConnectorLogin {
        attempt: RequestId,
    },
    UpdateConnection {
        connection: crate::plugin::connector::Id,
        expected_revision: u64,
        input: crate::plugin::connector::Input,
    },
    SetConnectionEnabled {
        connection: crate::plugin::connector::Id,
        expected_revision: u64,
        enabled: bool,
    },
    DisconnectConnection {
        connection: crate::plugin::connector::Id,
        expected_revision: u64,
    },
    RemoveConnection {
        connection: crate::plugin::connector::Id,
        expected_revision: u64,
    },
    ConfirmConnectorConsent {
        connection: crate::plugin::connector::Id,
        expected_revision: u64,
        accept: bool,
    },
    ProbeConnection {
        connection: crate::plugin::connector::Id,
    },
    SavePluginMcp {
        package: crate::plugin::Reference,
        configuration: crate::plugin::mcp::Configuration,
    },
    /// Controller-only credential editing; any plugin provenance is rejected.
    ReadPluginSecret {
        package: crate::plugin::Reference,
        field: String,
    },
    ReadPluginConnection {
        package: crate::plugin::Reference,
        card: String,
    },
    SavePluginSettings {
        package: crate::plugin::Reference,
        values: std::collections::BTreeMap<String, serde_json::Value>,
        secrets: std::collections::BTreeMap<String, crate::plugin::settings::SecretUpdate>,
    },
    /// Reads only declared desktop resources from the enabled, exact package version.
    ReadPluginView {
        #[serde(default)]
        surface: crate::plugin::desktop::Surface,
        package: crate::plugin::Reference,
    },
    /// Imports a directory from this execution Node's registered worktree.
    /// Installation discovers components but does not run package code.
    InstallPlugin {
        worktree: WorktreeId,
        path: String,
        name: String,
        expected_revision: u64,
        /// Rejects source changes after an update preview.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expected_digest: Option<String>,
    },
    SetPluginEnabled {
        name: String,
        expected_revision: u64,
        enabled: bool,
    },
    /// Removes the inventory entry; immutable resources remain for admitted tasks.
    RemovePlugin {
        name: String,
        expected_revision: u64,
    },
    ListRoles,
    PutRole {
        role: crate::role::Profile,
        expected_revision: u64,
    },
    RemoveRole {
        role: crate::RoleId,
        expected_revision: u64,
    },
    ListProviders,
    /// Explicit API-key read for the provider editor; never returns login tokens.
    ReadProviderKey {
        provider: ProviderId,
        expected_revision: u64,
    },
    BeginProviderLogin {
        provider: ProviderId,
        expected_revision: u64,
        redirect: Option<String>,
    },
    CompleteProviderLogin {
        attempt: RequestId,
        callback: Secret,
    },
    CancelProviderLogin {
        attempt: RequestId,
    },
    ReadCatalogStatus,
    ListChannelPresets {
        #[serde(default)]
        refresh_partners: bool,
    },
    RefreshModelCatalog,
    ReadModelCatalog(crate::conversation::catalog::Query),
    DiscoverModels(Box<crate::conversation::discovery::Source>),
    ValidateProvider {
        provider: ProviderId,
        expected_revision: u64,
    },
    PutProvider {
        provider: crate::conversation::Provider,
        expected_revision: u64,
    },
    ReadSession {
        session: SessionId,
    },
    ReadConversation {
        session: SessionId,
        before: Option<crate::TurnId>,
        /// Number of turns, not events or model messages.
        limit: u16,
    },
    ReadUsage(crate::usage::Query),
    ReadActivityCatalog,
    ReadActivity {
        sessions: Vec<SessionId>,
    },
    /// Reads the immutable admission capture of a visible turn on demand.
    ReadTurnCapture {
        session: SessionId,
        turn: crate::TurnId,
    },
    ReadTurn {
        session: SessionId,
        turn: crate::TurnId,
        expected_revision: u64,
        before: Option<u64>,
        limit: u16,
    },
    ListConversationAssets {
        session: SessionId,
        query: crate::conversation::assets::Query,
    },
    SearchConversation {
        session: SessionId,
        query: crate::conversation::search::Query,
    },
    ListFileCheckpoints {
        session: SessionId,
        turn: crate::TurnId,
        before: Option<crate::CheckpointId>,
        limit: u16,
    },
    ReadFileCheckpoint {
        session: SessionId,
        checkpoint: crate::CheckpointId,
    },
    ReadTurnDiff {
        session: SessionId,
        turn: crate::TurnId,
    },
    ReadTurnHunks {
        session: SessionId,
        turn: crate::TurnId,
        path: String,
    },
    ApplyTurnHunk(crate::conversation::checkpoint::hunk::Selection),
    /// Restores pre-write text only when the current file matches the captured write.
    RestoreFileCheckpoint {
        session: SessionId,
        checkpoint: crate::CheckpointId,
        worktree: WorktreeId,
    },
    /// Creates an independent session through a closed turn, without changing files or executing it.
    ForkConversation {
        session: SessionId,
        through: crate::TurnId,
        expected_revision: u64,
    },
    /// Retains a closed prefix and preserves the previous history in an independent session.
    RewindConversation {
        session: SessionId,
        through: Option<crate::TurnId>,
        /// Latest admitted nonqueued turn observed by the caller.
        expected_head: crate::TurnId,
        /// Structural history revision, not the effective configuration revision.
        expected_revision: u64,
    },
    /// Replaces a closed turn and its suffix, atomically admitting the edited input.
    ReplaceTurn {
        session: SessionId,
        turn: crate::TurnId,
        expected_head: crate::TurnId,
        expected_history_revision: u64,
        expected_revision: u64,
        message: crate::conversation::Input,
    },
    SaveProvider {
        provider: crate::conversation::Provider,
        expected_revision: u64,
        secret: Option<crate::Secret>,
    },
    RemoveProvider {
        provider: crate::ProviderId,
        expected_revision: u64,
    },
    /// Sends input and resumes an idle/stopping session; a running session queues it.
    SubmitTurn {
        session: SessionId,
        expected_revision: u64,
        message: crate::conversation::Input,
    },
    /// Native task submission freezes the current configuration on first admission.
    SubmitTask {
        session: SessionId,
        expected_revision: Option<u64>,
        message: crate::conversation::Input,
    },
    /// Guides an active task at its next model boundary using its frozen capture.
    GuideTurn {
        recipient: crate::TurnId,
        expected_revision: u64,
        message: crate::conversation::Input,
    },
    /// Node-internal admission of completed background work.
    DeliverBackground {
        session: SessionId,
    },
    /// Atomically admits an idle continuation fenced by the calling package's private data.
    ContinueTurn {
        session: SessionId,
        after: Option<crate::TurnId>,
        message: crate::conversation::Input,
        key: String,
        expected_revision: u64,
        scope: crate::plugin::storage::Scope,
    },
    /// Queues model-only context maintenance using the session's frozen configuration.
    CompactContext {
        session: SessionId,
        expected_revision: u64,
    },
    /// Makes a held turn eligible without bypassing the session pause or current run.
    StartQueuedTurn {
        turn: crate::TurnId,
    },
    StopTurn {
        turn: crate::TurnId,
    },
    RunCommand {
        turn: crate::TurnId,
        command: String,
        access: crate::process::Access,
        description: Option<String>,
        cwd: String,
        timeout_ms: u64,
        background: bool,
        #[serde(default)]
        pty: bool,
        /// Files from this turn's visible history, staged only for this command.
        attachments: Vec<crate::AttachmentId>,
    },
    ListCommands {
        session: SessionId,
    },
    ReadCommand {
        session: SessionId,
        id: RequestId,
    },
    StopCommands {
        session: SessionId,
    },
    StopCommand {
        session: SessionId,
        id: RequestId,
    },
    WriteCommand {
        session: SessionId,
        id: RequestId,
        input: String,
    },
    ResolveApproval {
        session: SessionId,
        approval: crate::ApprovalId,
        decision: crate::conversation::Decision,
    },
    ResolveQuestion {
        session: SessionId,
        question: crate::QuestionId,
        response: crate::conversation::question::Response,
    },
    ReadQueuedTurn {
        turn: crate::TurnId,
    },
    EditQueuedTurn {
        turn: crate::TurnId,
        expected_revision: u64,
        message: crate::conversation::Input,
    },
    RemoveQueuedTurn {
        turn: crate::TurnId,
        expected_revision: u64,
    },
    SendQueuedTurn {
        turn: crate::TurnId,
        expected_revision: u64,
    },
    /// Prioritizes compatible input without interrupting an unsettled tool batch.
    GuideQueuedTurn {
        turn: crate::TurnId,
        recipient: crate::TurnId,
        expected_revision: u64,
    },
    MoveQueuedTurn {
        session: SessionId,
        expected_revision: u64,
        turn: crate::TurnId,
        before: Option<crate::TurnId>,
    },
    SetQueuePaused {
        session: SessionId,
        expected_revision: u64,
        paused: bool,
    },
    ReadTerminalSettings,
    ReadNetworkSettings,
    SaveNetworkSettings {
        expected_revision: u64,
        selection: crate::network::Selection,
    },
    ReadSandboxSettings,
    SaveSandboxSettings(crate::sandbox::Settings),
    ReadSearchSettings,
    SaveSearchSettings(crate::search::Update),
    ReadBackupSettings,
    SaveBackupSettings(crate::backup::Update),
    ListBackups,
    ExportBackup {
        expected_revision: u64,
    },
    DownloadBackup {
        expected_revision: u64,
    },
    UploadBackup {
        size: u64,
        revision: String,
    },
    RestoreBackupFile {
        stream: StreamId,
        expected_revision: u64,
        target: String,
        #[serde(default)]
        passphrase: Option<crate::Secret>,
    },
    RestoreBackup {
        id: String,
        expected_revision: u64,
        target: String,
        #[serde(default)]
        passphrase: Option<crate::Secret>,
        #[serde(default, skip_serializing_if = "crate::backup::Location::is_online")]
        location: crate::backup::Location,
    },
    ReadPythonSettings,
    SavePythonSettings(crate::python::Settings),
    Decide(crate::decision::Request),
    ReadLocalModelSettings,
    SaveLocalModelSettings(crate::models::Settings),
    ReadLocalModels {
        kind: Option<crate::models::Kind>,
    },
    RefreshLocalModel {
        id: String,
    },
    RegisterLocalModel {
        definition: crate::models::Definition,
    },
    InstallLocalModel {
        id: String,
    },
    DeleteLocalModel {
        id: String,
    },
    CancelLocalModel {
        id: String,
    },
    ListShells,
    SaveTerminalSettings(crate::terminal::Settings),
    ListMediaModels,
    ReadMediaSettings,
    SaveMediaSettings(crate::media::Settings),
    SearchModels(crate::models::Search),
    ReadModelVersions {
        hub: crate::models::Hub,
        repository: String,
    },
    ResolveModel {
        kind: crate::models::Kind,
        source: crate::models::Source,
    },
    QueryKnowledge(crate::knowledge::Query),
    ReadKnowledgeCitation(crate::knowledge::CitationRequest),
    ListKnowledgeCollections,
    CreateKnowledgeCollection {
        definition: crate::knowledge::Definition,
        /// Only controllers may select a real installed plugin as the owner.
        plugin: Option<String>,
    },
    UpdateKnowledgeCollection {
        id: crate::KnowledgeId,
        expected_revision: u64,
        definition: crate::knowledge::Definition,
    },
    DeleteKnowledgeCollection {
        id: crate::KnowledgeId,
        expected_revision: u64,
    },
    RebuildKnowledgeCollection {
        id: crate::KnowledgeId,
        expected_revision: u64,
    },
    PauseKnowledgeCollection {
        id: crate::KnowledgeId,
        expected_revision: u64,
        paused: bool,
    },
    CreateTerminal(crate::terminal::Launch),
    /// Open an existing terminal, restoring an absent shell without replaying input.
    OpenTerminal {
        terminal: crate::TerminalId,
        viewport: crate::terminal::Viewport,
        appearance: crate::terminal::Appearance,
    },
    OpenToolTerminal {
        tool: crate::terminal::Tool,
        launch: crate::terminal::Launch,
    },
    ListTerminalTools {
        worktree: WorktreeId,
    },
    CloseTerminal {
        worktree: WorktreeId,
        terminal: crate::TerminalId,
    },
    ListTerminals {
        worktree: WorktreeId,
    },
    InspectTerminal {
        terminal: crate::TerminalId,
    },
    ClaimTerminal {
        terminal: crate::TerminalId,
        expected_revision: u64,
    },
    InputTerminal {
        terminal: crate::TerminalId,
        revision: u64,
        input: crate::terminal::Input,
    },
    ResizeTerminal {
        terminal: crate::TerminalId,
        revision: u64,
        viewport: crate::terminal::Viewport,
    },
    SetTerminalAppearance {
        terminal: crate::TerminalId,
        revision: u64,
        appearance: crate::terminal::Appearance,
    },
    Snapshot,
    InspectHost,
    ReadHostMetrics,
    UseMedia {
        turn: TurnId,
        session: SessionId,
        worktree: WorktreeId,
        action: crate::media::Action,
    },
    ReadComputerPermissions,
    RequestComputerPermission {
        permission: crate::computer::Permission,
    },
    StopHostProcess {
        pid: u32,
        started_at_secs: u64,
        force: bool,
    },
    /// Reads the caller's existing admission without submitting the original command.
    InspectRequest {
        id: RequestId,
        /// Lowercase BLAKE3 of the serialized original request, including its target and ID.
        digest: String,
    },
    InspectGit {
        worktree: WorktreeId,
    },
    ListGitBranches {
        worktree: WorktreeId,
    },
    /// Resolves a Git revision to an immutable commit without changing checkout.
    ResolveGitRevision {
        worktree: WorktreeId,
        revision: String,
    },
    ListWorktrees {
        worktree: WorktreeId,
    },
    /// Registers an existing worktree from the project's Git repository.
    RegisterWorktree {
        project: ProjectId,
        path: String,
    },
    /// Creates a linked worktree and new branch from an immutable commit.
    CreateWorktree {
        project: ProjectId,
        path: String,
        branch: String,
        commit: String,
    },
    /// Create an isolated checkout in the execution Node's managed worktree directory.
    CreateManagedWorktree {
        project: ProjectId,
        source: WorktreeId,
        branch: String,
        expected_head: String,
        expected_index: String,
        include_changes: bool,
    },
    /// Removes a clean linked worktree without active turns, retaining its branch
    /// and the history of sessions that used it.
    RemoveWorktree {
        worktree: WorktreeId,
        expected_head: String,
        expected_branch: String,
    },
    /// Creates a local branch at an immutable commit without changing checkout.
    CreateGitBranch {
        worktree: WorktreeId,
        name: String,
        commit: String,
    },
    /// Renames a local branch at the displayed commit, retaining checkout and tracking.
    RenameGitBranch {
        worktree: WorktreeId,
        name: String,
        new_name: String,
        commit: String,
    },
    /// Deletes a merged local branch without changing checkout or remote refs.
    DeleteGitBranch {
        worktree: WorktreeId,
        name: String,
        commit: String,
        expected_head: String,
        expected_branch: Option<String>,
    },
    SwitchGitBranch {
        worktree: WorktreeId,
        name: String,
        commit: String,
        expected_head: Option<String>,
        expected_branch: Option<String>,
        expected_index: String,
    },
    /// Merges the displayed local branch into the target worktree's checked-out branch.
    MergeGitBranch {
        worktree: WorktreeId,
        name: String,
        commit: String,
        expected_head: String,
        expected_branch: String,
        expected_index: String,
    },
    ReadGitStash {
        worktree: WorktreeId,
        commit: String,
    },
    ReadGitOutput {
        worktree: WorktreeId,
    },
    ListGitRemoteTags {
        worktree: WorktreeId,
        remote: String,
    },
    ReadGitLog {
        worktree: WorktreeId,
        limit: usize,
        cursor: Option<crate::GitLogCursor>,
    },
    ReadGitCommit {
        worktree: WorktreeId,
        commit: String,
    },
    ListGitChanges {
        worktree: WorktreeId,
        target: Option<crate::git::GitChangeTarget>,
        cursor: Option<crate::git::GitChangesCursor>,
        limit: usize,
    },
    ReadGitChange {
        worktree: WorktreeId,
        snapshot: crate::git::GitChangeSnapshot,
        path: String,
        expected_metadata_revision: String,
        expected_revision: Option<String>,
        offset: usize,
        limit: usize,
    },
    ReadGitContext {
        worktree: WorktreeId,
        snapshot: crate::git::GitChangeSnapshot,
        path: String,
        expected_metadata_revision: String,
        expected_revision: String,
        side: crate::git::GitContextSide,
        offset: usize,
        limit: usize,
    },
    ReadGitShow {
        worktree: WorktreeId,
        revision: String,
        path: Option<String>,
        offset: usize,
        limit: usize,
    },
    ReadGitBlame {
        worktree: WorktreeId,
        revision: Option<String>,
        path: String,
        offset: usize,
        limit: usize,
    },
    SearchFiles {
        worktree: WorktreeId,
        options: crate::FileSearch,
    },
    RunGitAction {
        worktree: WorktreeId,
        action: crate::GitAction,
        expected_index: String,
        expected_head: Option<String>,
        expected_branch: Option<String>,
    },
    CreateGitCommit {
        worktree: WorktreeId,
        message: String,
        amend: bool,
        options: crate::GitCommitOptions,
        expected_index: String,
        expected_head: Option<String>,
        expected_branch: Option<String>,
    },
    UpdateGitIndex {
        worktree: WorktreeId,
        paths: Vec<String>,
        operation: crate::GitIndexChange,
        expected_index: String,
        expected_head: Option<String>,
    },
    ReadGitDiff {
        worktree: WorktreeId,
        path: String,
        scope: crate::GitDiffScope,
    },
    ListDirectory {
        worktree: WorktreeId,
        path: String,
        after: Option<crate::DirectoryCursor>,
    },
    ReadFile {
        worktree: WorktreeId,
        path: String,
    },
    ReadCode {
        worktree: WorktreeId,
        query: crate::code::Query,
        overlay: Option<crate::code::Overlay>,
    },
    ListCodeServers {
        worktree: WorktreeId,
    },
    ListCodePackages {
        worktree: WorktreeId,
    },
    InstallCodeServer {
        install: crate::code::Install,
    },
    CancelCodeInstall {
        package: String,
    },
    CancelCodeQuery {
        request: RequestId,
    },
    ReadCodeCatalog,
    ListCodeInstallations,
    UninstallCodeServer {
        package: String,
        version: String,
    },
    ReadCodeRules,
    UpdateCodeRules,
    ReadCodeSettings,
    ReadHooks {
        worktree: WorktreeId,
    },
    SaveHooks {
        worktree: WorktreeId,
        settings: crate::hooks::Settings,
        expected_revision: u64,
    },
    SaveCodeSettings {
        settings: crate::code::Settings,
        expected_revision: u64,
    },
    StartCodeServer {
        startup: crate::code::Startup,
    },
    StopCodeServer {
        startup: crate::code::Startup,
    },
    ReadFileLines {
        worktree: WorktreeId,
        path: String,
        offset: usize,
        limit: usize,
    },
    FindFiles {
        worktree: WorktreeId,
        globs: Vec<String>,
        #[serde(default)]
        path: String,
        #[serde(default = "find_limit")]
        limit: usize,
        #[serde(default)]
        offset: usize,
    },
    ReadOffice {
        worktree: WorktreeId,
        options: crate::office::Read,
    },
    ReadNotebook {
        worktree: WorktreeId,
        options: crate::notebook::Read,
    },
    EditNotebook {
        worktree: WorktreeId,
        edit: crate::notebook::Edit,
    },
    /// Prepares a bounded-lifetime binary download without mutating the source.
    DownloadFile {
        worktree: WorktreeId,
        path: String,
    },
    /// Allocates a bounded-lifetime upload stream without changing the target file.
    UploadFile(crate::FileUploadSpec),
    UploadAttachment(crate::attachment::Spec),
    FinishAttachmentUpload {
        worktree: WorktreeId,
        stream: crate::StreamId,
    },
    ReadAttachment {
        worktree: WorktreeId,
        attachment: crate::AttachmentId,
    },
    DownloadAttachment {
        worktree: WorktreeId,
        attachment: crate::AttachmentId,
    },
    /// Downloads an image from visible canonical history without copying it.
    DownloadImage {
        session: SessionId,
        image: crate::conversation::Image,
    },
    DiscardAttachment {
        worktree: WorktreeId,
        attachment: crate::AttachmentId,
    },
    /// Publishes verified staging through the existing durable command ledger.
    FinishFileUpload {
        worktree: WorktreeId,
        path: String,
        stream: crate::StreamId,
    },
    /// Cancels only the caller's pending or active file transfer.
    CancelFileTransfer {
        stream: crate::StreamId,
    },
    WriteFile {
        worktree: WorktreeId,
        path: String,
        text: String,
        /// None creates a new file and never replaces an existing entry.
        expected_revision: Option<String>,
    },
    EditFile {
        worktree: WorktreeId,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        old_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_text: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        replace_all: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edits: Option<Vec<crate::FileEdit>>,
        expected_revision: String,
    },
    /// Moves a regular file to Trash only if its current content matches the revision.
    DeleteFile {
        worktree: WorktreeId,
        path: String,
        expected_revision: String,
    },
    /// Moves a regular file within its worktree without replacing a destination.
    MoveFile {
        worktree: WorktreeId,
        from: String,
        to: String,
        expected_revision: String,
    },
    /// Creates one directory in an existing parent, without replacing an entry.
    CreateDirectory {
        worktree: WorktreeId,
        path: String,
    },
    /// Renames an entry within its worktree after checking for a destination conflict.
    RenameEntry {
        worktree: WorktreeId,
        from: String,
        to: String,
    },
    /// Copies a file or directory within its worktree after checking destination absence.
    /// Reads saved contents, rejects symbolic links, and omits nested Git metadata.
    CopyEntry {
        worktree: WorktreeId,
        from: String,
        to: String,
    },
    /// Copies saved contents between registered worktrees on this execution Node.
    /// Both source and destination are explicit, authenticated resource identities.
    CopyEntryTo {
        source: WorktreeId,
        worktree: WorktreeId,
        from: String,
        to: String,
    },
    /// Moves an entry between registered worktrees on this execution Node.
    /// Uses a filesystem rename, or verified copy followed by Trash across filesystems.
    MoveEntryTo {
        source: WorktreeId,
        worktree: WorktreeId,
        from: String,
        to: String,
    },
    /// Moves an entry to the execution Node's system Trash, never permanent deletion.
    TrashEntry {
        worktree: WorktreeId,
        path: String,
    },
    /// Recycles a regular file only if its content and source stamp still match.
    /// The controller must verify a published destination before submitting this command.
    TrashFile {
        worktree: WorktreeId,
        path: String,
        expected_revision: String,
        expected_stamp: String,
    },
    ListCredentials,
    PutCredential {
        id: CredentialId,
        provider: ProviderId,
        expected_revision: u64,
        secret: Secret,
        expires_at_ms: Option<u64>,
    },
    RevokeCredential {
        id: CredentialId,
        expected_revision: u64,
    },
    RegisterProject {
        name: String,
        path: String,
    },
    CreateProject(crate::projects::Draft),
    UpdateProject {
        expected: Project,
        name: String,
        path: String,
        appearance: crate::projects::Appearance,
    },
    /// Removes the inventory entry, retaining files and session history.
    RemoveProject {
        expected: Project,
    },
    SetDefaults {
        expected_revision: u64,
        config: SessionConfig,
    },
    BindConnectionSession {
        session: SessionId,
        expected_revision: u64,
        resource: crate::connection::Resource,
    },
    CreateSession {
        project: Option<ProjectId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        worktree: Option<WorktreeId>,
        config: Option<SessionConfig>,
    },
    StartSession(crate::conversation::Start),
    ReadProjectCatalog,
    /// Registered resources in the captured worktree's project.
    ReadWorktreeCatalog {
        worktree: WorktreeId,
    },
    /// Executed only by the configuration owner's local controller.
    CreateSessionAt {
        target: NodeId,
        project: Option<ProjectId>,
        worktree: Option<WorktreeId>,
        config: Box<SessionConfig>,
        provider_revision: u64,
    },
    ImportSession(Box<crate::SessionImport>),
    RenameSession {
        session: SessionId,
        expected_revision: u64,
        title: String,
    },
    /// Compare the attention revision, not the session configuration revision.
    SetSessionRead {
        session: SessionId,
        expected_revision: u64,
        read: bool,
    },
    SetSessionArchived {
        session: SessionId,
        expected_revision: u64,
        archived: bool,
    },
    /// Archive the selected Node sessions in one revision-checked transaction.
    ArchiveSessions {
        expected: Vec<SessionRevision>,
    },
    /// Remove selected sessions and their owned children atomically.
    RemoveSessions {
        expected: Vec<SessionRevision>,
    },
    /// Reorder one project's sessions without changing their execution configuration.
    SetSessionOrder {
        project: ProjectId,
        expected: Vec<SessionId>,
        sessions: Vec<SessionId>,
    },
    RemoveSession {
        session: SessionId,
        expected_revision: u64,
    },
    /// Change only future execution; admitted turns retain their original worktree.
    MoveConversation {
        session: SessionId,
        worktree: WorktreeId,
        expected_revision: u64,
    },
    /// Branch visible history into another worktree in the same project.
    ForkConversationAt {
        session: SessionId,
        worktree: WorktreeId,
        expected_revision: u64,
    },
    SetSessionConfig {
        session: SessionId,
        expected_revision: u64,
        config: SessionConfig,
    },
    /// Replace the session roster after checking the selected catalog revisions.
    SetSessionRoles {
        session: SessionId,
        expected_revision: u64,
        roles: Vec<crate::role::Reference>,
    },
    QueueTurn {
        session: SessionId,
        expected_revision: u64,
        message: crate::conversation::Input,
    },
}

impl Command {
    pub fn durable(&self) -> bool {
        !matches!(
            self,
            Self::Dispatch {
                action: crate::dispatch::Command::ListHandlers
                    | crate::dispatch::Command::ListSchedules
                    | crate::dispatch::Command::ListJobs { .. }
                    | crate::dispatch::Command::ReadJob { .. }
                    | crate::dispatch::Command::ReadResult { .. },
                ..
            } | Self::CompleteBrowser { .. }
                | Self::Snapshot
                | Self::ReadArtifact { .. }
                | Self::ListArtifacts { .. }
                | Self::ListArtifactRevisions { .. }
                | Self::ReadProjectCatalog
                | Self::ReadActivityCatalog
                | Self::ReadWorktreeCatalog { .. }
                | Self::OpenPort { .. }
                | Self::CancelPort { .. }
                | Self::ReadHostInstall { .. }
                | Self::BrowseSshDirectory { .. }
                | Self::DownloadSshFile { .. }
                | Self::StageSshUpload(_)
                | Self::ListSsh
                | Self::ListSshTerminals { .. }
                | Self::ListDatabases
                | Self::ListPlugins
                | Self::ListConnectorTypes
                | Self::InspectConnectorCli { .. }
                | Self::ListConnections
                | Self::ReadSkillDocument { .. }
                | Self::DiscoverSkills { .. }
                | Self::DiscoverRegistrySkill { .. }
                | Self::DiscoverSkillUpload { .. }
                | Self::DiscoverLocalSkills { .. }
                | Self::SearchPluginCatalog { .. }
                | Self::SearchSkillCatalog { .. }
                | Self::ReadCatalogPlugin { .. }
                | Self::ReadCatalogPluginInfo { .. }
                | Self::InspectPluginSource { .. }
                | Self::CheckPluginUpdate { .. }
                | Self::CheckPluginUpdates { .. }
                | Self::UploadPlugin(_)
                | Self::InspectPluginUpload { .. }
                | Self::ReadPluginCallback { .. }
                | Self::ReadPlugin { .. }
                | Self::ReadPluginVersion { .. }
                | Self::ReadPluginSettings { .. }
                | Self::ReadPluginMcp { .. }
                | Self::ReadPluginValue { .. }
                | Self::ReadPluginConversationValue { .. }
                | Self::ListPluginKeys { .. }
                | Self::SearchPluginValues(_)
                | Self::ListPluginModels
                | Self::ResolvePluginModel { .. }
                | Self::ReadPluginConnection { .. }
                | Self::ReadPluginView { .. }
                | Self::ListRoles
                | Self::ListProviders
                | Self::ReadProviderKey { .. }
                | Self::ListChannelPresets { .. }
                | Self::ReadCatalogStatus
                | Self::RefreshModelCatalog
                | Self::ReadModelCatalog(_)
                | Self::RequestProviderHttp { .. }
                | Self::DiscoverModels(_)
                | Self::ValidateProvider { .. }
                | Self::ReadConversation { .. }
                | Self::ReadSession { .. }
                | Self::ReadUsage(_)
                | Self::ReadActivity { .. }
                | Self::ReadTurnCapture { .. }
                | Self::ReadTurn { .. }
                | Self::SearchConversation { .. }
                | Self::ListConversationAssets { .. }
                | Self::ListFileCheckpoints { .. }
                | Self::ReadFileCheckpoint { .. }
                | Self::ReadTurnDiff { .. }
                | Self::ReadTurnHunks { .. }
                | Self::ReadQueuedTurn { .. }
                | Self::ListTerminals { .. }
                | Self::ListTerminalTools { .. }
                | Self::ListCommands { .. }
                | Self::ReadCommand { .. }
                | Self::ReadTerminalSettings
                | Self::ReadNetworkSettings
                | Self::ReadSandboxSettings
                | Self::ReadSearchSettings
                | Self::ReadBackupSettings
                | Self::ListBackups
                | Self::DownloadBackup { .. }
                | Self::UploadBackup { .. }
                | Self::ReadPythonSettings
                | Self::DismissInstallation { .. }
                | Self::ReadInstallations { .. }
                | Self::Decide(_)
                | Self::ReadLocalModelSettings
                | Self::ReadLocalModels { .. }
                | Self::RefreshLocalModel { .. }
                | Self::CancelLocalModel { .. }
                | Self::ListShells
                | Self::ReadPluginSecret { .. }
                | Self::ListMediaModels
                | Self::ReadMediaSettings
                | Self::SearchModels(_)
                | Self::ReadModelVersions { .. }
                | Self::ResolveModel { .. }
                | Self::QueryKnowledge(_)
                | Self::ReadKnowledgeCitation(_)
                | Self::ListKnowledgeCollections
                | Self::InspectTerminal { .. }
                | Self::ClaimTerminal { .. }
                | Self::InputTerminal { .. }
                | Self::ResizeTerminal { .. }
                | Self::SetTerminalAppearance { .. }
                | Self::InspectRequest { .. }
                | Self::ListCredentials
                | Self::BrowseDatabase { .. }
                | Self::TestDatabase { .. }
                | Self::InspectHost
                | Self::ReadHostMetrics
                | Self::ReadComputerPermissions
                | Self::RequestComputerPermission { .. }
                | Self::ListDirectory { .. }
                | Self::BrowseFiles { .. }
                | Self::ReadFile { .. }
                | Self::ReadCode { .. }
                | Self::FetchWeb(_)
                | Self::CancelWebFetch { .. }
                | Self::SearchWeb(_)
                | Self::CancelWebSearch { .. }
                | Self::TestSearchDriver { .. }
                | Self::ListCodeServers { .. }
                | Self::ListCodePackages { .. }
                | Self::ReadCodeCatalog
                | Self::ListCodeInstallations
                | Self::CancelCodeInstall { .. }
                | Self::CancelCodeQuery { .. }
                | Self::ReadCodeRules
                | Self::ReadCodeSettings
                | Self::ReadHooks { .. }
                | Self::ReadFileLines { .. }
                | Self::FindFiles { .. }
                | Self::ReadOffice { .. }
                | Self::ReadNotebook { .. }
                | Self::DownloadFile { .. }
                | Self::UploadFile(_)
                | Self::UploadAttachment(_)
                | Self::ReadAttachment { .. }
                | Self::DownloadAttachment { .. }
                | Self::DownloadImage { .. }
                | Self::CancelFileTransfer { .. }
                | Self::SearchFiles { .. }
                | Self::InspectGit { .. }
                | Self::ListGitBranches { .. }
                | Self::ResolveGitRevision { .. }
                | Self::ListWorktrees { .. }
                | Self::ReadGitStash { .. }
                | Self::ReadGitOutput { .. }
                | Self::ListGitRemoteTags { .. }
                | Self::ReadGitLog { .. }
                | Self::ReadGitCommit { .. }
                | Self::ReadGitShow { .. }
                | Self::ListGitChanges { .. }
                | Self::ReadGitChange { .. }
                | Self::ReadGitContext { .. }
                | Self::ReadGitBlame { .. }
                | Self::ReadGitDiff { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub version: u16,
    pub id: RequestId,
    pub target: NodeId,
    pub command: Command,
    /// Present only for a package-contributed software action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<crate::plugin::Context>,
}

impl Request {
    pub fn new(target: NodeId, command: Command) -> Self {
        Self {
            version: crate::VERSION,
            id: RequestId::new(),
            target,
            command,
            plugin: None,
        }
    }

    pub fn with_plugin(mut self, context: crate::plugin::Context) -> Self {
        self.plugin = Some(context);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub node: NodeId,
    pub cursor: u64,
    pub defaults: Defaults,
    pub projects: Vec<Project>,
    pub worktrees: Vec<Worktree>,
    pub sessions: Vec<Session>,
    pub terminals: Vec<crate::terminal::Info>,
    pub terminal_settings_revision: u64,
    pub sandbox_settings_revision: u64,
    pub search_settings_revision: u64,
    pub media_settings: crate::media::Settings,
    pub providers: Vec<crate::conversation::Provider>,
    pub provider_quotas: Vec<crate::conversation::quota::Snapshot>,
    pub model_catalog: crate::conversation::catalog::Status,
    pub roles: Vec<crate::role::Profile>,
    pub ssh: Vec<crate::ssh::Profile>,
    pub databases: Vec<crate::database::Profile>,
    pub plugins: Vec<crate::plugin::Summary>,
    pub notifications: Vec<crate::notification::Notice>,
    /// Admission identities only; complete captures are read with ReadTurnCapture.
    pub turns: Vec<crate::TurnSummary>,
}

/// Retained metadata keeps its original meaning without reviving a retired command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedRecord {
    pub kind: String,
    pub data: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Output {
    RetainedRecord(RetainedRecord),
    InstallationDismissed {
        request: RequestId,
    },
    Installations(Vec<crate::installation::Task>),
    Artifact(crate::artifact::Content),
    ArtifactWritten {
        artifact: crate::artifact::Summary,
        warnings: Vec<String>,
    },
    Artifacts(crate::artifact::Page),
    ArtifactRevisions(crate::artifact::RevisionPage),
    ModelSearch(crate::models::Page),
    ModelVersions(Vec<crate::models::Version>),
    ModelDefinition(crate::models::Definition),
    KnowledgeSearch(crate::knowledge::Page),
    KnowledgeCitation(crate::knowledge::CitationPage),
    KnowledgeCollections(Vec<crate::knowledge::Collection>),
    KnowledgeCollection(crate::knowledge::Collection),
    KnowledgeCollectionDeleted {
        id: crate::KnowledgeId,
    },
    Code(crate::code::Result),
    HookSettings(crate::hooks::State),
    CodeSettings(crate::code::Settings),
    CodeServer(crate::code::Server),
    CodeServers(Vec<crate::code::Server>),
    CodePackages(crate::code::Workspace),
    CodeCatalog(Vec<crate::code::Language>),
    CodeInstallation(crate::code::Installation),
    CodeInstallations(Vec<crate::code::Installation>),
    CodeUninstalled {
        package: String,
        version: String,
    },
    CodeRules(crate::code::Rules),
    CodeQueryCancelled {
        request: RequestId,
    },
    ActivityCatalog(crate::activity::Catalog),
    ProjectCatalog(crate::projects::Catalog),
    PluginTransaction(Vec<Output>),
    PluginResult(serde_json::Value),
    PluginCallStopping {
        request: RequestId,
    },
    Notification(crate::notification::Notice),
    NotificationsDismissed,
    Dispatch(crate::dispatch::Output),
    BrowserCompleted,
    Browser(serde_json::Value),
    ExternalBrowser(serde_json::Value),
    PluginValue(crate::plugin::storage::Entry),
    PluginConversationValue(crate::plugin::storage::ConversationEntry),
    PluginHttp(crate::plugin::http::Response),
    WebPage(crate::web::Page),
    SearchPage(crate::search::Page),
    WebSearchCancelled {
        request: RequestId,
    },
    WebFetchCancelled {
        request: RequestId,
    },
    PluginKeys(crate::plugin::storage::Page),
    PluginSearch(crate::plugin::storage::SearchPage),
    PluginText(crate::plugin::Text),
    PluginModels(crate::plugin::models::Catalog),
    SessionConfig(SessionConfig),
    FileListing(crate::file_browser::Listing),
    FilesManaged(Vec<crate::file_browser::Outcome>),
    PortStream {
        stream: crate::StreamId,
    },
    PortCancelled,
    DatabaseProfiles(Vec<crate::database::Profile>),
    DatabaseProfile(crate::database::Profile),
    DatabaseOutcome(crate::database::Outcome),
    DatabaseCancelled {
        request: RequestId,
    },
    SshProfiles(Vec<crate::ssh::Profile>),
    SshProfile(crate::ssh::Profile),
    HostInstallProgress(crate::ssh::InstallProgress),
    SshOutcome(crate::ssh::Outcome),
    SshUpload(crate::ssh::Upload),
    SshCancelled {
        request: RequestId,
    },
    PluginConnection(crate::plugin::connection::Status),
    ConnectorTypes(Vec<crate::plugin::connector::Type>),
    ConnectorIdentity(crate::plugin::connector::Identity),
    Connections(Vec<crate::plugin::connector::Connection>),
    Connection(crate::plugin::connector::Connection),
    ConnectorLogin(crate::plugin::connector::Attempt),
    Plugin(crate::plugin::Info),
    CatalogDetails(crate::plugin::catalog::Details),
    SkillDocument(crate::plugin::skills::Document),
    SkillDiscovery(crate::plugin::skills::Discovery),
    RegistrySkillDiscovery(crate::plugin::skills::RegistryDiscovery),
    SkillImportDiscovery(crate::plugin::skills::ImportDiscovery),
    PluginCatalog(crate::plugin::catalog::Page),
    SkillCatalog(crate::plugin::skillcatalog::Page),
    PluginSource(crate::plugin::catalog::Selection),
    PluginUpdate(crate::plugin::updates::Report),
    PluginUpdates(crate::plugin::updates::Batch),
    PluginRepository(crate::plugin::skills::Source),
    PluginUpload(crate::plugin::Upload),
    PluginSettings(crate::plugin::settings::State),
    PluginMcp(crate::plugin::mcp::State),
    PluginSecret(Option<crate::Secret>),
    PluginView(crate::plugin::desktop::Bundle),
    Plugins(Vec<crate::plugin::Summary>),
    Role(crate::role::Profile),
    Roles(Vec<crate::role::Profile>),
    CommandResult(crate::process::Completion),
    CommandStarted(crate::process::Info),
    CommandInputAccepted {
        id: RequestId,
        bytes: usize,
    },
    Commands(Vec<crate::process::Info>),
    CommandOutput(crate::process::Snapshot),
    Provider(crate::conversation::Provider),
    Providers(Vec<crate::conversation::Provider>),
    ProviderKey(Option<crate::Secret>),
    ProviderLogin(crate::conversation::login::Attempt),
    ChannelPresets(crate::conversation::presets::Listing),
    CatalogStatus(crate::conversation::catalog::Status),
    ModelCatalog(crate::conversation::catalog::Page),
    Conversation(crate::conversation::History),
    Usage(crate::usage::Report),
    Activity(Vec<crate::activity::Preview>),
    DiscoveredModels(crate::conversation::discovery::Catalog),
    ProviderValidation(crate::conversation::discovery::Validation),
    TurnHistory(crate::conversation::TurnHistory),
    ConversationAssets(crate::conversation::assets::Page),
    ConversationMatches(crate::conversation::search::Page),
    FileCheckpoints(crate::conversation::checkpoint::Page),
    TurnDiff(crate::conversation::checkpoint::TurnDiff),
    TurnHunks(crate::conversation::checkpoint::hunk::Review),
    HunkApplied(crate::conversation::checkpoint::hunk::Applied),
    FileCheckpoint(Box<crate::conversation::checkpoint::Content>),
    FileRestored(crate::conversation::checkpoint::Restored),
    Rewound(Box<crate::conversation::Rewind>),
    TurnReplaced {
        turn: crate::QueuedTurn,
        history: Box<crate::conversation::Rewind>,
    },
    Run(crate::conversation::Run),
    Approval(crate::conversation::Approval),
    Question(crate::conversation::question::Question),
    PlanAccepted(Box<crate::conversation::question::AcceptedPlan>),
    Queue(crate::conversation::QueueUpdate),
    QueuedMessage(crate::conversation::QueuedMessage),
    Terminal(crate::terminal::Info),
    TerminalSettings(crate::terminal::Settings),
    NetworkSettings(crate::network::Settings),
    SandboxSettings(crate::sandbox::Settings),
    SearchSettings(crate::search::Settings),
    BackupSettings(crate::backup::Settings),
    Backups(crate::backup::Page),
    Backup(crate::backup::Record),
    BackupDownload(crate::backup::Download),
    BackupUpload(crate::backup::Upload),
    BackupRestored {
        record: crate::backup::Record,
        target: String,
    },
    PythonSettings(crate::python::Settings),
    LocalModelSettings(crate::models::Settings),
    LocalModels(crate::models::Catalog),
    Decision(crate::decision::Response),
    LocalModel {
        id: String,
    },
    Shells(Vec<String>),
    TerminalTools(Vec<crate::terminal::ToolInfo>),
    MediaSettings(crate::media::Settings),
    MediaModels(Vec<crate::media::Candidate>),
    Media(serde_json::Value),
    Terminals(Vec<crate::terminal::Info>),
    TerminalSnapshot(crate::terminal::Snapshot),
    TerminalInput {
        terminal: crate::TerminalId,
    },
    RequestOutcome {
        id: RequestId,
        outcome: RequestOutcome,
    },
    SearchResults(crate::SearchResults),
    Snapshot(Snapshot),
    HostInfo(crate::HostInfo),
    HostMetrics(crate::host::metrics::Sample),
    ComputerPermissions(crate::computer::Permissions),
    HostProcessSignalled {
        pid: u32,
        started_at_secs: u64,
    },
    Directory(crate::Directory),
    FileContent(crate::FileContent),
    FileLines(crate::FileLines),
    FileNames(crate::FileNames),
    FileDownload(crate::FileDownload),
    FileUpload(crate::FileUpload),
    Attachment(crate::attachment::Attachment),
    AttachmentUpload(crate::attachment::Upload),
    AttachmentDownload(crate::attachment::Download),
    AttachmentDiscarded {
        attachment: crate::AttachmentId,
    },
    FileTransferCancelled {
        stream: crate::StreamId,
    },
    FileWritten(crate::FileWritten),
    OfficeContent(crate::office::Inspection),
    NotebookPage(crate::notebook::Page),
    DirectoryCreated {
        path: String,
    },
    EntryRenamed {
        from: String,
        to: String,
    },
    EntryCopied {
        from: String,
        to: String,
    },
    EntryMoved {
        from: String,
        to: String,
    },
    EntryTrashed {
        path: String,
    },
    GitStatus(crate::GitStatus),
    GitBranches(crate::GitBranches),
    GitRevision {
        commit: String,
    },
    GitWorktrees(crate::GitWorktrees),
    GitBranchCreated(crate::GitBranch),
    GitBranchRenamed(crate::GitBranch),
    GitMerged(crate::GitMerge),
    GitBranchSwitched(crate::GitBranch),
    GitBranchDeleted {
        name: String,
    },
    GitLog(crate::GitLog),
    GitCommit(crate::GitCommit),
    GitFile(crate::git::GitFile),
    GitChanges(crate::git::GitChanges),
    GitChange(crate::git::GitChange),
    GitContext(crate::git::GitContext),
    GitBlame(crate::git::GitBlame),
    GitActionCompleted,
    GitOutput {
        text: String,
    },
    GitRemoteTags(Vec<crate::GitTag>),
    GitCommitCreated {
        id: String,
        follow_up: Option<Fault>,
    },
    GitDiff(crate::GitDiff),
    GitIndex(crate::GitIndex),
    Credential(Credential),
    Credentials(Vec<Credential>),
    Project(Project),
    ProjectRemoved {
        id: ProjectId,
    },
    Worktree(Worktree),
    WorktreeRemoved {
        id: WorktreeId,
    },
    Defaults(Defaults),
    Session(Session),
    SessionsArchived(Vec<Session>),
    SessionsRemoved(Vec<SessionId>),
    SessionOrder(Vec<SessionId>),
    QueuedTurn(QueuedTurn),
}

/// A read-only observation of the original durable admission, not a new receipt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RequestOutcome {
    /// No matching admission exists for this authenticated caller at the time of the read.
    NotAdmitted,
    Admitted,
    Completed(Box<Result<Output, Fault>>),
    /// The Node cannot determine the result and will not replay the side effect.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Historical metadata whose original feature no longer has a live API.
    RetainedRecord(RetainedRecord),
    LocalModelsChanged {
        models: Vec<crate::models::Model>,
    },
    LocalModelSettingsChanged {
        revision: u64,
    },
    /// Invalidates collection metadata without exposing source paths or values.
    KnowledgeCollectionsChanged,
    TaskStarted {
        session: Box<Session>,
        turn: QueuedTurn,
    },
    NotificationChanged(crate::notification::Notice),
    NotificationsDismissed {
        ids: Vec<NotificationId>,
    },
    DispatchChanged {
        packages: Vec<String>,
    },
    MediaSettingsChanged(crate::media::Settings),
    TerminalSettingsChanged {
        revision: u64,
    },
    SandboxSettingsChanged {
        revision: u64,
    },
    SearchSettingsChanged {
        revision: u64,
    },
    BackupSettingsChanged {
        revision: u64,
    },
    PythonSettingsChanged {
        revision: u64,
    },
    PluginChanged(crate::plugin::Summary),
    /// Invalidates private package data without broadcasting its keys or values.
    PluginValuesChanged {
        name: String,
    },
    PluginRemoved {
        name: String,
    },
    RoleChanged(crate::role::Profile),
    SshChanged(crate::ssh::Profile),
    SshRemoved {
        id: crate::SshId,
    },
    DatabaseChanged(crate::database::Profile),
    DatabaseRemoved {
        id: crate::DatabaseId,
    },
    RoleRemoved {
        id: crate::RoleId,
    },
    ProviderChanged(crate::conversation::Provider),
    /// Invalidates public credential metadata; authorization values stay on the Node.
    CredentialChanged {
        credential: crate::CredentialRef,
        revision: u64,
    },
    ProviderQuotaChanged {
        provider: ProviderId,
        quota: Option<crate::conversation::quota::Snapshot>,
    },
    ModelCatalogChanged(crate::conversation::catalog::Status),
    ProviderRemoved {
        id: crate::ProviderId,
    },
    ConversationChanged {
        session: SessionId,
        activity: Box<crate::activity::Summary>,
    },
    TerminalChanged(crate::terminal::Info),
    ProjectRegistered(Project),
    ProjectChanged(Project),
    ProjectRemoved {
        id: ProjectId,
    },
    WorktreeRegistered(Worktree),
    WorktreeRemoved {
        id: WorktreeId,
    },
    DefaultsChanged(Defaults),
    SessionChanged(Box<Session>),
    SessionsRemoved(Vec<SessionId>),
    SessionsReordered(Vec<SessionId>),
    SessionRewound {
        session: Box<Session>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backup: Option<Box<Session>>,
    },
    TurnQueued {
        turn: QueuedTurn,
        activity: Box<crate::activity::Summary>,
    },
    PlanAccepted(Box<crate::conversation::question::AcceptedPlan>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub node: NodeId,
    pub cursor: u64,
    pub event: Event,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Update {
    Installations {
        node: NodeId,
        tasks: Vec<crate::installation::Task>,
    },
    CodeInstallations {
        node: NodeId,
        installations: Vec<crate::code::Installation>,
    },
    Python {
        node: NodeId,
        preparations: Vec<crate::python::Preparation>,
    },
    CodeServers {
        node: NodeId,
        worktree: WorktreeId,
        editor_enabled: bool,
        servers: Vec<crate::code::Server>,
        installations: Vec<crate::code::Installation>,
    },
    Commands {
        node: NodeId,
        session: SessionId,
        items: Vec<crate::process::Info>,
    },
    BrowserCall(crate::browser::Call),
    ConnectorLogin(crate::plugin::connector::LoginUpdate),
    ConnectionsChanged(crate::plugin::connector::Changes),
    ProviderLogin(crate::conversation::login::Update),
    ConversationSnapshot(crate::conversation::Snapshot),
    ConversationFrame(crate::conversation::Frame),
    TerminalSnapshot(crate::terminal::Snapshot),
    TerminalFrame(crate::terminal::Frame),
    /// Re-read loaded directories and clean documents, including after reconnect or overflow.
    FilesChanged {
        node: NodeId,
        worktree: WorktreeId,
    },
    Snapshot(Snapshot),
    Event(EventEnvelope),
    ResetRequired,
}

/// Transient subscriptions do not append to the durable Node event history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Topic {
    Installations,
    ScopedInstallations(Box<Request>),
    Python,
    CodeServers(WorktreeId),
    CodeInstallations,
    Commands(SessionId),
    Browser,
    ConnectorLogin(RequestId),
    Connections,
    ProviderLogin(RequestId),
    Conversation(crate::SessionId),
    Terminal(crate::TerminalId),
    Node,
    Files(WorktreeId),
}

/// Durable receipt is distinct from completion; read-only requests have no durable receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub id: RequestId,
    pub durable: bool,
}

fn find_limit() -> usize {
    crate::MAX_FIND_MATCHES
}
