use sailry_protocol::*;
use turso::{Connection, params};

use super::database::{encode, storage_error};

pub(super) fn execute<'a>(
    db: &'a Connection,
    node: NodeId,
    caller: NodeId,
    profile: Option<&'a std::path::Path>,
    request: &'a Request,
) -> sailry_link::Pending<'a, Result<(Output, Option<Event>), Fault>> {
    // Select the handler before polling its async work.
    match &request.command {
        Command::ListConnectorTypes
        | Command::ListConnections
        | Command::UpdateConnection { .. }
        | Command::SetConnectionEnabled { .. }
        | Command::DisconnectConnection { .. }
        | Command::RemoveConnection { .. }
        | Command::ConfirmConnectorConsent { .. } => {
            defer(
                move || async move { super::plugins::connectors::execute(db, profile, request).await },
            )
        }
        Command::InspectConnectorCli { .. }
        | Command::StartConnectorLogin { .. }
        | Command::SubmitConnectorInput { .. }
        | Command::CancelConnectorLogin { .. }
        | Command::ProbeConnection { .. } => defer(move || async move {
            Err(Fault::new(
                ErrorCode::Internal,
                "connector lifecycle command reached the synchronous handler",
            ))
        }),
        Command::DeliverBackground { .. } => defer(move || async move {
            Err(Fault::new(
                ErrorCode::PermissionDenied,
                "background delivery is internal Node input",
            ))
        }),
        Command::WriteArtifact(_)
        | Command::ReadArtifact { .. }
        | Command::ListArtifacts { .. }
        | Command::ListArtifactRevisions { .. } => {
            defer(move || async move { super::artifacts::execute(db, profile, request).await })
        }
        Command::ListKnowledgeCollections => defer(move || async move {
            super::knowledge::list(db, request)
                .await
                .map(|collections| (Output::KnowledgeCollections(collections), None))
        }),
        Command::CreateKnowledgeCollection { .. }
        | Command::UpdateKnowledgeCollection { .. }
        | Command::DeleteKnowledgeCollection { .. }
        | Command::RebuildKnowledgeCollection { .. }
        | Command::PauseKnowledgeCollection { .. } => {
            defer(move || async move { super::knowledge::execute(db, request).await })
        }
        Command::SearchModels(_)
        | Command::ReadModelVersions { .. }
        | Command::ResolveModel { .. }
        | Command::RefreshLocalModel { .. }
        | Command::QueryKnowledge(_)
        | Command::ReadKnowledgeCitation(_) => defer(move || async move {
            Err(Fault::new(
                ErrorCode::Internal,
                "knowledge execution must run outside the database",
            ))
        }),
        Command::StartSession(draft) => defer(move || async move {
            super::sessions::start(db, node, caller, profile, request, draft).await
        }),
        Command::ReadProjectCatalog => defer(move || async move {
            let projects = super::projects::list(db).await?;
            let worktrees = super::worktrees::list(db)
                .await?
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
        }),
        Command::ReadWorktreeCatalog { worktree } => defer(move || async move {
            let entries = super::worktrees::list(db).await?;
            let source = entries
                .iter()
                .find(|entry| entry.id == *worktree)
                .ok_or_else(|| {
                    Fault::new(ErrorCode::NotFound, "captured worktree is unavailable")
                })?;
            let projects = super::projects::list(db)
                .await?
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
        }),
        Command::ReportDiagnostic(report) => defer(move || async move {
            if request.plugin.is_some() {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "system diagnostics require a controller",
                ));
            }
            super::notifications::diagnostics::report(db, report).await
        }),
        Command::PublishNotification { .. }
        | Command::MarkNotificationRead { .. }
        | Command::MarkNotificationsRead { .. }
        | Command::DismissReadNotifications { .. }
        | Command::DismissNotifications { .. } => defer(move || async move {
            let admitted = (match request
                .plugin
                .as_ref()
                .filter(|context| context.invocation.is_some())
            {
                Some(value) => Some(
                    async {
                        let context = value;
                        super::plugins::invocations::package(db, caller, context).await
                    }
                    .await,
                ),
                None => None,
            })
            .transpose()?;
            super::notifications::execute(db, &request.command, admitted).await
        }),
        Command::Dispatch { package, action } => defer(move || async move {
            if let Some(context) = request
                .plugin
                .as_ref()
                .filter(|context| context.invocation.is_some())
            {
                super::plugins::invocations::package(db, caller, context).await?;
                super::dispatch::execute_admitted(db, package, action).await
            } else {
                super::dispatch::execute(db, package, action).await
            }
        }),
        Command::UpdateProject {
            expected,
            name,
            path,
            appearance,
        } => defer(move || async move {
            super::projects::update(db, expected, name, path, appearance).await
        }),
        Command::RemoveProject { expected } => {
            defer(move || async move { super::projects::remove(db, expected).await })
        }
        Command::BrowseFiles { .. } | Command::ManageFiles(_) | Command::CreateProject(_) => {
            defer(move || async move {
                Err(Fault::new(
                    ErrorCode::Internal,
                    "host file operations must run outside the database",
                ))
            })
        }
        Command::ListMediaModels => defer(move || async move {
            Ok((Output::MediaModels(super::media::catalog(db).await?), None))
        }),
        Command::ReadMediaSettings => defer(move || async move {
            Ok((Output::MediaSettings(super::media::read(db).await?), None))
        }),
        Command::SaveMediaSettings(settings) => {
            defer(move || async move { super::media::save(db, settings).await })
        }
        Command::ReadHooks { worktree } => defer(move || async move {
            super::hooks::controller(request)?;
            Ok((
                Output::HookSettings(super::hooks::state(
                    *worktree,
                    super::hooks::read(db, *worktree).await?,
                )),
                None,
            ))
        }),
        Command::SaveHooks {
            worktree,
            settings,
            expected_revision,
        } => defer(move || async move {
            super::hooks::controller(request)?;
            Ok((
                Output::HookSettings(super::hooks::state(
                    *worktree,
                    super::hooks::save(db, *worktree, settings, *expected_revision).await?,
                )),
                None,
            ))
        }),
        Command::DismissInstallation { .. } | Command::ReadInstallations { .. } => {
            defer(move || async move {
                Err(Fault::new(
                    ErrorCode::Internal,
                    "installation observation must run outside the database",
                ))
            })
        }
        Command::SearchCode { .. }
        | Command::OutlineCode { .. }
        | Command::CancelCodeIndexQuery { .. } => defer(move || async move {
            Err(Fault::new(
                ErrorCode::Internal,
                "code queries must run outside the database",
            ))
        }),
        Command::ReadCodeIndexSettings => defer(move || async move {
            Ok((
                Output::CodeIndexSettings(super::code_index::settings(db).await?),
                None,
            ))
        }),
        Command::SaveCodeIndexSettings {
            settings,
            expected_revision,
        } => defer(move || async move {
            let saved = super::code_index::save(db, settings, *expected_revision).await?;
            let changed = super::plugins::services::synchronize(
                db,
                sailry_protocol::plugin::Service::CodeIndex,
                saved.enabled,
            )
            .await?;
            Ok((
                Output::CodeIndexSettings(saved),
                changed.map(Event::PluginChanged),
            ))
        }),
        Command::ReadPowerSettings => defer(move || async move {
            Ok((Output::PowerSettings(super::power::read(db).await?), None))
        }),
        Command::SavePowerSettings(settings) => {
            defer(move || async move { super::power::save(db, settings).await })
        }
        Command::ReadTerminalSettings => defer(move || async move {
            Ok((
                Output::TerminalSettings(super::terminals::settings::read(db).await?),
                None,
            ))
        }),
        Command::SaveTerminalSettings(settings) => {
            defer(move || async move { super::terminals::settings::save(db, settings).await })
        }
        Command::ReadNetworkSettings | Command::SaveNetworkSettings { .. } => {
            defer(move || async move {
                Err(Fault::new(
                    ErrorCode::Internal,
                    "network configuration requires the Link worker",
                ))
            })
        }
        Command::ReadSandboxSettings => defer(move || async move {
            Ok((
                Output::SandboxSettings(super::sandbox::read(db).await?),
                None,
            ))
        }),
        Command::SaveSandboxSettings(settings) => {
            defer(move || async move { super::sandbox::save(db, settings).await })
        }
        Command::ReadSearchSettings => defer(move || async move {
            Ok((Output::SearchSettings(super::search::read(db).await?), None))
        }),
        Command::SaveSearchSettings(update) => {
            defer(move || async move { super::search::save(db, request, update).await })
        }
        Command::ReadBackupSettings | Command::SaveBackupSettings(_) => {
            defer(move || async move { super::backup::execute(db, request).await })
        }
        Command::ListBackups
        | Command::ExportBackup { .. }
        | Command::RestoreBackup { .. }
        | Command::RestoreBackupFile { .. }
        | Command::DownloadBackup { .. }
        | Command::UploadBackup { .. } => defer(move || async move {
            Err(Fault::invalid(
                "backup operation requires the resource worker",
            ))
        }),
        Command::ReadPythonSettings => defer(move || async move {
            super::python::controller(request)?;
            Ok((Output::PythonSettings(super::python::read(db).await?), None))
        }),
        Command::SavePythonSettings(settings) => {
            defer(move || async move { super::python::save(db, request, settings).await })
        }
        Command::ReadLocalModelSettings
        | Command::SaveLocalModelSettings(_)
        | Command::RegisterLocalModel { .. } => {
            defer(move || async move { super::local_models::execute(db, request).await })
        }
        Command::Decide(_)
        | Command::ReadLocalModels { .. }
        | Command::CancelLocalModel { .. }
        | Command::InstallLocalModel { .. }
        | Command::DeleteLocalModel { .. } => {
            defer(
                move || async move { Err(Fault::invalid("local model resource command expected")) },
            )
        }
        Command::SearchWeb(_)
        | Command::CancelWebSearch { .. }
        | Command::TestSearchDriver { .. } => defer(move || async move {
            Err(Fault::new(
                ErrorCode::Internal,
                "web search must run outside the database",
            ))
        }),
        Command::ListDatabases if request.plugin.is_some() => defer(move || async move {
            let context = request.plugin.as_ref().expect("checked plugin context");
            let mut profiles = Vec::new();
            for profile in super::databases::list(db).await? {
                if super::plugins::connections::check_target(
                    db,
                    context,
                    connection::Resource::Database(profile.id),
                )
                .await
                .is_ok()
                {
                    profiles.push(profile);
                }
            }
            Ok((Output::DatabaseProfiles(profiles), None))
        }),
        Command::ListSsh if request.plugin.is_some() => defer(move || async move {
            let context = request.plugin.as_ref().expect("checked plugin context");
            let mut profiles = Vec::new();
            for profile in super::ssh::list(db).await? {
                if super::plugins::connections::check_target(
                    db,
                    context,
                    connection::Resource::Ssh(profile.id),
                )
                .await
                .is_ok()
                {
                    profiles.push(profile);
                }
            }
            Ok((Output::SshProfiles(profiles), None))
        }),
        Command::ListSshTerminals { profile } => defer(move || async move {
            Ok((
                Output::Terminals(
                    super::terminals::list(db)
                        .await?
                        .into_iter()
                        .filter(|info| {
                            info.ssh == Some(*profile)
                                && info.owner == Some(caller)
                                && info.status != terminal::Status::Closed
                        })
                        .collect(),
                ),
                None,
            ))
        }),
        Command::ListDatabases | Command::SaveDatabase { .. } | Command::RemoveDatabase { .. } => {
            defer(move || async move { super::databases::execute(db, &request.command).await })
        }
        Command::ListSsh
        | Command::SaveSsh { .. }
        | Command::RemoveSsh { .. }
        | Command::TrustSsh { .. } => {
            defer(move || async move { super::ssh::execute(db, &request.command).await })
        }
        Command::ReadAttachment {
            worktree,
            attachment,
        } => defer(move || async move {
            Ok((
                Output::Attachment(super::attachments::read(db, *worktree, *attachment).await?),
                None,
            ))
        }),
        Command::DiscardAttachment {
            worktree,
            attachment,
        } => defer(move || async move {
            super::attachments::discard(db, caller, *worktree, *attachment).await?;
            Ok((
                Output::AttachmentDiscarded {
                    attachment: *attachment,
                },
                None,
            ))
        }),
        Command::ReadPluginValue { .. }
        | Command::ReadPluginConversationValue { .. }
        | Command::ListPluginKeys { .. }
        | Command::WritePluginValue { .. }
        | Command::WriteIndexedPluginValue { .. }
        | Command::SearchPluginValues(_)
        | Command::RemovePluginValue { .. }
        | Command::WritePluginConversationValue { .. }
        | Command::RemovePluginConversationValue { .. } => {
            defer(move || async move { super::plugins::execute_storage(db, request).await })
        }
        Command::ReadPluginSettings { .. }
            if request
                .plugin
                .as_ref()
                .is_some_and(|context| context.invocation.is_some() || context.turn.is_some()) =>
        {
            defer(move || async move {
                let context = request.plugin.as_ref().expect("checked plugin context");
                let info = if context.invocation.is_some() {
                    super::plugins::invocations::package(db, caller, context).await?
                } else {
                    super::agent::plugin_package(db, context).await?
                };
                Ok((
                    Output::PluginSettings(
                        super::plugins::settings::read_admitted(db, &info).await?,
                    ),
                    None,
                ))
            })
        }
        Command::ReadPluginConnection { card, .. }
            if request
                .plugin
                .as_ref()
                .is_some_and(|context| context.invocation.is_some() || context.turn.is_some()) =>
        {
            defer(move || async move {
                let context = request.plugin.as_ref().expect("checked plugin context");
                let info = if context.invocation.is_some() {
                    super::plugins::invocations::package(db, caller, context).await?
                } else {
                    super::agent::plugin_package(db, context).await?
                };
                Ok((
                    Output::PluginConnection(
                        super::plugins::connection::read_admitted(db, profile, &info, card).await?,
                    ),
                    None,
                ))
            })
        }
        Command::ReadPluginConnection { package, card } => defer(move || async move {
            Ok((
                Output::PluginConnection(
                    super::plugins::connection::read(db, profile, package, card).await?,
                ),
                None,
            ))
        }),
        Command::ListPlugins
        | Command::ReadPluginSettings { .. }
        | Command::ReadPluginSecret { .. }
        | Command::ListPluginModels
        | Command::ResolvePluginModel { .. }
        | Command::SavePluginSettings { .. }
        | Command::ReadPlugin { .. }
        | Command::ReadPluginVersion { .. }
        | Command::SetPluginEnabled { .. }
        | Command::SetPluginMcpAllowed { .. }
        | Command::RemovePlugin { .. } => {
            defer(
                move || async move { super::plugins::execute(db, profile, &request.command).await },
            )
        }
        Command::ListRoles => {
            defer(move || async move { Ok((Output::Roles(super::roles::list(db).await?), None)) })
        }
        Command::PutRole {
            role,
            expected_revision,
        } => defer(move || async move {
            super::roles::approvals::check(db, request).await?;
            let role = super::roles::put(db, role, *expected_revision).await?;
            Ok((Output::Role(role.clone()), Some(Event::RoleChanged(role))))
        }),
        Command::RemoveRole {
            role,
            expected_revision,
        } => defer(move || async move {
            super::roles::approvals::check(db, request).await?;
            super::roles::remove(db, *role, *expected_revision).await?;
            Ok((
                Output::Roles(super::roles::list(db).await?),
                Some(Event::RoleRemoved { id: *role }),
            ))
        }),
        Command::ResolveQuestion {
            session,
            question,
            response,
        } => defer(move || async move {
            if matches!(
                response,
                conversation::question::Response::StartCoding { .. }
            ) {
                let accepted =
                    Box::new(super::agent::questions::accept(db, caller, request).await?);
                return Ok((
                    Output::PlanAccepted(accepted.clone()),
                    Some(Event::PlanAccepted(accepted)),
                ));
            }
            let question =
                super::agent::questions::resolve(db, *session, *question, response).await?;
            Ok((
                Output::Question(question),
                Some(super::agent::activity::changed(db, *session).await?),
            ))
        }),
        Command::ResolveApproval {
            session,
            approval,
            decision,
        } => defer(move || async move {
            let approval =
                super::agent::approvals::resolve(db, *session, *approval, decision.clone()).await?;
            // The database owner records activity after claiming this decision
            // and applying any matching session approvals in the same transaction.
            Ok((Output::Approval(approval), None))
        }),
        Command::ListProviders => defer(move || async move {
            Ok((Output::Providers(super::agent::providers(db).await?), None))
        }),
        Command::ReadProviderKey {
            provider,
            expected_revision,
        } => defer(move || async move {
            Ok((
                Output::ProviderKey(
                    super::providers::authentication::provider_key(
                        db,
                        node,
                        *provider,
                        *expected_revision,
                    )
                    .await?,
                ),
                None,
            ))
        }),
        Command::ListChannelPresets { .. } => defer(move || async move {
            Err(Fault::new(
                ErrorCode::Internal,
                "model partner loading must run outside the database",
            ))
        }),
        Command::ReadCatalogStatus => defer(move || async move {
            Ok((
                Output::CatalogStatus(super::providers::catalog::status(db).await?),
                None,
            ))
        }),
        Command::ReadModelCatalog(query) => defer(move || async move {
            Ok((
                Output::ModelCatalog(super::providers::catalog::page(db, query).await?),
                None,
            ))
        }),
        Command::PutProvider {
            provider,
            expected_revision,
        } => defer(move || async move {
            let provider =
                super::agent::put_provider(db, node, provider, *expected_revision).await?;
            Ok((
                Output::Provider(provider.clone()),
                Some(Event::ProviderChanged(provider)),
            ))
        }),
        Command::SaveProvider {
            provider,
            expected_revision,
            secret,
        } => defer(move || async move {
            let mut provider = provider.clone();
            if let Some(secret) = secret {
                if provider.authentication != Authentication::ApiKey {
                    return Err(sailry_protocol::Fault::invalid(
                        "stored secrets require API-key authentication",
                    ));
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
                )
                .await?;
                provider.credential = Some(CredentialRef { node, id });
            }
            let provider =
                super::agent::put_provider(db, node, &provider, *expected_revision).await?;
            Ok((
                Output::Provider(provider.clone()),
                Some(Event::ProviderChanged(provider)),
            ))
        }),
        Command::RemoveProvider {
            provider,
            expected_revision,
        } => defer(move || async move {
            super::agent::remove_provider(db, *provider, *expected_revision).await?;
            Ok((
                Output::Providers(super::agent::providers(db).await?),
                Some(Event::ProviderRemoved { id: *provider }),
            ))
        }),
        Command::ReadUsage(query) => defer(move || async move {
            Ok((
                Output::Usage(super::agent::statistics::report::read(db, node, query).await?),
                None,
            ))
        }),
        Command::ReadActivityCatalog => defer(move || async move {
            Ok((
                Output::ActivityCatalog(super::agent::activity::catalog(db, node).await?),
                None,
            ))
        }),
        Command::ReadActivity { sessions } => defer(move || async move {
            Ok((
                Output::Activity(super::agent::activity::previews(db, sessions).await?),
                None,
            ))
        }),
        Command::ReadSession { session: id } => {
            defer(move || async move { Ok((Output::Session(session(db, *id).await?), None)) })
        }
        Command::ReadConversation {
            session,
            before,
            limit,
        } => defer(move || async move {
            Ok((
                Output::Conversation(super::agent::read(db, *session, *before, *limit).await?),
                None,
            ))
        }),
        Command::ReadTurnCapture { session, turn } => defer(move || async move {
            Ok((
                Output::QueuedTurn(super::captures::read(db, *session, *turn).await?),
                None,
            ))
        }),
        Command::ReadTurn {
            session,
            turn,
            expected_revision,
            before,
            limit,
        } => defer(move || async move {
            check_revision(
                super::sessions::history_revision(db, *session).await?,
                *expected_revision,
            )?;
            Ok((
                Output::TurnHistory(
                    super::agent::read_turn(db, *session, *turn, *before, *limit).await?,
                ),
                None,
            ))
        }),
        Command::ListConversationAssets { session, query } => defer(move || async move {
            Ok((
                Output::ConversationAssets(super::agent::assets::read(db, *session, query).await?),
                None,
            ))
        }),
        Command::SearchConversation { session, query } => defer(move || async move {
            Ok((
                Output::ConversationMatches(super::agent::search(db, *session, query).await?),
                None,
            ))
        }),
        Command::ListFileCheckpoints {
            session,
            turn,
            before,
            limit,
        } => defer(move || async move {
            Ok((
                Output::FileCheckpoints(
                    super::agent::checkpoints::list(db, *session, *turn, *before, *limit).await?,
                ),
                None,
            ))
        }),
        Command::ReadTurnDiff { session, turn } => defer(move || async move {
            Ok((
                Output::TurnDiff(super::agent::checkpoints::diff(db, *session, *turn).await?),
                None,
            ))
        }),
        Command::ReadTurnHunks {
            session,
            turn,
            path,
        } => defer(move || async move {
            Ok((
                Output::TurnHunks(
                    super::agent::checkpoints::hunks::read(db, *session, *turn, path).await?,
                ),
                None,
            ))
        }),
        Command::ReadFileCheckpoint {
            session,
            checkpoint,
        } => defer(move || async move {
            Ok((
                Output::FileCheckpoint(Box::new(
                    super::agent::checkpoints::read(db, *session, *checkpoint).await?,
                )),
                None,
            ))
        }),
        Command::ForkConversation {
            session,
            through,
            expected_revision,
        } => defer(move || async move {
            super::sessions::fork(db, *session, *through, *expected_revision).await
        }),
        Command::RewindConversation {
            session,
            through,
            expected_head,
            expected_revision,
        } => defer(move || async move {
            super::sessions::rewind(
                db,
                *session,
                *through,
                *expected_head,
                *expected_revision,
                false,
            )
            .await
        }),
        Command::ReplaceTurn { .. } => {
            defer(move || async move { super::sessions::replace(db, caller, request).await })
        }
        Command::StartQueuedTurn { turn } => defer(move || async move {
            let queued = super::agent::runs::get(db, *turn).await?;
            super::sessions::writable(&session(db, queued.session).await?)?;
            let run = super::agent::start(db, *turn).await?;
            Ok((
                Output::Run(run.clone()),
                Some(super::agent::activity::changed(db, run.session).await?),
            ))
        }),
        Command::GuideTurn { .. } => {
            defer(move || async move { super::agent::delegation::guide(db, caller, request).await })
        }
        Command::ReadQueuedTurn { .. }
        | Command::EditQueuedTurn { .. }
        | Command::RemoveQueuedTurn { .. }
        | Command::SendQueuedTurn { .. }
        | Command::GuideQueuedTurn { .. }
        | Command::MoveQueuedTurn { .. }
        | Command::SetQueuePaused { .. } => {
            defer(move || async move { super::agent::queue::execute(db, caller, request).await })
        }
        Command::StopDispatchTurn { package, job } => defer(move || async move {
            if let Some(context) = request
                .plugin
                .as_ref()
                .filter(|context| context.invocation.is_some())
            {
                super::plugins::invocations::package(db, caller, context).await?;
            } else {
                super::dispatch::available(db, package).await?;
            }
            let turn = super::dispatch::jobs::turn(db, &package.name, *job).await?;
            let run = super::agent::stop(db, turn).await?;
            Ok((
                Output::Run(run.clone()),
                Some(super::agent::activity::changed(db, run.session).await?),
            ))
        }),
        Command::StopTurn { turn } => defer(move || async move {
            let run = super::agent::stop(db, *turn).await?;
            Ok((
                Output::Run(run.clone()),
                Some(super::agent::activity::changed(db, run.session).await?),
            ))
        }),
        Command::ListTerminals { worktree } => defer(move || async move {
            Ok((
                Output::Terminals(
                    super::terminals::list(db)
                        .await?
                        .into_iter()
                        .filter(|info| info.worktree == Some(*worktree))
                        .collect(),
                ),
                None,
            ))
        }),
        Command::BeginProviderLogin { .. }
        | Command::CompleteProviderLogin { .. }
        | Command::CancelProviderLogin { .. } => defer(move || async move {
            Err(Fault::new(
                ErrorCode::Internal,
                "authorization command must use its lifecycle owner",
            ))
        }),
        Command::CreateTerminal(_)
        | Command::OpenTerminal { .. }
        | Command::OpenToolTerminal { .. }
        | Command::ListTerminalTools { .. }
        | Command::CompleteBrowser { .. }
        | Command::UseExternalBrowser { .. }
        | Command::UseBrowser { .. }
        | Command::UseMedia { .. }
        | Command::OpenPort { .. }
        | Command::CancelPort { .. }
        | Command::RunCommand { .. }
        | Command::ListCommands { .. }
        | Command::ReadCommand { .. }
        | Command::StopCommand { .. }
        | Command::WriteCommand { .. }
        | Command::StopCommands { .. }
        | Command::InstallHost { .. }
        | Command::UpdateSshHost { .. }
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
        | Command::SetTerminalAppearance { .. } => defer(move || async move {
            Err(Fault::new(
                ErrorCode::Internal,
                "terminal command must use its owner thread",
            ))
        }),
        Command::PluginTransaction { .. }
        | Command::GeneratePluginText { .. }
        | Command::RequestPluginHttp(_)
        | Command::RequestProviderHttp { .. }
        | Command::ExecuteProviderHttp { .. }
        | Command::FetchWeb(_)
        | Command::CancelWebFetch { .. }
        | Command::CallPlugin { .. }
        | Command::ReadPluginCallback { .. }
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
        | Command::ReadGitHead { .. }
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
        | Command::ReadGitShow { .. }
        | Command::ReadGitBlame { .. }
        | Command::ListGitChanges { .. }
        | Command::ReadGitChange { .. }
        | Command::ReadGitContext { .. }
        | Command::ReadGitCommit { .. }
        | Command::ReadGitDiff { .. }
        | Command::ListDirectory { .. }
        | Command::ReadFile { .. }
        | Command::ReadFileLines { .. }
        | Command::FindFiles { .. }
        | Command::ReadOffice { .. }
        | Command::ReadNotebook { .. }
        | Command::EditNotebook { .. }
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
        | Command::EditFile { .. }
        | Command::DeleteFile { .. }
        | Command::MoveFile { .. }
        | Command::InstallPlugin { .. }
        | Command::ReadSkillDocument { .. }
        | Command::DiscoverSkills { .. }
        | Command::DiscoverRegistrySkill { .. }
        | Command::DiscoverSkillUpload { .. }
        | Command::DiscoverLocalSkills { .. }
        | Command::SearchPluginCatalog { .. }
        | Command::SearchSkillCatalog { .. }
        | Command::ReadCatalogPlugin { .. }
        | Command::ReadCatalogPluginInfo { .. }
        | Command::InspectPluginSource { .. }
        | Command::CheckPluginUpdate { .. }
        | Command::CheckPluginUpdates { .. }
        | Command::InstallPluginSource { .. }
        | Command::InstallBundledPlugin { .. }
        | Command::InstallSkill { .. }
        | Command::InstallRegistrySkill { .. }
        | Command::InstallSkillUpload { .. }
        | Command::InstallMcp { .. }
        | Command::CheckPluginMcp { .. }
        | Command::SavePluginMcp { .. }
        | Command::InstallPluginUpload { .. }
        | Command::UploadPlugin(_)
        | Command::InspectPluginUpload { .. }
        | Command::RestoreFileCheckpoint { .. }
        | Command::ApplyTurnHunk(_)
        | Command::CreateDirectory { .. }
        | Command::RenameEntry { .. }
        | Command::CopyEntry { .. }
        | Command::CopyEntryTo { .. }
        | Command::MoveEntryTo { .. }
        | Command::TrashEntry { .. }
        | Command::TrashFile { .. }
        | Command::UpdateGitIndex { .. }
        | Command::CreateGitCommit { .. } => defer(move || async move {
            Err(Fault::new(
                ErrorCode::Internal,
                "resource inspection must bypass the database",
            ))
        }),
        Command::ListCredentials => defer(move || async move {
            Ok((
                super::providers::authentication::execute(db, &request.command).await?,
                None,
            ))
        }),
        Command::PutCredential { .. } | Command::RevokeCredential { .. } => {
            defer(move || async move {
                let output =
                    super::providers::authentication::execute(db, &request.command).await?;
                let Output::Credential(credential) = &output else {
                    return Err(Fault::new(
                        ErrorCode::Internal,
                        "credential metadata expected",
                    ));
                };
                let event = super::providers::authentication::changed(node, credential);
                Ok((output, Some(event)))
            })
        }
        Command::Snapshot => {
            defer(move || async move { Ok((Output::Snapshot(snapshot(db, node).await?), None)) })
        }
        Command::InspectRequest { id, digest } => defer(move || async move {
            Ok((
                Output::RequestOutcome {
                    id: *id,
                    outcome: super::outcomes::inspect(db, caller, *id, digest).await?,
                },
                None,
            ))
        }),
        Command::RegisterProject { name, path } => defer(move || async move {
            super::projects::register(db, name, path, Default::default()).await
        }),
        Command::SetDefaults {
            expected_revision,
            config,
        } => defer(move || async move {
            if config.resource.is_some() || config.assistant.is_some() {
                return Err(sailry_protocol::Fault::invalid(
                    "default conversations cannot bind a resource or plugin assistant",
                ));
            }
            validate_config(db, node, config).await?;
            super::sessions::validate_model(db, config, None).await?;
            let current = defaults(db).await?;
            check_revision(current.revision, *expected_revision)?;
            super::plugins::options::validate(
                db,
                current.config.as_ref(),
                config,
                request.plugin.as_ref().map(|scope| &scope.package),
            )
            .await?;
            let previous: Option<Vec<u8>> = {
                let mut statement = db
                    .prepare("SELECT config FROM defaults WHERE singleton=1")
                    .await
                    .map_err(storage_error)?;
                match statement.query_row(()).await {
                    Ok(row) => {
                        let mapped: turso::Result<_> = row.get(0);
                        Some(mapped.map_err(storage_error)?)
                    }
                    Err(turso::Error::QueryReturnedNoRows) => None,
                    Err(error) => return Err(storage_error(error)),
                }
            };
            let defaults = Defaults {
                revision: next_revision(current.revision)?,
                config: Some(config.clone()),
            };
            db.execute("INSERT INTO defaults(singleton,revision,config) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET revision=excluded.revision,config=excluded.config", params![defaults.revision as i64, super::plugins::options::preserve(previous.as_deref(),config)?]).await.map_err(storage_error)?;
            Ok((
                Output::Defaults(defaults.clone()),
                Some(Event::DefaultsChanged(defaults)),
            ))
        }),
        Command::BindConnectionSession {
            session,
            expected_revision,
            resource,
        } => defer(move || async move {
            super::connections::bind(db, profile, *session, *expected_revision, *resource).await
        }),
        Command::CreateSession {
            project,
            worktree,
            config,
        } => defer(move || async move {
            let config = match config {
                Some(config) => config.clone(),
                None => defaults(db).await?.config.ok_or_else(|| {
                    sailry_protocol::Fault::invalid("Node model defaults are not configured")
                })?,
            };
            let roles =
                super::sessions::roles::capture(db, node, super::roles::list(db).await?).await?;
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
            .await
        }),
        Command::ImportSession(bundle) => defer(move || async move {
            super::sessions::import(db, node, caller, profile, bundle).await
        }),
        Command::RenameSession {
            session,
            expected_revision,
            title,
        } => defer(move || async move {
            super::sessions::rename(db, *session, *expected_revision, title).await
        }),
        Command::SetSessionRead {
            session,
            expected_revision,
            read,
        } => defer(move || async move {
            super::sessions::attention::set(db, *session, *expected_revision, *read).await
        }),
        Command::SetSessionArchived {
            session,
            expected_revision,
            archived,
        } => defer(move || async move {
            super::sessions::archive(db, *session, *expected_revision, *archived).await
        }),
        Command::ArchiveSessions { .. } | Command::RemoveSessions { .. } => {
            defer(move || async move {
                Err(Fault::invalid(
                    "session batch requires its command transaction",
                ))
            })
        }
        Command::SetSessionOrder {
            project,
            expected,
            sessions,
        } => defer(move || async move {
            super::sessions::reorder(db, *project, expected, sessions).await
        }),
        Command::RemoveSession {
            session,
            expected_revision,
        } => {
            defer(
                move || async move { super::sessions::remove(db, *session, *expected_revision).await },
            )
        }
        Command::CreateSessionAt { .. } => defer(move || async move {
            Err(sailry_protocol::Fault::invalid(
                "configuration forwarding requires the Node worker",
            ))
        }),
        Command::MoveConversation {
            session,
            worktree,
            expected_revision,
        } => defer(move || async move {
            super::sessions::location::move_to(db, *session, *worktree, *expected_revision).await
        }),
        Command::ForkConversationAt {
            session,
            worktree,
            expected_revision,
        } => defer(move || async move {
            super::sessions::location::fork_at(db, *session, *worktree, *expected_revision).await
        }),
        Command::SetSessionConfig {
            session: id,
            expected_revision,
            config,
        } => defer(move || async move {
            validate_config(db, node, config).await?;
            let mut session = session(db, *id).await?;
            check_revision(session.revision, *expected_revision)?;
            super::plugins::options::validate(
                db,
                Some(&session.config),
                config,
                request.plugin.as_ref().map(|scope| &scope.package),
            )
            .await?;
            if session.delegation.is_some()
                && config.mode != sailry_protocol::WorkMode::Plan
                && super::agent::delegation::read_only(db, session.id).await?
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "read-only child mode is immutable",
                ));
            }
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
            super::sessions::validate_model(db, config, session.profile.as_ref()).await?;
            session.config = config.clone();
            revise_session(db, session).await
        }),
        Command::SetSessionRoles {
            session: id,
            expected_revision,
            roles,
        } => defer(move || async move {
            super::roles::approvals::check(db, request).await?;
            let mut session = session(db, *id).await?;
            super::sessions::writable(&session)?;
            check_revision(session.revision, *expected_revision)?;
            session.roles = super::sessions::roles::select(db, node, roles).await?;
            revise_session(db, session).await
        }),
        Command::CompactContext {
            session: id,
            expected_revision,
        } => defer(move || async move {
            let session = session(db, *id).await?;
            check_revision(session.revision, *expected_revision)?;
            let turn = super::agent::queue::compact(db, caller, request, session).await?;
            Ok((
                Output::QueuedTurn(turn.clone()),
                Some(super::agent::activity::queued(db, turn).await?),
            ))
        }),
        Command::ContinueTurn { .. } => {
            defer(
                move || async move { super::agent::continuation::admit(db, caller, request).await },
            )
        }
        Command::SubmitTask {
            session: id,
            expected_revision,
            message,
        } => defer(move || async move {
            if request.plugin.is_some() {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "task submission requires a native controller",
                ));
            }
            let session = session(db, *id).await?;
            if let Some(revision) = expected_revision {
                check_revision(session.revision, *revision)?;
            }
            let turn =
                super::agent::queue::create(db, caller, request, session, message, true).await?;
            Ok((
                Output::QueuedTurn(turn.clone()),
                Some(super::agent::activity::queued(db, turn).await?),
            ))
        }),
        Command::QueueTurn {
            session: id,
            expected_revision,
            message,
        }
        | Command::SubmitTurn {
            session: id,
            expected_revision,
            message,
        } => defer(move || async move {
            let session = session(db, *id).await?;
            check_revision(session.revision, *expected_revision)?;
            let resume = matches!(request.command, Command::SubmitTurn { .. })
                && super::agent::queue::can_resume(db, session.id).await?;
            let turn = super::agent::queue::create(
                db,
                caller,
                request,
                session,
                message,
                matches!(request.command, Command::SubmitTurn { .. }),
            )
            .await?;
            if resume {
                super::agent::queue::prioritize(db, &turn).await?;
            }
            Ok((
                Output::QueuedTurn(turn.clone()),
                Some(super::agent::activity::queued(db, turn).await?),
            ))
        }),
    }
}

// Keep alternative handler constructors off the same synchronous stack frame.
#[inline(never)]
fn defer<'a, F>(
    factory: impl FnOnce() -> F + Send + 'a,
) -> sailry_link::Pending<'a, Result<(Output, Option<Event>), Fault>>
where
    F: std::future::Future<Output = Result<(Output, Option<Event>), Fault>> + Send + 'a,
{
    Box::pin(async move { factory().await })
}

pub(super) async fn validate_config(
    db: &Connection,
    node: NodeId,
    config: &SessionConfig,
) -> Result<(), Fault> {
    if config.model.trim().is_empty() || config.model.len() > 256 {
        return Err(sailry_protocol::Fault::invalid(
            "model identifier is required and must be bounded",
        ));
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
        super::providers::authentication::validate(db, reference.id, config.provider).await?;
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
        .ok_or_else(|| sailry_protocol::Fault::invalid("configuration revision exhausted"))
}

pub(super) async fn revise_session(
    db: &Connection,
    mut session: Session,
) -> Result<(Output, Option<Event>), Fault> {
    super::sessions::writable(&session)?;
    session.revision = next_revision(session.revision)?;
    insert_revision(db, &session).await?;
    db.execute(
        "UPDATE sessions SET revision=?2,worktree=?3 WHERE id=?1",
        params![
            session.id.to_string(),
            session.revision as i64,
            session.worktree.to_string()
        ],
    )
    .await
    .map_err(storage_error)?;
    Ok((
        Output::Session(session.clone()),
        Some(Event::SessionChanged(Box::new(session))),
    ))
}

pub(super) async fn insert_revision(db: &Connection, session: &Session) -> Result<(), Fault> {
    let previous: Option<Vec<u8>> = {
        let mut statement = db.prepare("SELECT config FROM session_revisions WHERE session=?1 ORDER BY revision DESC LIMIT 1").await.map_err(storage_error)?;
        match statement.query_row([session.id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    db.execute(
        "INSERT INTO session_revisions(session,revision,config,profile,roles,worktree) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            session.id.to_string(),
            session.revision as i64,
            super::plugins::options::preserve(previous.as_deref(),&session.config)?,
            session.profile.as_ref().map(encode).transpose()?,
            encode(&session.roles)?,
            session.worktree.to_string()
        ],
    ).await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn defaults(db: &Connection) -> Result<Defaults, Fault> {
    let row: Option<(i64, Vec<u8>)> = {
        let mut statement = db
            .prepare("SELECT revision,config FROM defaults WHERE singleton=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(()).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
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

pub(super) async fn session(db: &Connection, id: SessionId) -> Result<Session, Fault> {
    type StoredSession = (i64, Option<String>, i64, Vec<u8>, String, bool);
    let row: Option<StoredSession> = {
        let mut statement = db.prepare("SELECT s.rowid,s.project,s.revision,r.config,s.worktree,s.state='archived' FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision WHERE s.id=?1 AND s.state!='removed'").await.map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                })(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let (creation_order, project, revision, config, worktree, archived) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "session does not exist on this Node"))?;
    Ok(Session {
        id,
        creation_order: creation_order.try_into().map_err(storage_error)?,
        archived,
        activity: super::agent::activity::read(db, id).await?,
        project: project
            .map(|id| id.parse())
            .transpose()
            .map_err(storage_error)?,
        worktree: worktree.parse().map_err(storage_error)?,
        revision: revision.try_into().map_err(storage_error)?,
        config: serde_json::from_slice(&config).map_err(storage_error)?,
        roles: super::sessions::roles::read(db, id, revision.try_into().map_err(storage_error)?)
            .await?,
        profile: super::sessions::profile(db, id, revision.try_into().map_err(storage_error)?)
            .await?,
        fork: super::sessions::origin(db, id).await?,
        delegation: super::agent::delegation(db, id).await?,
    })
}

async fn snapshot(db: &Connection, node: NodeId) -> Result<Snapshot, Fault> {
    let projects = super::projects::list(db).await?;
    let sessions = {
        let mut selected = Vec::new();
        for value in super::sessions::ordered(db).await?.into_iter() {
            selected.push(
                async {
                    let id = value;
                    session(db, id).await
                }
                .await?,
            );
        }
        Ok::<_, Fault>(selected)
    }?;
    let turns = super::captures::list(db).await?;
    let cursor = {
        let mut statement = db
            .prepare("SELECT coalesce(max(cursor),0) FROM events")
            .await
            .map_err(storage_error)?;
        let row = statement.query_row(()).await.map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get::<i64>(0);
        mapped.map_err(storage_error)?
    };
    Ok(Snapshot {
        node,
        worktrees: super::worktrees::list(db).await?,
        terminals: super::terminals::list(db).await?,
        power_settings_revision: super::power::read(db).await?.revision,
        terminal_settings_revision: super::terminals::settings::read(db).await?.revision,
        sandbox_settings_revision: super::sandbox::read(db).await?.revision,
        search_settings_revision: super::search::read(db).await?.revision,
        media_settings: super::media::read(db).await?,
        providers: super::agent::providers(db).await?,
        provider_quotas: super::providers::quota::list(db, node).await?,
        model_catalog: super::providers::catalog::status(db).await?,
        roles: super::roles::list(db).await?,
        ssh: super::ssh::list(db).await?,
        databases: super::databases::list(db).await?,
        plugins: super::plugins::list(db).await?,
        notifications: super::notifications::list(db).await?,
        cursor: cursor.try_into().map_err(storage_error)?,
        defaults: defaults(db).await?,
        projects,
        sessions,
        turns,
    })
}
