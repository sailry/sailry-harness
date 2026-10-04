use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::*;

use super::database::{encode, storage_error};

pub(super) fn execute(
    db: &Connection,
    node: NodeId,
    caller: NodeId,
    profile: Option<&std::path::Path>,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    match &request.command {
        Command::StartSession(draft) => {
            super::sessions::start(db, node, caller, profile, request, draft)
        }
        Command::ReadProjectCatalog => {
            let projects = super::projects::list(db)?;
            let worktrees = super::worktrees::list(db)?
                .into_iter()
                .filter(|tree| {
                    projects
                        .iter()
                        .any(|project| Some(project.id) == tree.project)
                })
                .collect();
            Ok((
                Output::ProjectCatalog(projects::Catalog {
                    projects,
                    worktrees,
                }),
                None,
            ))
        }
        Command::ReadWorktreeCatalog { worktree } => {
            let entries = super::worktrees::list(db)?;
            let source = entries
                .iter()
                .find(|entry| entry.id == *worktree)
                .ok_or_else(|| {
                    Fault::new(ErrorCode::NotFound, "captured worktree is unavailable")
                })?;
            let projects = super::projects::list(db)?
                .into_iter()
                .filter(|project| Some(project.id) == source.project)
                .collect();
            let project = source.project;
            let worktrees = entries
                .into_iter()
                .filter(|entry| {
                    entry.id == *worktree || project.is_some() && entry.project == project
                })
                .collect();
            Ok((
                Output::ProjectCatalog(projects::Catalog {
                    projects,
                    worktrees,
                }),
                None,
            ))
        }
        Command::PublishNotification { .. }
        | Command::MarkNotificationRead { .. }
        | Command::DismissNotifications { .. } => {
            let admitted = request
                .plugin
                .as_ref()
                .filter(|context| context.invocation.is_some())
                .map(|context| super::plugins::invocations::package(db, caller, context))
                .transpose()?;
            super::notifications::execute(db, &request.command, admitted)
        }
        Command::Dispatch { package, action } => {
            if let Some(context) = request
                .plugin
                .as_ref()
                .filter(|context| context.invocation.is_some())
            {
                super::plugins::invocations::package(db, caller, context)?;
                super::dispatch::execute_admitted(db, package, action)
            } else {
                super::dispatch::execute(db, package, action)
            }
        }
        Command::UpdateProject {
            expected,
            name,
            path,
            appearance,
        } => super::projects::update(db, expected, name, path, appearance),
        Command::RemoveProject { expected } => super::projects::remove(db, expected),
        Command::BrowseFiles { .. } | Command::ManageFiles(_) | Command::CreateProject(_) => {
            Err(Fault::new(
                ErrorCode::Internal,
                "host file operations must run outside the database",
            ))
        }
        Command::ListMediaModels => Ok((Output::MediaModels(super::media::catalog(db)?), None)),
        Command::ReadMediaSettings => Ok((Output::MediaSettings(super::media::read(db)?), None)),
        Command::SaveMediaSettings(settings) => super::media::save(db, settings),
        Command::ReadTerminalSettings => Ok((
            Output::TerminalSettings(super::terminals::settings::read(db)?),
            None,
        )),
        Command::SaveTerminalSettings(settings) => super::terminals::settings::save(db, settings),
        Command::ListDatabases if request.plugin.is_some() => {
            let context = request.plugin.as_ref().expect("checked plugin context");
            let profiles = super::databases::list(db)?
                .into_iter()
                .filter(|profile| {
                    super::plugins::connections::check_target(
                        db,
                        context,
                        connection::Resource::Database(profile.id),
                    )
                    .is_ok()
                })
                .collect();
            Ok((Output::DatabaseProfiles(profiles), None))
        }
        Command::ListSsh if request.plugin.is_some() => {
            let context = request.plugin.as_ref().expect("checked plugin context");
            let profiles = super::ssh::list(db)?
                .into_iter()
                .filter(|profile| {
                    super::plugins::connections::check_target(
                        db,
                        context,
                        connection::Resource::Ssh(profile.id),
                    )
                    .is_ok()
                })
                .collect();
            Ok((Output::SshProfiles(profiles), None))
        }
        Command::ListSshTerminals { profile } => Ok((
            Output::Terminals(
                super::terminals::list(db)?
                    .into_iter()
                    .filter(|info| {
                        info.ssh == Some(*profile)
                            && info.owner == Some(caller)
                            && info.status != terminal::Status::Closed
                    })
                    .collect(),
            ),
            None,
        )),
        Command::ListDatabases | Command::SaveDatabase { .. } | Command::RemoveDatabase { .. } => {
            super::databases::execute(db, &request.command)
        }
        Command::ListSsh
        | Command::SaveSsh { .. }
        | Command::RemoveSsh { .. }
        | Command::TrustSsh { .. } => super::ssh::execute(db, &request.command),
        Command::ReadAttachment {
            worktree,
            attachment,
        } => Ok((
            Output::Attachment(super::attachments::read(db, *worktree, *attachment)?),
            None,
        )),
        Command::DiscardAttachment {
            worktree,
            attachment,
        } => {
            super::attachments::discard(db, caller, *worktree, *attachment)?;
            Ok((
                Output::AttachmentDiscarded {
                    attachment: *attachment,
                },
                None,
            ))
        }
        Command::ReadPluginValue { .. }
        | Command::ReadPluginConversationValue { .. }
        | Command::ListPluginKeys { .. }
        | Command::WritePluginValue { .. }
        | Command::WriteIndexedPluginValue { .. }
        | Command::SearchPluginValues(_)
        | Command::RemovePluginValue { .. }
        | Command::WritePluginConversationValue { .. }
        | Command::RemovePluginConversationValue { .. } => {
            super::plugins::execute_storage(db, request)
        }
        Command::ReadPluginSettings { .. }
            if request
                .plugin
                .as_ref()
                .is_some_and(|context| context.invocation.is_some() || context.turn.is_some()) =>
        {
            let context = request.plugin.as_ref().expect("checked plugin context");
            let info = if context.invocation.is_some() {
                super::plugins::invocations::package(db, caller, context)?
            } else {
                super::agent::plugin_package(db, context)?
            };
            Ok((
                Output::PluginSettings(super::plugins::settings::read_admitted(db, &info)?),
                None,
            ))
        }
        Command::ListPlugins
        | Command::ReadMcpAuthorization { .. }
        | Command::RevokeMcpAuthorization { .. }
        | Command::ReadPluginSettings { .. }
        | Command::ReadPluginSecret { .. }
        | Command::ListPluginModels
        | Command::ResolvePluginModel { .. }
        | Command::SavePluginSettings { .. }
        | Command::ReadPlugin { .. }
        | Command::ReadPluginVersion { .. }
        | Command::SetPluginEnabled { .. }
        | Command::RemovePlugin { .. } => super::plugins::execute(db, &request.command),
        Command::ListRoles => Ok((Output::Roles(super::roles::list(db)?), None)),
        Command::PutRole {
            role,
            expected_revision,
        } => {
            let role = super::roles::put(db, role, *expected_revision)?;
            Ok((Output::Role(role.clone()), Some(Event::RoleChanged(role))))
        }
        Command::RemoveRole {
            role,
            expected_revision,
        } => {
            super::roles::remove(db, *role, *expected_revision)?;
            Ok((
                Output::Roles(super::roles::list(db)?),
                Some(Event::RoleRemoved { id: *role }),
            ))
        }
        Command::ResolveQuestion {
            session,
            question,
            response,
        } => {
            if matches!(
                response,
                conversation::question::Response::StartCoding { .. }
            ) {
                let accepted = Box::new(super::agent::questions::accept(db, caller, request)?);
                return Ok((
                    Output::PlanAccepted(accepted.clone()),
                    Some(Event::PlanAccepted(accepted)),
                ));
            }
            let question = super::agent::questions::resolve(db, *session, *question, response)?;
            Ok((
                Output::Question(question),
                Some(super::agent::activity::changed(db, *session)?),
            ))
        }
        Command::ResolveApproval {
            session,
            approval,
            decision,
        } => {
            let approval = super::agent::approvals::resolve(db, *session, *approval, *decision)?;
            Ok((
                Output::Approval(approval),
                Some(super::agent::activity::changed(db, *session)?),
            ))
        }
        Command::ListProviders => Ok((Output::Providers(super::agent::providers(db)?), None)),
        Command::ReadProviderKey {
            provider,
            expected_revision,
        } => Ok((
            Output::ProviderKey(super::providers::authentication::provider_key(
                db,
                node,
                *provider,
                *expected_revision,
            )?),
            None,
        )),
        Command::ReadCatalogStatus => Ok((
            Output::CatalogStatus(super::providers::catalog::status(db)?),
            None,
        )),
        Command::ReadModelCatalog(query) => Ok((
            Output::ModelCatalog(super::providers::catalog::page(db, query)?),
            None,
        )),
        Command::PutProvider {
            provider,
            expected_revision,
        } => {
            let provider = super::agent::put_provider(db, node, provider, *expected_revision)?;
            Ok((
                Output::Provider(provider.clone()),
                Some(Event::ProviderChanged(provider)),
            ))
        }
        Command::SaveProvider {
            provider,
            expected_revision,
            secret,
        } => {
            let mut provider = provider.clone();
            if let Some(secret) = secret {
                if provider.authentication != Authentication::ApiKey {
                    return Err(invalid("stored secrets require API-key authentication"));
                }
                let id = CredentialId::new();
                super::providers::authentication::execute(
                    db,
                    &Command::PutCredential {
                        id,
                        provider: provider.id,
                        expected_revision: 0,
                        secret: secret.clone(),
                        expires_at_ms: None,
                    },
                )?;
                provider.credential = Some(CredentialRef { node, id });
            }
            let provider = super::agent::put_provider(db, node, &provider, *expected_revision)?;
            Ok((
                Output::Provider(provider.clone()),
                Some(Event::ProviderChanged(provider)),
            ))
        }
        Command::RemoveProvider {
            provider,
            expected_revision,
        } => {
            super::agent::remove_provider(db, *provider, *expected_revision)?;
            Ok((
                Output::Providers(super::agent::providers(db)?),
                Some(Event::ProviderRemoved { id: *provider }),
            ))
        }
        Command::ReadUsage(query) => Ok((
            Output::Usage(super::agent::statistics::report::read(db, node, query)?),
            None,
        )),
        Command::ReadActivityCatalog => Ok((
            Output::ActivityCatalog(super::agent::activity::catalog(db, node)?),
            None,
        )),
        Command::ReadActivity { sessions } => Ok((
            Output::Activity(super::agent::activity::previews(db, sessions)?),
            None,
        )),
        Command::ReadSession { session: id } => Ok((Output::Session(session(db, *id)?), None)),
        Command::ReadConversation {
            session,
            before,
            limit,
        } => Ok((
            Output::Conversation(super::agent::read(db, *session, *before, *limit)?),
            None,
        )),
        Command::ReadTurn {
            session,
            turn,
            expected_revision,
            before,
            limit,
        } => {
            check_revision(
                super::sessions::history_revision(db, *session)?,
                *expected_revision,
            )?;
            Ok((
                Output::TurnHistory(super::agent::read_turn(
                    db, *session, *turn, *before, *limit,
                )?),
                None,
            ))
        }
        Command::ListConversationAssets { session, query } => Ok((
            Output::ConversationAssets(super::agent::assets::read(db, *session, query)?),
            None,
        )),
        Command::SearchConversation { session, query } => Ok((
            Output::ConversationMatches(super::agent::search(db, *session, query)?),
            None,
        )),
        Command::ListFileCheckpoints {
            session,
            turn,
            before,
            limit,
        } => Ok((
            Output::FileCheckpoints(super::agent::checkpoints::list(
                db, *session, *turn, *before, *limit,
            )?),
            None,
        )),
        Command::ReadTurnDiff { session, turn } => Ok((
            Output::TurnDiff(super::agent::checkpoints::diff(db, *session, *turn)?),
            None,
        )),
        Command::ReadFileCheckpoint {
            session,
            checkpoint,
        } => Ok((
            Output::FileCheckpoint(Box::new(super::agent::checkpoints::read(
                db,
                *session,
                *checkpoint,
            )?)),
            None,
        )),
        Command::ForkConversation {
            session,
            through,
            expected_revision,
        } => super::sessions::fork(db, *session, *through, *expected_revision),
        Command::RewindConversation {
            session,
            through,
            expected_head,
            expected_revision,
        } => super::sessions::rewind(db, *session, *through, *expected_head, *expected_revision),
        Command::ReplaceTurn { .. } => super::sessions::replace(db, caller, request),
        Command::StartQueuedTurn { turn } => {
            let run = super::agent::start(db, *turn)?;
            Ok((
                Output::Run(run.clone()),
                Some(super::agent::activity::changed(db, run.session)?),
            ))
        }
        Command::ReadQueuedTurn { .. }
        | Command::EditQueuedTurn { .. }
        | Command::RemoveQueuedTurn { .. }
        | Command::SendQueuedTurn { .. }
        | Command::MoveQueuedTurn { .. }
        | Command::SetQueuePaused { .. } => super::agent::queue::execute(db, caller, request),
        Command::StopDispatchTurn { package, job } => {
            if let Some(context) = request
                .plugin
                .as_ref()
                .filter(|context| context.invocation.is_some())
            {
                super::plugins::invocations::package(db, caller, context)?;
            } else {
                super::dispatch::available(db, package)?;
            }
            let turn = super::dispatch::jobs::turn(db, &package.name, *job)?;
            let run = super::agent::stop(db, turn)?;
            Ok((
                Output::Run(run.clone()),
                Some(super::agent::activity::changed(db, run.session)?),
            ))
        }
        Command::StopTurn { turn } => {
            let run = super::agent::stop(db, *turn)?;
            Ok((
                Output::Run(run.clone()),
                Some(super::agent::activity::changed(db, run.session)?),
            ))
        }
        Command::ListTerminals { worktree } => Ok((
            Output::Terminals(
                super::terminals::list(db)?
                    .into_iter()
                    .filter(|info| info.worktree == Some(*worktree))
                    .collect(),
            ),
            None,
        )),
        Command::BeginProviderLogin { .. }
        | Command::CancelProviderLogin { .. }
        | Command::BeginMcpLogin { .. }
        | Command::CompleteMcpLogin { .. }
        | Command::CancelMcpLogin { .. } => Err(Fault::new(
            ErrorCode::Internal,
            "authorization command must use its lifecycle owner",
        )),
        Command::CreateTerminal(_)
        | Command::OpenTerminal { .. }
        | Command::OpenToolTerminal { .. }
        | Command::ListTerminalTools { .. }
        | Command::CompleteBrowser { .. }
        | Command::UseExternalBrowser { .. }
        | Command::UseBrowser { .. }
        | Command::UseComputer { .. }
        | Command::UseMedia { .. }
        | Command::OpenPort { .. }
        | Command::CancelPort { .. }
        | Command::RunCommand { .. }
        | Command::ListCommands { .. }
        | Command::ReadCommand { .. }
        | Command::StopCommand { .. }
        | Command::StopCommands { .. }
        | Command::InstallHost { .. }
        | Command::ReadHostInstall { .. }
        | Command::CheckSsh { .. }
        | Command::OpenSshTerminal { .. }
        | Command::CloseSshTerminal { .. }
        | Command::BrowseDatabase { .. }
        | Command::CheckDatabase { .. }
        | Command::TestDatabase { .. }
        | Command::QueryDatabase { .. }
        | Command::CancelDatabase { .. }
        | Command::RunSsh { .. }
        | Command::TransferSsh { .. }
        | Command::BrowseSshDirectory { .. }
        | Command::DownloadSshFile { .. }
        | Command::FinishSshUpload { .. }
        | Command::ModifySshFile { .. }
        | Command::CancelSsh { .. }
        | Command::CloseTerminal { .. }
        | Command::InspectTerminal { .. }
        | Command::ClaimTerminal { .. }
        | Command::InputTerminal { .. }
        | Command::ResizeTerminal { .. }
        | Command::SetTerminalAppearance { .. } => Err(Fault::new(
            ErrorCode::Internal,
            "terminal command must use its owner thread",
        )),
        Command::PluginTransaction { .. }
        | Command::GeneratePluginText { .. }
        | Command::RequestPluginHttp(_)
        | Command::CallPlugin { .. }
        | Command::CancelPluginCall { .. }
        | Command::ReadPluginView { .. }
        | Command::ReadPluginMcp { .. }
        | Command::DiscoverModels(_)
        | Command::ValidateProvider { .. }
        | Command::RefreshModelCatalog
        | Command::InspectHost
        | Command::ListShells
        | Command::ReadHostMetrics
        | Command::ReadComputerPermissions
        | Command::RequestComputerPermission { .. }
        | Command::StopHostProcess { .. }
        | Command::InspectGit { .. }
        | Command::ReadGitStash { .. }
        | Command::ReadGitOutput { .. }
        | Command::ListGitRemoteTags { .. }
        | Command::ListGitBranches { .. }
        | Command::ResolveGitRevision { .. }
        | Command::ListWorktrees { .. }
        | Command::RegisterWorktree { .. }
        | Command::CreateGitBranch { .. }
        | Command::CreateWorktree { .. }
        | Command::CreateManagedWorktree { .. }
        | Command::RemoveWorktree { .. }
        | Command::RenameGitBranch { .. }
        | Command::DeleteGitBranch { .. }
        | Command::SwitchGitBranch { .. }
        | Command::MergeGitBranch { .. }
        | Command::RunGitAction { .. }
        | Command::ReadGitLog { .. }
        | Command::ReadGitCommit { .. }
        | Command::ReadGitDiff { .. }
        | Command::ListDirectory { .. }
        | Command::ReadFile { .. }
        | Command::OfficeRuntime { .. }
        | Command::ReadOffice { .. }
        | Command::ExportPdf { .. }
        | Command::PreviewOffice { .. }
        | Command::DownloadFile { .. }
        | Command::UploadFile(_)
        | Command::StageSshUpload(_)
        | Command::FinishFileUpload { .. }
        | Command::FinishAttachmentUpload { .. }
        | Command::UploadAttachment(_)
        | Command::DownloadAttachment { .. }
        | Command::DownloadImage { .. }
        | Command::CancelFileTransfer { .. }
        | Command::SearchFiles { .. }
        | Command::WriteFile { .. }
        | Command::InstallPlugin { .. }
        | Command::DiscoverSkills { .. }
        | Command::SearchPluginCatalog { .. }
        | Command::ReadCatalogPlugin { .. }
        | Command::ReadCatalogPluginInfo { .. }
        | Command::InspectPluginSource { .. }
        | Command::CheckPluginUpdate { .. }
        | Command::InstallPluginSource { .. }
        | Command::InstallBundledPlugin { .. }
        | Command::InstallSkill { .. }
        | Command::InstallMcp { .. }
        | Command::SavePluginMcp { .. }
        | Command::InstallPluginUpload { .. }
        | Command::UploadPlugin(_)
        | Command::InspectPluginUpload { .. }
        | Command::RestoreFileCheckpoint { .. }
        | Command::CreateDirectory { .. }
        | Command::RenameEntry { .. }
        | Command::CopyEntry { .. }
        | Command::CopyEntryTo { .. }
        | Command::MoveEntryTo { .. }
        | Command::TrashEntry { .. }
        | Command::TrashFile { .. }
        | Command::UpdateGitIndex { .. }
        | Command::CreateGitCommit { .. } => Err(Fault::new(
            ErrorCode::Internal,
            "resource inspection must bypass the database",
        )),
        Command::ListCredentials
        | Command::PutCredential { .. }
        | Command::RevokeCredential { .. } => Ok((
            super::providers::authentication::execute(db, &request.command)?,
            None,
        )),
        Command::Snapshot => Ok((Output::Snapshot(snapshot(db, node)?), None)),
        Command::InspectRequest { id, digest } => Ok((
            Output::RequestOutcome {
                id: *id,
                outcome: super::outcomes::inspect(db, caller, *id, digest)?,
            },
            None,
        )),
        Command::RegisterProject { name, path } => {
            super::projects::register(db, name, path, Default::default())
        }
        Command::SetDefaults {
            expected_revision,
            config,
        } => {
            if config.resource.is_some() || config.assistant.is_some() {
                return Err(invalid(
                    "default conversations cannot bind a resource or plugin assistant",
                ));
            }
            validate_config(db, node, config)?;
            super::sessions::validate_model(db, config, None)?;
            let current = defaults(db)?;
            check_revision(current.revision, *expected_revision)?;
            let defaults = Defaults {
                revision: next_revision(current.revision)?,
                config: Some(config.clone()),
            };
            db.execute("INSERT INTO defaults(singleton,revision,config) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision,config=excluded.config", params![defaults.revision as i64, encode(config)?]).map_err(storage_error)?;
            Ok((
                Output::Defaults(defaults.clone()),
                Some(Event::DefaultsChanged(defaults)),
            ))
        }
        Command::BindConnectionSession {
            session,
            expected_revision,
            resource,
        } => super::connections::bind(db, profile, *session, *expected_revision, *resource),
        Command::CreateSession {
            project,
            worktree,
            config,
        } => {
            let config = match config {
                Some(config) => config.clone(),
                None => defaults(db)?
                    .config
                    .ok_or_else(|| invalid("Node model defaults are not configured"))?,
            };
            let roles = super::sessions::roles::capture(db, node, super::roles::list(db)?)?;
            super::sessions::create(
                db,
                node,
                profile,
                *project,
                *worktree,
                super::sessions::Configuration {
                    config,
                    profile: None,
                    roles,
                },
            )
        }
        Command::ImportSession(bundle) => {
            super::sessions::import(db, node, caller, profile, bundle)
        }
        Command::RenameSession {
            session,
            expected_revision,
            title,
        } => super::sessions::rename(db, *session, *expected_revision, title),
        Command::SetSessionRead {
            session,
            expected_revision,
            read,
        } => super::sessions::attention::set(db, *session, *expected_revision, *read),
        Command::SetSessionArchived {
            session,
            expected_revision,
            archived,
        } => super::sessions::archive(db, *session, *expected_revision, *archived),
        Command::SetSessionOrder {
            project,
            expected,
            sessions,
        } => super::sessions::reorder(db, *project, expected, sessions),
        Command::RemoveSession {
            session,
            expected_revision,
        } => super::sessions::remove(db, *session, *expected_revision),
        Command::CreateSessionAt { .. } => {
            Err(invalid("configuration forwarding requires the Node worker"))
        }
        Command::MoveConversation {
            session,
            worktree,
            expected_revision,
        } => super::sessions::location::move_to(db, *session, *worktree, *expected_revision),
        Command::ForkConversationAt {
            session,
            worktree,
            expected_revision,
        } => super::sessions::location::fork_at(db, *session, *worktree, *expected_revision),
        Command::SetSessionConfig {
            session: id,
            expected_revision,
            config,
        } => {
            validate_config(db, node, config)?;
            let mut session = session(db, *id)?;
            check_revision(session.revision, *expected_revision)?;
            if session.config.resource != config.resource
                || session.config.assistant != config.assistant
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "conversation resource and assistant scope are immutable",
                ));
            }
            if session.config.provider != config.provider {
                session.profile = None;
            } else if let Some(profile) = &mut session.profile {
                profile.provider.credential = config.credential.clone();
            }
            super::sessions::validate_model(db, config, session.profile.as_ref())?;
            session.config = config.clone();
            revise_session(db, session)
        }
        Command::SetSessionRoles {
            session: id,
            expected_revision,
            roles,
        } => {
            let mut session = session(db, *id)?;
            super::sessions::writable(&session)?;
            check_revision(session.revision, *expected_revision)?;
            session.roles = super::sessions::roles::select(db, node, roles)?;
            revise_session(db, session)
        }
        Command::CompactContext {
            session: id,
            expected_revision,
        } => {
            let session = session(db, *id)?;
            check_revision(session.revision, *expected_revision)?;
            let turn = super::agent::queue::compact(db, caller, request, session)?;
            Ok((
                Output::QueuedTurn(turn.clone()),
                Some(super::agent::activity::queued(db, turn)?),
            ))
        }
        Command::ContinueTurn { .. } => super::agent::continuation::admit(db, caller, request),
        Command::QueueTurn {
            session: id,
            expected_revision,
            message,
        }
        | Command::SubmitTurn {
            session: id,
            expected_revision,
            message,
        } => {
            let session = session(db, *id)?;
            check_revision(session.revision, *expected_revision)?;
            let turn = super::agent::queue::create(
                db,
                caller,
                request,
                session,
                message,
                matches!(request.command, Command::SubmitTurn { .. }),
            )?;
            Ok((
                Output::QueuedTurn(turn.clone()),
                Some(super::agent::activity::queued(db, turn)?),
            ))
        }
    }
}

pub(super) fn validate_config(
    db: &Connection,
    node: NodeId,
    config: &SessionConfig,
) -> Result<(), Fault> {
    if config.model.trim().is_empty() || config.model.len() > 256 {
        return Err(invalid("model identifier is required and must be bounded"));
    }
    if config
        .credential
        .as_ref()
        .is_some_and(|credential| credential.node != node)
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "credential references must belong to the execution Node",
        ));
    }
    if let Some(reference) = &config.credential {
        super::providers::authentication::validate(db, reference.id, config.provider)?;
    }
    Ok(())
}

pub(super) fn check_revision(actual: u64, expected: u64) -> Result<(), Fault> {
    if actual == expected {
        Ok(())
    } else {
        Err(Fault::new(
            ErrorCode::RevisionConflict,
            "configuration revision changed; reload before editing",
        ))
    }
}

fn next_revision(revision: u64) -> Result<u64, Fault> {
    revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| invalid("configuration revision exhausted"))
}

pub(super) fn revise_session(
    db: &Connection,
    mut session: Session,
) -> Result<(Output, Option<Event>), Fault> {
    super::sessions::writable(&session)?;
    session.revision = next_revision(session.revision)?;
    insert_revision(db, &session)?;
    db.execute(
        "UPDATE sessions SET revision=?2,worktree=?3 WHERE id=?1",
        params![
            session.id.to_string(),
            session.revision as i64,
            session.worktree.to_string()
        ],
    )
    .map_err(storage_error)?;
    Ok((
        Output::Session(session.clone()),
        Some(Event::SessionChanged(Box::new(session))),
    ))
}

pub(super) fn insert_revision(db: &Connection, session: &Session) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO session_revisions(session,revision,config,profile,roles,worktree) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            session.id.to_string(),
            session.revision as i64,
            encode(&session.config)?,
            session.profile.as_ref().map(encode).transpose()?,
            encode(&session.roles)?,
            session.worktree.to_string()
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn defaults(db: &Connection) -> Result<Defaults, Fault> {
    let row: Option<(i64, Vec<u8>)> = db
        .query_row(
            "SELECT revision,config FROM defaults WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    match row {
        Some((revision, config)) => Ok(Defaults {
            revision: revision.try_into().map_err(storage_error)?,
            config: Some(serde_json::from_slice(&config).map_err(storage_error)?),
        }),
        None => Ok(Defaults {
            revision: 0,
            config: None,
        }),
    }
}

pub(super) fn session(db: &Connection, id: SessionId) -> Result<Session, Fault> {
    type StoredSession = (Option<String>, i64, Vec<u8>, String, bool);
    let row: Option<StoredSession> = db.query_row("SELECT s.project,s.revision,r.config,s.worktree,s.state='archived' FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision WHERE s.id=?1 AND s.state!='removed'", [id.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).optional().map_err(storage_error)?;
    let (project, revision, config, worktree, archived) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "session does not exist on this Node"))?;
    Ok(Session {
        id,
        archived,
        activity: super::agent::activity::read(db, id)?,
        project: project
            .map(|id| id.parse())
            .transpose()
            .map_err(storage_error)?,
        worktree: worktree.parse().map_err(storage_error)?,
        revision: revision.try_into().map_err(storage_error)?,
        config: serde_json::from_slice(&config).map_err(storage_error)?,
        roles: super::sessions::roles::read(db, id, revision.try_into().map_err(storage_error)?)?,
        profile: super::sessions::profile(db, id, revision.try_into().map_err(storage_error)?)?,
        fork: super::sessions::origin(db, id)?,
        delegation: super::agent::delegation(db, id)?,
    })
}

fn snapshot(db: &Connection, node: NodeId) -> Result<Snapshot, Fault> {
    let projects = super::projects::list(db)?;
    let sessions = super::sessions::ordered(db)?
        .into_iter()
        .map(|id| session(db, id))
        .collect::<Result<Vec<_>, Fault>>()?;
    let mut query = db.prepare("SELECT t.id,t.session,t.request,t.revision,r.config,t.kind FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision JOIN sessions s ON s.id=t.session WHERE s.state!='removed' ORDER BY t.rowid").map_err(storage_error)?;
    let turns = query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (id, session, request, revision, config, kind) = row.map_err(storage_error)?;
            Ok(QueuedTurn {
                id: id.parse().map_err(storage_error)?,
                kind: match kind.as_str() {
                    "task" => conversation::RunKind::Task,
                    "compaction" => conversation::RunKind::Compaction,
                    _ => return Err(storage_error("turn kind is invalid")),
                },
                session: session.parse().map_err(storage_error)?,
                request: request.parse().map_err(storage_error)?,
                revision: revision.try_into().map_err(storage_error)?,
                config: serde_json::from_slice(&config).map_err(storage_error)?,
                roles: super::sessions::roles::read(
                    db,
                    session.parse().map_err(storage_error)?,
                    revision.try_into().map_err(storage_error)?,
                )?,
                plugins: crate::store::agent::plugins::read(
                    db,
                    id.parse().map_err(storage_error)?,
                )?,
            })
        })
        .collect::<Result<Vec<_>, Fault>>()?;
    let cursor = db
        .query_row("SELECT coalesce(max(cursor),0) FROM events", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(storage_error)?;
    Ok(Snapshot {
        node,
        worktrees: super::worktrees::list(db)?,
        terminals: super::terminals::list(db)?,
        terminal_settings_revision: super::terminals::settings::read(db)?.revision,
        media_settings: super::media::read(db)?,
        providers: super::agent::providers(db)?,
        model_catalog: super::providers::catalog::status(db)?,
        roles: super::roles::list(db)?,
        ssh: super::ssh::list(db)?,
        databases: super::databases::list(db)?,
        plugins: super::plugins::list(db)?,
        notifications: super::notifications::list(db)?,
        cursor: cursor.try_into().map_err(storage_error)?,
        defaults: defaults(db)?,
        projects,
        sessions,
        turns,
    })
}

pub(super) fn invalid(message: &'static str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
