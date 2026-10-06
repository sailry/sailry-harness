use crate::NotificationId;
use crate::WorktreeId;
use serde::{Deserialize, Serialize};

use crate::{
    Credential, CredentialId, Defaults, NodeId, ProjectId, ProviderId, QueuedTurn, RequestId,
    Secret, Session, SessionConfig, SessionId, TurnId,
};

pub(crate) fn discard_payload<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde::de::IgnoredAny::deserialize(deserializer).map(|_| ())
}

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
}

impl Fault {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
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
    PluginTransaction {
        operations: Vec<crate::plugin::transaction::Operation>,
    },
    /// Invoke a declared host export in the captured plugin package.
    CallPlugin {
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
    SearchPluginCatalog {
        source: crate::plugin::catalog::Source,
        query: String,
        page: u32,
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
    DiscoverSkills {
        source: crate::plugin::skills::Source,
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
    SavePluginMcp {
        package: crate::plugin::Reference,
        configuration: crate::plugin::mcp::Configuration,
    },
    /// Controller-only credential editing; any plugin provenance is rejected.
    ReadPluginSecret {
        package: crate::plugin::Reference,
        field: String,
    },
    ReadMcpAuthorization {
        package: crate::plugin::Reference,
        server: String,
    },
    BeginMcpLogin {
        package: crate::plugin::Reference,
        server: String,
        redirect: String,
        client_id: Option<String>,
    },
    CompleteMcpLogin {
        attempt: RequestId,
        callback: Secret,
    },
    CancelMcpLogin {
        attempt: RequestId,
    },
    RevokeMcpAuthorization {
        package: crate::plugin::Reference,
        server: String,
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
    },
    CancelProviderLogin {
        attempt: RequestId,
    },
    ReadCatalogStatus,
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
        cwd: String,
        timeout_ms: u64,
        background: bool,
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
    ListShells,
    SaveTerminalSettings(crate::terminal::Settings),
    ListMediaModels,
    ReadMediaSettings,
    SaveMediaSettings(crate::media::Settings),
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
    UseComputer {
        session: SessionId,
        worktree: WorktreeId,
        name: String,
        arguments: serde_json::Value,
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
    /// Removes an unused, clean linked worktree, retaining its branch.
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
    PreviewOffice {
        worktree: WorktreeId,
        path: String,
    },
    ExportPdf {
        worktree: WorktreeId,
        options: crate::office::Export,
    },
    ReadOffice {
        worktree: WorktreeId,
        options: crate::office::Read,
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
                | Self::DiscoverSkills { .. }
                | Self::SearchPluginCatalog { .. }
                | Self::ReadCatalogPlugin { .. }
                | Self::ReadCatalogPluginInfo { .. }
                | Self::InspectPluginSource { .. }
                | Self::CheckPluginUpdate { .. }
                | Self::UploadPlugin(_)
                | Self::InspectPluginUpload { .. }
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
                | Self::ReadMcpAuthorization { .. }
                | Self::ReadPluginView { .. }
                | Self::ListRoles
                | Self::ListProviders
                | Self::ReadProviderKey { .. }
                | Self::ReadCatalogStatus
                | Self::RefreshModelCatalog
                | Self::ReadModelCatalog(_)
                | Self::DiscoverModels(_)
                | Self::ValidateProvider { .. }
                | Self::ReadConversation { .. }
                | Self::ReadSession { .. }
                | Self::ReadUsage(_)
                | Self::ReadActivity { .. }
                | Self::ReadTurn { .. }
                | Self::SearchConversation { .. }
                | Self::ListConversationAssets { .. }
                | Self::ListFileCheckpoints { .. }
                | Self::ReadFileCheckpoint { .. }
                | Self::ReadTurnDiff { .. }
                | Self::ReadQueuedTurn { .. }
                | Self::ListTerminals { .. }
                | Self::ListTerminalTools { .. }
                | Self::ListCommands { .. }
                | Self::ReadCommand { .. }
                | Self::ReadTerminalSettings
                | Self::ListShells
                | Self::ReadPluginSecret { .. }
                | Self::ListMediaModels
                | Self::ReadMediaSettings
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
                | Self::ReadOffice { .. }
                | Self::PreviewOffice { .. }
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
    pub media_settings: crate::media::Settings,
    pub providers: Vec<crate::conversation::Provider>,
    pub model_catalog: crate::conversation::catalog::Status,
    pub roles: Vec<crate::role::Profile>,
    pub ssh: Vec<crate::ssh::Profile>,
    pub databases: Vec<crate::database::Profile>,
    pub plugins: Vec<crate::plugin::Summary>,
    pub notifications: Vec<crate::notification::Notice>,
    pub turns: Vec<QueuedTurn>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Output {
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
    McpAuthorization(crate::plugin::authorization::Status),
    McpLogin(crate::plugin::authorization::Attempt),
    Plugin(crate::plugin::Info),
    SkillDiscovery(crate::plugin::skills::Discovery),
    PluginCatalog(crate::plugin::catalog::Page),
    PluginSource(crate::plugin::catalog::Selection),
    PluginUpdate(crate::plugin::updates::Report),
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
    Commands(Vec<crate::process::Info>),
    CommandOutput(crate::process::Snapshot),
    Provider(crate::conversation::Provider),
    Providers(Vec<crate::conversation::Provider>),
    ProviderKey(Option<crate::Secret>),
    ProviderLogin(crate::conversation::login::Attempt),
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
    Computer(serde_json::Value),
    ComputerPermissions(crate::computer::Permissions),
    HostProcessSignalled {
        pid: u32,
        started_at_secs: u64,
    },
    Directory(crate::Directory),
    FileContent(crate::FileContent),
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
    OfficeWritten(crate::office::Written),
    OfficePreview(crate::office::Preview),
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
    SessionsRemoved(Vec<SessionId>),
    SessionOrder(Vec<SessionId>),
    QueuedTurn(QueuedTurn),
    /// An unreadable result kind, never a successful execution receipt.
    #[serde(other, deserialize_with = "discard_payload")]
    Unsupported,
}

impl Output {
    /// Whether all nested result kinds are understood by this implementation.
    pub fn supported(&self) -> bool {
        match self {
            Self::Unsupported => false,
            Self::PluginTransaction(outputs) => outputs.iter().all(Self::supported),
            Self::Dispatch(output) => output.supported(),
            Self::RequestOutcome { outcome, .. } => outcome.supported(),
            _ => true,
        }
    }
}

/// A read-only observation of the original durable admission, not a new receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RequestOutcome {
    /// No matching admission exists for this authenticated caller at the time of the read.
    NotAdmitted,
    Admitted,
    Completed(Box<Result<Output, Fault>>),
    /// The Node cannot determine the result and will not replay the side effect.
    Unknown,
}

impl RequestOutcome {
    pub fn supported(&self) -> bool {
        match self {
            Self::Completed(result) => match result.as_ref() {
                Ok(output) => output.supported(),
                Err(_) => true,
            },
            _ => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Event {
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
    ModelCatalogChanged(crate::conversation::catalog::Status),
    ProviderRemoved {
        id: crate::ProviderId,
    },
    ConversationChanged {
        session: SessionId,
        activity: crate::activity::Summary,
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
        backup: Box<Session>,
    },
    TurnQueued {
        turn: QueuedTurn,
        activity: crate::activity::Summary,
    },
    PlanAccepted(Box<crate::conversation::question::AcceptedPlan>),
    /// Unknown history has no state effect and retains only its envelope cursor.
    #[serde(other, deserialize_with = "discard_payload")]
    Unsupported,
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
    Commands {
        node: NodeId,
        session: SessionId,
        items: Vec<crate::process::Info>,
    },
    BrowserCall(crate::browser::Call),
    McpLogin(crate::plugin::authorization::Update),
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
    Commands(SessionId),
    Browser,
    McpLogin(RequestId),
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
