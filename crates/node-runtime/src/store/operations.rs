//! SQL jobs execute with their own connection; only mutable execution services use the control loop.
use super::*;

pub(super) fn accepts(job: &Job) -> bool {
    if let Job::Agent(operation) = job {
        return agent::readonly(operation);
    }
    matches!(
        job,
        Job::Read { .. }
            | Job::CheckPlugin { .. }
            | Job::PowerFailed(..)
            | Job::BackupPlan { .. }
            | Job::LocalModels { .. }
            | Job::PythonSettings { .. }
            | Job::PluginPreparation { .. }
            | Job::KnowledgeCatalog { .. }
            | Job::KnowledgeAllowed { .. }
            | Job::KnowledgeProgress { .. }
            | Job::KnowledgeSources { .. }
            | Job::KnowledgeSourceCurrent { .. }
            | Job::KnowledgeSource { .. }
            | Job::KnowledgeSemanticPlan { .. }
            | Job::KnowledgeSemanticPage { .. }
            | Job::KnowledgeRegister { .. }
            | Job::McpCredentials(..)
            | Job::ConnectorTarget { .. }
            | Job::ConnectorGrant { .. }
            | Job::NetworkScope { .. }
            | Job::ProjectRemovalPlan { .. }
            | Job::ProjectTerminalClosed { .. }
            | Job::PluginView { .. }
            | Job::PluginUpdate { .. }
            | Job::ProviderHttp { .. }
            | Job::ModelAdapter { .. }
            | Job::CaptureChannel { .. }
            | Job::Provider { .. }
            | Job::ChannelPresets { .. }
            | Job::Catalog { .. }
            | Job::WorktreeRoot { .. }
            | Job::TerminalSettings { .. }
            | Job::SearchPlan { .. }
            | Job::TerminalOpening { .. }
            | Job::ValidateRemoval { .. }
            | Job::ValidateRelocation { .. }
            | Job::RestoreCheckpoint { .. }
            | Job::PrepareHunk { .. }
            | Job::ResolveCredential { .. }
            | Job::Remember { .. }
            | Job::SetTrust { .. }
            | Job::DispatchTick { .. }
            | Job::DispatchFinished { .. }
    )
}

pub(super) async fn execute(ingress: Ingress, job: Job) {
    let Some(storage) = ingress.storage.get() else {
        return;
    };
    // Each search phase releases its read snapshot before embedding can run.
    let job = match job {
        Job::KnowledgeSemanticPlan {
            request,
            stop,
            reply,
        } => {
            let result = storage
                .read(move |connection| {
                    Box::pin(async move {
                        knowledge::semantic::prepare(connection, &request, &stop).await
                    })
                })
                .await;
            let _ = reply.send(result);
            return;
        }
        Job::KnowledgeSemanticPage {
            request,
            plan,
            selection,
            stop,
            reply,
        } => {
            let result = storage
                .read(move |connection| {
                    Box::pin(async move {
                        knowledge::semantic::materialize(
                            connection, &request, &plan, &selection, &stop,
                        )
                        .await
                    })
                })
                .await;
            let _ = reply.send(result);
            return;
        }
        other => other,
    };
    let readonly = match &job {
        Job::Read { .. }
        | Job::CheckPlugin { .. }
        | Job::BackupPlan { .. }
        | Job::PythonSettings { .. }
        | Job::PluginPreparation { .. }
        | Job::KnowledgeCatalog { .. }
        | Job::KnowledgeAllowed { .. }
        | Job::KnowledgeSources { .. }
        | Job::KnowledgeSourceCurrent { .. }
        | Job::KnowledgeSource { .. }
        | Job::ConnectorTarget { .. }
        | Job::NetworkScope { .. }
        | Job::PluginView { .. }
        | Job::PluginUpdate { .. }
        | Job::ProviderHttp { .. }
        | Job::ModelAdapter { .. }
        | Job::CaptureChannel { .. }
        | Job::Provider { .. }
        | Job::WorktreeRoot { .. }
        | Job::TerminalSettings { .. }
        | Job::SearchPlan { .. }
        | Job::TerminalOpening { .. }
        | Job::ValidateRemoval { .. }
        | Job::ValidateRelocation { .. }
        | Job::RestoreCheckpoint { .. }
        | Job::PrepareHunk { .. }
        | Job::ResolveCredential { .. } => true,
        Job::Agent(operation) => agent::readonly(operation),
        Job::McpCredentials(operation) => !matches!(operation, mcp::Operation::Save { .. }),
        _ => false,
    };
    let mut database = match storage.context(readonly).await {
        Ok(database) => database,
        Err(_) => return,
    };
    let events = &ingress.events;
    let agents = &ingress.agents;
    let power = &ingress.power;
    let accepting = &ingress.accepting;
    let handling: Pending<'_, ()> = {
        let database = &mut database;
        match job {
            Job::Read {
                caller,
                request,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(database.read(caller, &request).await);
            }),
            Job::CheckPlugin {
                caller,
                request,
                reply,
            } => Box::pin(async move {
                let result = async {
                    let transaction = database
                        .connection
                        .unchecked_transaction()
                        .await
                        .map_err(database::storage_error)?;
                    match database.check_plugin(caller, &request).await {
                        Ok(()) => database::conflict::commit(transaction).await,
                        Err(error) => {
                            transaction
                                .rollback()
                                .await
                                .map_err(database::storage_error)?;
                            Err(error)
                        }
                    }
                }
                .await;
                let _ = reply.send(result);
            }),
            Job::PowerFailed(code) => Box::pin(async move {
                eprintln!(
                    "native idle-sleep request failed: {code}; status: {:?}",
                    power.status()
                );
                if let Err(error) = power::failed(database, code, events).await {
                    eprintln!("power failure notification persistence failed: {error}");
                }
            }),
            Job::BackupPlan { expected, reply } => Box::pin(async move {
                let _ = reply.send(backup::plan(&database.connection, expected).await);
            }),
            Job::LocalModels { models, reply } => Box::pin(async move {
                let _ = reply.send(local_models::models(database, models, events).await);
            }),
            Job::PythonSettings { reply } => Box::pin(async move {
                let _ = reply.send(python::read(&database.connection).await);
            }),
            Job::PluginPreparation {
                name,
                revision,
                reply,
            } => Box::pin(async move {
                let _ = reply
                    .send(plugins::prepare_enable(&database.connection, &name, revision).await);
            }),
            Job::KnowledgeCatalog { reply } => Box::pin(async move {
                let _ = reply.send(knowledge::catalog(&database.connection).await);
            }),
            Job::KnowledgeAllowed { request, reply } => Box::pin(async move {
                let _ = reply.send(knowledge::allowed(&database.connection, &request).await);
            }),
            Job::KnowledgeProgress {
                id,
                generation,
                progress,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    knowledge::runtime::progress(database, id, generation, &progress, events).await,
                );
            }),
            Job::KnowledgeSources {
                stored,
                after,
                limit,
                stop,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    knowledge::storage::page(
                        &database.connection,
                        &stored,
                        after.as_deref(),
                        limit,
                        &stop,
                    )
                    .await,
                );
            }),
            Job::KnowledgeSourceCurrent {
                stored,
                record,
                stop,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    knowledge::storage::unchanged(&database.connection, &stored, &record, &stop)
                        .await,
                );
            }),
            Job::KnowledgeSource {
                stored,
                key,
                stop,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    knowledge::storage::record(&database.connection, &stored, &key, &stop).await,
                );
            }),
            Job::KnowledgeRegister { stop, reply } => Box::pin(async move {
                let _ = reply
                    .send(knowledge::runtime::storage::register(database, &stop, events).await);
            }),
            Job::McpCredentials(operation) => {
                Box::pin(async move { mcp::execute(database, operation, events).await })
            }
            Job::ConnectorTarget {
                id,
                revision,
                checking,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    plugins::connectors::current_target(
                        &database.connection,
                        database.profile.as_deref(),
                        id,
                        revision,
                        checking,
                    )
                    .await,
                );
            }),
            Job::ConnectorGrant {
                id,
                revision,
                grant_revision,
                grant,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    plugins::connectors::save_grant(
                        database,
                        id,
                        revision,
                        grant_revision,
                        grant,
                        events,
                    )
                    .await,
                );
            }),
            Job::NetworkScope { fallback, reply } => Box::pin(async move {
                let _ = reply.send(network::scope(&database.connection, fallback).await);
            }),
            Job::Agent(operation) => Box::pin(async move {
                agent::execute(database, *operation, events, agents).await;
            }),
            Job::ProjectRemovalPlan {
                caller,
                request,
                expected,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    database
                        .prepare_project_removal(caller, request, &expected)
                        .await,
                );
            }),
            Job::ProjectTerminalClosed {
                worktree,
                terminal,
                result,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    database
                        .project_terminal_closed(worktree, terminal, result, events)
                        .await,
                );
            }),

            Job::PluginView {
                package,
                surface,
                reply,
            } => Box::pin(async move {
                let _ = reply
                    .send(plugins::views::current(&database.connection, &package, surface).await);
            }),
            Job::PluginUpdate {
                name,
                revision,
                reply,
            } => Box::pin(async move {
                let _ = reply
                    .send(plugins::updates::current(&database.connection, &name, revision).await);
            }),
            Job::ProviderHttp {
                caller,
                request,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    providers::resources::prepare(database, caller, &request)
                        .await
                        .map(Box::new),
                );
            }),
            Job::ModelAdapter {
                provider,
                messages,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    providers::adapters::read(&database.connection, &provider, messages).await,
                );
            }),
            Job::CaptureChannel {
                mut provider,
                reply,
            } => Box::pin(async move {
                let captured =
                    providers::presets::capture_read(&database.connection, &mut provider)
                        .await
                        .and_then(|()| {
                            crate::providers::login::capture(&mut provider)?;
                            Ok(*provider)
                        });
                let _ = reply.send(captured);
            }),
            Job::Provider {
                id,
                revision,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(providers::inspect(&database.connection, id, revision).await);
            }),
            Job::ChannelPresets {
                catalog,
                unavailable,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    providers::presets::list(&database.connection, catalog.as_ref(), unavailable)
                        .await,
                );
            }),
            Job::Catalog { download, reply } => Box::pin(async move {
                let _ = reply.send(database.cache_catalog(download, events).await);
            }),
            Job::WorktreeRoot { id, reply } => Box::pin(async move {
                let _ = reply.send(database.worktree_root(id).await);
            }),
            Job::TerminalSettings { reply } => Box::pin(async move {
                let _ = reply.send(terminals::settings::read(&database.connection).await);
            }),
            Job::SearchPlan { reply } => Box::pin(async move {
                let _ = reply.send(search::plan(&database.connection).await);
            }),
            Job::TerminalOpening { id, reply } => Box::pin(async move {
                let _ = reply.send(database.terminal_opening(id).await);
            }),
            Job::ValidateRemoval { id, root, reply } => Box::pin(async move {
                let _ = reply.send(database.validate_removal(id, &root).await);
            }),
            Job::ValidateRelocation {
                paths,
                reply,
                access,
            } => Box::pin(async move {
                let _ = reply.send(database.validate_with(&paths, access).await.map(|_| ()));
            }),
            Job::RestoreCheckpoint {
                session,
                checkpoint,
                worktree,
                root,
                reply,
            } => Box::pin(async move {
                let _ = reply.send(
                    agent::checkpoints::restore::prepare(
                        database, session, checkpoint, worktree, &root,
                    )
                    .await,
                );
            }),
            Job::PrepareHunk {
                selection,
                root,
                reply,
            } => Box::pin(async move {
                let _ = reply
                    .send(agent::checkpoints::hunks::prepare(database, &selection, &root).await);
            }),
            Job::ResolveCredential {
                reference,
                provider,
                reply,
            } => Box::pin(async move {
                let result = if accepting.load(Ordering::Acquire) {
                    database
                        .credential(
                            &reference,
                            provider,
                            sailry_protocol::Authentication::ApiKey,
                        )
                        .await
                } else {
                    Err(unavailable())
                };
                let _ = reply.send(result);
            }),
            Job::Remember { address, reply } => Box::pin(async move {
                let result = if accepting.load(Ordering::Acquire) {
                    database.remember(address).await
                } else {
                    Err(unavailable())
                };
                let _ = reply.send(result);
            }),
            Job::SetTrust {
                peer,
                trusted,
                reply,
            } => Box::pin(async move {
                let result = if accepting.load(Ordering::Acquire) {
                    database.set_trust(peer, trusted).await
                } else {
                    Err(unavailable())
                };
                let _ = reply.send(result);
            }),
            Job::DispatchTick { recover, reply } => Box::pin(async move {
                let result = if accepting.load(Ordering::Acquire) {
                    dispatch::tick(database, events, recover).await
                } else {
                    Err(unavailable())
                };
                let _ = reply.send(result);
            }),
            Job::DispatchFinished { id, result, reply } => Box::pin(async move {
                let _ = reply.send(dispatch::finish(database, id, result, events).await);
            }),
            _ => unreachable!("execution jobs stay with their owner"),
        }
    };
    handling.await;
    if let Err(error) = database::conflict::rollback(&database.connection).await {
        eprintln!("storage operation cleanup failed: {error}");
    }
}
