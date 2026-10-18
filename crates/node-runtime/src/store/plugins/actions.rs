//! Software actions retain the ordinary request, admission and business handler.
//! Exact declarations and captured scopes follow Platform 727ce0a callback semantics
//! (Apache-2.0), without its separate action registry or execution lifecycle.
use sailry_protocol::{Command, ErrorCode, Fault, Request, plugin::Action};

use crate::store::{commands, database::Database, sessions};

impl Database {
    pub(in crate::store) fn check_plugin(
        &self,
        caller: sailry_protocol::NodeId,
        request: &Request,
    ) -> Result<(), Fault> {
        if matches!(request.command, Command::PluginTransaction { .. }) {
            return super::transaction::check(self, caller, request);
        }
        let Some(context) = &request.plugin else {
            return Ok(());
        };
        if matches!(request.command, Command::SubmitTask { .. }) {
            return Err(denied("task submission requires a native controller"));
        }
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        if matches!(
            context.surface,
            sailry_protocol::plugin::desktop::Surface::Card(_)
        ) {
            return super::cards::check(&self.connection, self.node, caller, request);
        }
        // Browser provisioning and controller authority follow live package availability,
        // while code and settings retain the admitted package revision.
        if (matches!(
            request.command,
            Command::UseBrowser { .. }
                | Command::UseExternalBrowser { .. }
                | Command::UseComputer { .. }
                | Command::UseMedia { .. }
                | Command::InstallCodeServer { .. }
                | Command::UninstallCodeServer { .. }
                | Command::StopCodeServer { .. }
                | Command::RegisterOcrModel { .. }
                | Command::RegisterKnowledgeModel { .. }
                | Command::InstallOcrModel { .. }
                | Command::DeleteOcrModel { .. }
                | Command::InstallKnowledgeModel { .. }
        ) || matches!(request.command, Command::StartCodeServer { .. })
            && super::super::code::startup_action(self, request)? == Action::ManageCode)
            && !context.controller_settings()
            && !super::required(&self.connection, &context.package.name)?
                .summary
                .enabled
        {
            return Err(Fault::new(ErrorCode::NotConfigured, "plugin is disabled"));
        }
        // Admitted private reads retain their frozen package. New mutations still
        // require live availability, independently of the captured code revision.
        if (context.turn.is_some()
            && matches!(
                request.command,
                Command::WritePluginValue { .. }
                    | Command::WriteIndexedPluginValue { .. }
                    | Command::RemovePluginValue { .. }
                    | Command::ReadProjectCatalog
            )
            || matches!(
                request.command,
                Command::WritePluginConversationValue { .. }
                    | Command::RemovePluginConversationValue { .. }
            ))
            && !super::required(&self.connection, &context.package.name)?
                .summary
                .enabled
        {
            return Err(Fault::new(ErrorCode::NotConfigured, "plugin is disabled"));
        }
        let connection = super::connections::action(&self.connection, context, request)?;
        if connection.is_some()
            && context.turn.is_some()
            && !super::required(&self.connection, &context.package.name)?
                .summary
                .enabled
        {
            return Err(Fault::new(ErrorCode::NotConfigured, "plugin is disabled"));
        }
        let (action, worktree, session) = if let Some(action) = connection {
            (Some(action), None, None)
        } else {
            match &request.command {
                Command::WriteArtifact(_) => (
                    Some(Action::WriteArtifacts),
                    context.worktree,
                    context.session,
                ),
                Command::ListArtifacts { session, .. } => {
                    (Some(Action::ReadArtifacts), None, Some(*session))
                }
                Command::ReadArtifact { id, session, .. }
                | Command::ListArtifactRevisions { id, session, .. } => (
                    Some(Action::ReadArtifacts),
                    None,
                    Some(super::super::artifacts::scope(
                        &self.connection,
                        request,
                        *id,
                        *session,
                    )?),
                ),
                Command::QueryKnowledge(_)
                | Command::ReadKnowledgeCitation(_)
                | Command::ListKnowledgeCollections => (
                    Some(Action::ReadKnowledge),
                    context.worktree,
                    context.session,
                ),
                Command::CreateKnowledgeCollection { .. }
                | Command::UpdateKnowledgeCollection { .. }
                | Command::DeleteKnowledgeCollection { .. }
                | Command::RebuildKnowledgeCollection { .. }
                | Command::PauseKnowledgeCollection { .. } => (
                    Some(Action::WriteKnowledge),
                    context.worktree,
                    context.session,
                ),
                Command::ReadActivityCatalog | Command::ReadActivity { .. }
                    if context.worktree.is_none()
                        && context.session.is_none()
                        && context.turn.is_none()
                        && context.invocation.is_none() =>
                {
                    (Some(Action::ReadActivity), None, None)
                }
                Command::ReadUsage(_)
                    if context.turn.is_none()
                        && context.invocation.is_none()
                        && matches!(
                            context.surface,
                            sailry_protocol::plugin::desktop::Surface::Workspace
                                | sailry_protocol::plugin::desktop::Surface::Settings
                        ) =>
                {
                    (Some(Action::ReadUsage), None, None)
                }
                Command::ListRoles
                    if context.turn.is_some()
                        || (context.turn.is_none()
                            && context.surface
                                == sailry_protocol::plugin::desktop::Surface::Settings) =>
                {
                    (Some(Action::ReadRoles), None, None)
                }
                Command::PutRole { .. } | Command::RemoveRole { .. }
                    if context.turn.is_some()
                        || (context.turn.is_none()
                            && context.surface
                                == sailry_protocol::plugin::desktop::Surface::Settings) =>
                {
                    super::super::roles::approvals::check(&self.connection, request)?;
                    (Some(Action::WriteRoles), None, None)
                }
                Command::SetSessionRoles { session, .. } if context.turn.is_some() => {
                    super::super::roles::approvals::check(&self.connection, request)?;
                    (Some(Action::WriteRoles), context.worktree, Some(*session))
                }
                Command::BeginPluginRegistration { package }
                    if context.controller_settings() && *package == context.package =>
                {
                    super::registration::current(&self.connection, package)?;
                    (None, None, None)
                }
                Command::CancelPluginLogin { .. } if context.controller_settings() => {
                    super::registration::current(&self.connection, &context.package)?;
                    (None, None, None)
                }
                Command::ReadPluginSecret { .. }
                | Command::BeginPluginRegistration { .. }
                | Command::ReadPluginAuthorization { .. }
                | Command::BeginPluginLogin { .. }
                | Command::CompletePluginLogin { .. }
                | Command::CancelPluginLogin { .. }
                | Command::RevokePluginAuthorization { .. }
                | Command::ReadPluginMcp { .. }
                | Command::SavePluginMcp { .. } => {
                    return Err(denied(
                        "credential contents are not exposed to plugin actions",
                    ));
                }
                Command::UseMedia {
                    turn,
                    session,
                    worktree,
                    action,
                } => {
                    if context.turn != Some(*turn) {
                        return Err(denied("media operation is outside the captured turn"));
                    }
                    (
                        Some(match action.kind() {
                            sailry_protocol::media::Kind::Vision => Action::InspectMedia,
                            sailry_protocol::media::Kind::Image => Action::GenerateImage,
                            sailry_protocol::media::Kind::Video => Action::GenerateVideo,
                        }),
                        Some(*worktree),
                        Some(*session),
                    )
                }
                Command::ReadMediaSettings | Command::ListMediaModels
                    if context.surface == sailry_protocol::plugin::desktop::Surface::Settings =>
                {
                    (Some(Action::ReadMediaSettings), None, None)
                }
                Command::SaveMediaSettings(_)
                    if context.surface == sailry_protocol::plugin::desktop::Surface::Settings =>
                {
                    (Some(Action::SaveMediaSettings), None, None)
                }
                Command::UseComputer {
                    session,
                    worktree,
                    name,
                    ..
                } => (
                    Some(
                        if crate::computer::read_only(name)
                            .ok_or_else(|| Fault::invalid("unknown computer tool"))?
                        {
                            Action::ReadComputer
                        } else {
                            Action::ControlComputer
                        },
                    ),
                    Some(*worktree),
                    Some(*session),
                ),
                Command::ReadComputerPermissions => (Some(Action::ReadComputer), None, None),
                Command::RequestComputerPermission { .. } => {
                    (Some(Action::ControlComputer), None, None)
                }
                Command::UseBrowser {
                    session,
                    worktree,
                    action,
                } => (
                    Some(if action.requires_approval() {
                        Action::ControlBrowser
                    } else {
                        Action::ReadBrowser
                    }),
                    Some(*worktree),
                    Some(*session),
                ),
                Command::ReadWorktreeCatalog { worktree } => {
                    (Some(Action::ReadWorktrees), Some(*worktree), None)
                }
                Command::ReadProjectCatalog
                    if context.worktree.is_none()
                        && context.session.is_none()
                        && context.turn.is_none() =>
                {
                    (Some(Action::ReadProjects), None, None)
                }
                Command::StartSession(draft)
                    if context.turn.is_none() && context.session.is_none() =>
                {
                    if context.worktree.is_some() && context.worktree != draft.worktree {
                        return Err(denied("session creation is outside the captured worktree"));
                    }
                    // Independent assistants and connections use their explicit resource API.
                    if draft.config.as_ref().is_some_and(|config| {
                        config.assistant.is_some() || config.resource.is_some()
                    }) {
                        return Err(denied(
                            "session creation requires a project or unassigned scope",
                        ));
                    }
                    (Some(Action::StartSessions), None, None)
                }
                Command::CallPlugin { .. } | Command::ReadPluginCallback { .. }
                    if context.invocation.is_none() =>
                {
                    (None, None, None)
                }
                Command::CancelPluginCall { .. } if context.invocation.is_none() => {
                    (None, None, None)
                }
                Command::PublishNotification { package, content }
                    if *package == context.package =>
                {
                    (Some(Action::Notify), None, content.session)
                }
                Command::Dispatch { package, action } if *package == context.package => {
                    if let sailry_protocol::dispatch::Command::SaveHandler(handler) = action {
                        (
                            Some(Action::Dispatch),
                            handler.callback.scope.worktree,
                            handler.callback.scope.session,
                        )
                    } else {
                        (Some(Action::Dispatch), None, None)
                    }
                }
                Command::StopDispatchTurn { package, .. }
                    if *package == context.package
                        && context.worktree.is_none()
                        && context.session.is_none()
                        && context.turn.is_none() =>
                {
                    (Some(Action::Dispatch), None, None)
                }
                Command::ReadPluginValue { .. }
                | Command::ListPluginKeys { .. }
                | Command::SearchPluginValues(_) => (Some(Action::ReadStorage), None, None),
                Command::ReadPluginConversationValue { .. } => (
                    Some(Action::ReadStorage),
                    None,
                    Some(
                        context
                            .session
                            .ok_or_else(|| denied("plugin has no session scope"))?,
                    ),
                ),
                Command::WritePluginValue { .. }
                | Command::WriteIndexedPluginValue { .. }
                | Command::RemovePluginValue { .. } => (Some(Action::WriteStorage), None, None),
                Command::WritePluginConversationValue { .. }
                | Command::RemovePluginConversationValue { .. } => (
                    Some(Action::WriteStorage),
                    None,
                    Some(
                        context
                            .session
                            .ok_or_else(|| denied("plugin has no session scope"))?,
                    ),
                ),
                Command::GeneratePluginText { .. } | Command::ListPluginModels => {
                    (Some(Action::GenerateText), None, None)
                }
                Command::ResolvePluginModel { .. } => (Some(Action::ReadModels), None, None),
                Command::SearchModels(_)
                | Command::ReadModelVersions { .. }
                | Command::ResolveModel { .. }
                | Command::RefreshOcrModel { .. }
                | Command::ReadOcrModels
                | Command::ReadKnowledgeModels => (Some(Action::ReadModels), None, None),
                Command::RegisterOcrModel { .. }
                | Command::RegisterKnowledgeModel { .. }
                | Command::InstallOcrModel { .. }
                | Command::DeleteOcrModel { .. }
                | Command::InstallKnowledgeModel { .. } => {
                    if matches!(
                        request.command,
                        Command::InstallOcrModel { .. } | Command::DeleteOcrModel { .. }
                    ) {
                        super::super::ocr::approvals::check(&self.connection, request)?;
                    }
                    (Some(Action::InstallModels), None, None)
                }
                Command::ReadInstallations { .. } | Command::DismissInstallation { .. } => {
                    // Exact captured request/category visibility is checked by the Registry.
                    (None, None, None)
                }
                Command::RequestPluginHttp(_) => (Some(Action::Http), None, None),
                Command::FetchWeb(_)
                | Command::CancelWebFetch { .. }
                | Command::SearchWeb(_)
                | Command::CancelWebSearch { .. } => (Some(Action::ReadWeb), None, None),
                Command::ReadCodeSettings | Command::ReadCodeCatalog | Command::ReadCodeRules
                    if context.controller_settings() =>
                {
                    (Some(Action::ReadCodeSettings), None, None)
                }
                Command::SaveCodeSettings { .. }
                | Command::UpdateCodeRules
                | Command::CancelCodeInstall { .. }
                    if context.controller_settings() =>
                {
                    (Some(Action::SaveCodeSettings), None, None)
                }
                Command::InstallCodeServer { .. } | Command::UninstallCodeServer { .. }
                    if context.controller_settings() =>
                {
                    (Some(Action::ManageCode), None, None)
                }
                Command::ReadSearchSettings if context.controller_settings() => {
                    (Some(Action::ReadSearchSettings), None, None)
                }
                Command::SaveSearchSettings(_) | Command::TestSearchDriver { .. }
                    if context.controller_settings() =>
                {
                    (Some(Action::SaveSearchSettings), None, None)
                }
                Command::ListCodeInstallations => (Some(Action::ReadCode), None, None),
                Command::ReadCodeCatalog
                    if context.turn.is_some() && context.invocation.is_none() =>
                {
                    (Some(Action::ReadCode), context.worktree, context.session)
                }
                Command::ReadCode { worktree, .. }
                | Command::ListCodeServers { worktree }
                | Command::ListCodePackages { worktree } => {
                    (Some(Action::ReadCode), Some(*worktree), None)
                }
                Command::InstallCodeServer { .. } | Command::UninstallCodeServer { .. }
                    if context.turn.is_some() && context.invocation.is_none() =>
                {
                    (Some(Action::ManageCode), context.worktree, context.session)
                }
                Command::StartCodeServer { startup } if context.turn.is_some() => (
                    Some(super::super::code::startup_action(self, request)?),
                    Some(startup.worktree),
                    context.session,
                ),
                Command::StopCodeServer { startup }
                    if context.turn.is_some() && context.invocation.is_none() =>
                {
                    (
                        Some(Action::ManageCode),
                        Some(startup.worktree),
                        context.session,
                    )
                }
                Command::ListDirectory { worktree, .. }
                | Command::ReadFile { worktree, .. }
                | Command::ReadFileLines { worktree, .. }
                | Command::ReadNotebook { worktree, .. }
                | Command::FindFiles { worktree, .. }
                | Command::DownloadFile { worktree, .. }
                | Command::SearchFiles { worktree, .. } => {
                    (Some(Action::ReadFiles), Some(*worktree), None)
                }
                Command::ReadOffice { worktree, .. } => {
                    (Some(Action::ReadFiles), Some(*worktree), None)
                }
                Command::UploadFile(spec) => (Some(Action::WriteFiles), Some(spec.worktree), None),
                Command::WriteFile { worktree, .. }
                | Command::EditFile { worktree, .. }
                | Command::EditNotebook { worktree, .. }
                | Command::DeleteFile { worktree, .. }
                | Command::MoveFile { worktree, .. }
                | Command::FinishFileUpload { worktree, .. }
                | Command::CreateDirectory { worktree, .. }
                | Command::RenameEntry { worktree, .. }
                | Command::CopyEntry { worktree, .. }
                | Command::TrashEntry { worktree, .. }
                | Command::TrashFile { worktree, .. } => {
                    (Some(Action::WriteFiles), Some(*worktree), None)
                }
                Command::CopyEntryTo {
                    source, worktree, ..
                }
                | Command::MoveEntryTo {
                    source, worktree, ..
                } => {
                    if Some(*source) != context.worktree {
                        return Err(denied("file source is outside the captured worktree"));
                    }
                    (Some(Action::WriteFiles), Some(*worktree), None)
                }
                Command::UseExternalBrowser {
                    session,
                    worktree,
                    action,
                    ..
                } => (
                    Some(if action.read_only() {
                        Action::ReadExternalBrowser
                    } else {
                        Action::ControlExternalBrowser
                    }),
                    Some(*worktree),
                    Some(*session),
                ),
                Command::RunCommand { turn, .. } => {
                    let session = context
                        .session
                        .ok_or_else(|| denied("plugin has no session scope"))?;
                    if context.turn.is_some_and(|captured| captured != *turn) {
                        return Err(denied("command is outside the captured turn"));
                    }
                    let run = super::super::agent::visible_run(&self.connection, session, *turn)?;
                    if run.origin.is_some() {
                        return Err(denied("plugin turn belongs to another session"));
                    }
                    (Some(Action::ControlCommands), None, Some(session))
                }
                Command::ListCommands { session } | Command::ReadCommand { session, .. } => {
                    (Some(Action::ReadCommands), None, Some(*session))
                }
                Command::StopCommand { session, .. } | Command::WriteCommand { session, .. } => {
                    (Some(Action::ControlCommands), None, Some(*session))
                }
                Command::ListTerminals { worktree } | Command::ListTerminalTools { worktree } => {
                    (Some(Action::ReadTerminals), Some(*worktree), None)
                }
                Command::CreateTerminal(launch) | Command::OpenToolTerminal { launch, .. } => {
                    if launch.access == sailry_protocol::process::Access::Unrestricted
                        && !desktop(context)
                    {
                        return Err(denied("plugins cannot create unrestricted terminals"));
                    }
                    (Some(Action::ControlTerminals), Some(launch.worktree), None)
                }
                Command::CloseTerminal { worktree, terminal } => {
                    let info = super::super::terminals::required(&self.connection, *terminal)?;
                    if info.worktree != Some(*worktree) {
                        return Err(denied("terminal belongs to another worktree"));
                    }
                    (Some(Action::ControlTerminals), Some(*worktree), None)
                }
                Command::OpenTerminal { terminal, .. }
                | Command::InspectTerminal { terminal }
                | Command::ClaimTerminal { terminal, .. }
                | Command::InputTerminal { terminal, .. }
                | Command::ResizeTerminal { terminal, .. }
                | Command::SetTerminalAppearance { terminal, .. } => {
                    let info = super::super::terminals::required(&self.connection, *terminal)?;
                    if info.access == Some(sailry_protocol::process::Access::Unrestricted)
                        && (matches!(request.command, Command::InputTerminal { .. })
                            || matches!(request.command, Command::OpenTerminal { .. })
                                && !desktop(context))
                    {
                        return Err(denied("plugins cannot execute in unrestricted terminals"));
                    }
                    let worktree = info
                        .worktree
                        .ok_or_else(|| denied("terminal has no worktree scope"))?;
                    let action = if matches!(request.command, Command::InspectTerminal { .. }) {
                        Action::ReadTerminals
                    } else {
                        Action::ControlTerminals
                    };
                    (Some(action), Some(worktree), None)
                }
                Command::InspectGit { worktree }
                | Command::ReadGitLog { worktree, .. }
                | Command::ReadGitShow { worktree, .. }
                | Command::ReadGitBlame { worktree, .. }
                | Command::ListGitChanges { worktree, .. }
                | Command::ReadGitChange { worktree, .. }
                | Command::ReadGitDiff { worktree, .. } => (
                    Some(Action::ReadGit),
                    Some(super::repository::read_scope(
                        &self.connection,
                        context,
                        *worktree,
                    )?),
                    None,
                ),
                Command::ListGitBranches { worktree }
                | Command::ResolveGitRevision { worktree, .. }
                | Command::ReadGitCommit { worktree, .. }
                | Command::ReadGitStash { worktree, .. }
                | Command::ReadGitOutput { worktree }
                | Command::ListGitRemoteTags { worktree, .. } => {
                    (Some(Action::ReadGit), Some(*worktree), None)
                }
                Command::RunGitAction {
                    worktree,
                    action: sailry_protocol::GitAction::PruneWorktree { .. },
                    ..
                } => (Some(Action::WriteWorktrees), Some(*worktree), None),
                Command::UpdateGitIndex { worktree, .. }
                | Command::CreateGitCommit { worktree, .. }
                | Command::CreateGitBranch { worktree, .. }
                | Command::RenameGitBranch { worktree, .. }
                | Command::DeleteGitBranch { worktree, .. }
                | Command::SwitchGitBranch { worktree, .. }
                | Command::MergeGitBranch { worktree, .. }
                | Command::RunGitAction { worktree, .. } => {
                    (Some(Action::WriteGit), Some(*worktree), None)
                }
                Command::ListWorktrees { .. }
                | Command::RegisterWorktree { .. }
                | Command::CreateWorktree { .. }
                | Command::CreateManagedWorktree { .. }
                | Command::RemoveWorktree { .. } => (
                    Some(super::repository::action(
                        &self.connection,
                        context,
                        &request.command,
                    )?),
                    context.worktree,
                    None,
                ),
                Command::ReadSession { session }
                | Command::ReadConversation { session, .. }
                | Command::ReadTurn { session, .. }
                | Command::SearchConversation { session, .. }
                | Command::ListConversationAssets { session, .. } => {
                    (Some(Action::ReadConversation), None, Some(*session))
                }
                Command::GuideTurn { recipient, .. } => {
                    let parent = context
                        .turn
                        .ok_or_else(|| denied("child guidance requires a parent turn"))?;
                    let run = crate::store::agent::runs::get(&self.connection, *recipient)?;
                    let owned = crate::store::agent::delegation::owned(
                        &self.connection,
                        parent,
                        run.session,
                    )?;
                    if owned.turn != *recipient {
                        return Err(denied("child guidance recipient is no longer current"));
                    }
                    (
                        Some(Action::DelegateAgents),
                        context.worktree,
                        context.session,
                    )
                }
                Command::QueueTurn { session, .. }
                | Command::SubmitTurn { session, .. }
                | Command::ContinueTurn { session, .. }
                | Command::SetSessionConfig { session, .. }
                | Command::SetQueuePaused { session, .. }
                | Command::MoveQueuedTurn { session, .. } => {
                    (Some(Action::ControlConversation), None, Some(*session))
                }
                Command::StopTurn { turn }
                | Command::StartQueuedTurn { turn }
                | Command::EditQueuedTurn { turn, .. }
                | Command::RemoveQueuedTurn { turn, .. }
                | Command::SendQueuedTurn { turn, .. }
                | Command::ReadQueuedTurn { turn } => {
                    let session = context
                        .session
                        .ok_or_else(|| denied("plugin has no session scope"))?;
                    let run = super::super::agent::visible_run(&self.connection, session, *turn)?;
                    if run.origin.is_some() {
                        return Err(denied("plugin turn belongs to another session"));
                    }
                    let action = if matches!(request.command, Command::ReadQueuedTurn { .. }) {
                        Action::ReadConversation
                    } else {
                        Action::ControlConversation
                    };
                    (Some(action), None, Some(session))
                }
                // Reading a plugin's own public settings is intrinsic, not another grant.
                Command::ReadPluginSettings { package } if *package == context.package => {
                    (None, None, None)
                }
                Command::ReadMcpAuthorization { package, server }
                    if *package == context.package
                        && context.turn.is_some()
                        && context.invocation.is_none() =>
                {
                    let package = self.plugin_package(caller, request)?;
                    if !package.mcp.iter().any(|entry| {
                        entry.name == *server
                            && entry.transport != sailry_protocol::plugin::McpTransport::Stdio
                    }) {
                        return Err(denied("MCP status requires an owned HTTP server"));
                    }
                    (None, None, None)
                }
                Command::ReadPluginConnection { package, card }
                    if *package == context.package
                        && (context.turn.is_some() || context.invocation.is_some()) =>
                {
                    let package = self.plugin_package(caller, request)?;
                    let declaration = package
                        .extension
                        .as_ref()
                        .and_then(|extension| {
                            extension.cards.iter().find(|entry| entry.key == *card)
                        })
                        .ok_or_else(|| denied("connection status requires an owned card"))?;
                    let server = declaration
                        .mcp_server
                        .as_deref()
                        .ok_or_else(|| denied("connection status requires an owned HTTP server"))?;
                    if !package.mcp.iter().any(|entry| {
                        entry.name == server
                            && entry.transport != sailry_protocol::plugin::McpTransport::Stdio
                    }) {
                        return Err(denied("connection status requires an owned HTTP server"));
                    }
                    (None, None, None)
                }
                Command::SavePluginSettings { package, .. }
                    if *package == context.package
                        && context.surface
                            == sailry_protocol::plugin::desktop::Surface::Settings =>
                {
                    (None, None, None)
                }
                _ => return Err(denied("command is not exposed to plugin software actions")),
            }
        };
        if worktree.is_some_and(|worktree| Some(worktree) != context.worktree)
            || session.is_some_and(|session| context.session != Some(session))
        {
            return Err(denied(
                "plugin action is outside the captured resource scope",
            ));
        }
        if let Some(worktree) = context.worktree {
            self.worktree_root(worktree)?;
        }
        if let Some(session) = context.session {
            let session = commands::session(&self.connection, session)?;
            if Some(session.worktree) != context.worktree {
                return Err(denied("plugin session belongs to another worktree"));
            }
            if context.turn.is_none()
                && matches!(
                    action,
                    Some(Action::WriteFiles | Action::WriteGit | Action::WriteWorktrees)
                )
            {
                sessions::writable(&session)?;
            }
        }
        let package = self.plugin_package(caller, request)?;
        if let Command::UseComputer { name, .. } = &request.command {
            crate::store::agent::plugins::check_computer(
                &self.connection,
                context,
                &package,
                name,
            )?;
        }
        if let Some(turn) = context.turn {
            crate::store::agent::plugins::check_action(
                &self.connection,
                turn,
                &request.command,
                action.as_ref(),
            )?;
        }
        let settings_surface =
            context.surface == sailry_protocol::plugin::desktop::Surface::Settings;
        if settings_surface
            && !package
                .extension
                .as_ref()
                .is_some_and(|extension| extension.settings_page.is_some())
        {
            return Err(denied("plugin has no settings contribution"));
        }
        let settings_access = settings_surface
            && matches!(
                request.command,
                Command::ListRoles
                    | Command::ReadProjectCatalog
                    | Command::PutRole { .. }
                    | Command::RemoveRole { .. }
                    | Command::ListPluginModels
                    | Command::SearchModels(_)
                    | Command::ReadModelVersions { .. }
                    | Command::ResolveModel { .. }
                    | Command::RefreshOcrModel { .. }
                    | Command::ReadOcrModels
                    | Command::ReadKnowledgeModels
                    | Command::RegisterOcrModel { .. }
                    | Command::RegisterKnowledgeModel { .. }
                    | Command::InstallOcrModel { .. }
                    | Command::InstallKnowledgeModel { .. }
                    | Command::ReadInstallations { .. }
                    | Command::DismissInstallation { .. }
                    | Command::ReadPluginSettings { .. }
                    | Command::ReadPluginValue { .. }
                    | Command::ListPluginKeys { .. }
                    | Command::SearchPluginValues(_)
                    | Command::WritePluginValue { .. }
                    | Command::WriteIndexedPluginValue { .. }
                    | Command::RemovePluginValue { .. }
                    | Command::PluginTransaction { .. }
                    | Command::SavePluginSettings { .. }
                    | Command::ReadCodeSettings
                    | Command::ReadCodeCatalog
                    | Command::ReadCodeRules
                    | Command::ListCodeInstallations
                    | Command::SaveCodeSettings { .. }
                    | Command::UpdateCodeRules
                    | Command::CancelCodeInstall { .. }
                    | Command::InstallCodeServer { .. }
                    | Command::UninstallCodeServer { .. }
                    | Command::ReadSearchSettings
                    | Command::SaveSearchSettings(_)
                    | Command::TestSearchDriver { .. }
                    | Command::ReadMediaSettings
                    | Command::ListMediaModels
                    | Command::SaveMediaSettings(_)
                    | Command::ReadComputerPermissions
                    | Command::RequestComputerPermission { .. }
            );
        if !package.summary.enabled && !settings_access {
            return Err(Fault::new(ErrorCode::NotConfigured, "plugin is disabled"));
        }
        if package.summary.reference() != context.package {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "plugin package changed",
            ));
        }
        if !package.extension.as_ref().is_some_and(|extension| {
            let conversation = match request.command {
                Command::ReadPluginConversationValue { .. } => Some(Action::ReadConversation),
                Command::WritePluginConversationValue { .. }
                | Command::RemovePluginConversationValue { .. } => {
                    Some(Action::ControlConversation)
                }
                _ => None,
            };
            if conversation.is_some_and(|action| !extension.actions.contains(&action)) {
                return false;
            }
            if matches!(request.command, Command::ListPluginModels) {
                extension.actions.contains(&Action::GenerateText)
                    || extension.actions.contains(&Action::ReadModels)
            } else if matches!(
                request.command,
                Command::ReadInstallations { .. } | Command::DismissInstallation { .. }
            ) {
                extension.actions.contains(&Action::InstallModels)
                    || extension.actions.contains(&Action::ReadCode)
                    || extension.actions.contains(&Action::ManageCode)
            } else {
                action.is_none_or(|action| extension.actions.contains(&action))
            }
        }) {
            return Err(denied("plugin does not declare this software action"));
        }
        Ok(())
    }

    pub(in crate::store) fn plugin_package(
        &self,
        caller: sailry_protocol::NodeId,
        request: &Request,
    ) -> Result<sailry_protocol::plugin::Info, Fault> {
        let context = request
            .plugin
            .as_ref()
            .ok_or_else(|| denied("plugin provenance is required"))?;
        if let Command::CancelPluginCall { request } = &request.command {
            let mut running = context.clone();
            running.invocation = Some(*request);
            super::invocations::package(&self.connection, caller, &running)
        } else if context.invocation.is_some() {
            super::invocations::package(&self.connection, caller, context)
        } else if context.turn.is_some() {
            crate::store::agent::plugin_package(&self.connection, context)
        } else if let Some(info) =
            crate::store::dispatch::jobs::package(&self.connection, self.node, caller, request)?
        {
            Ok(info)
        } else {
            super::required(&self.connection, &context.package.name)
        }
    }
}

fn desktop(context: &sailry_protocol::plugin::Context) -> bool {
    use sailry_protocol::plugin::desktop::Surface;

    context.turn.is_none()
        && context.invocation.is_none()
        && matches!(context.surface, Surface::Workspace | Surface::Project)
}

fn denied(message: &str) -> Fault {
    Fault::new(ErrorCode::PermissionDenied, message)
}
