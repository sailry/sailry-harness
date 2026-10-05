mod admission;
pub(crate) mod agent;
mod attachments;
mod commands;
mod connections;
mod database;
mod databases;
pub(crate) mod dispatch;
mod external;
mod forwarding;
mod login;
mod mcp;
pub(crate) mod media;
mod mutations;
mod notifications;
mod outcomes;
mod plugins;
mod ports;
mod processes;
mod projects;
mod providers;
mod reads;
mod resources;
mod roles;
mod sessions;
mod ssh;
mod terminals;
mod transfers;
mod worktrees;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use sailry_link::{Admission, CancellationToken, Handler, PeerStore, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request, Update};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::Error;
use database::Database;

const CAPACITY: usize = 64;

enum Job {
    DispatchTick {
        recover: bool,
        reply: oneshot::Sender<Result<dispatch::Batch, Fault>>,
    },
    DispatchFinished {
        id: sailry_protocol::JobId,
        result: Result<(), Fault>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    McpCredentials(mcp::Operation),
    Authorization {
        reference: sailry_protocol::CredentialRef,
        provider: sailry_protocol::ProviderId,
        authentication: sailry_protocol::Authentication,
        reply: oneshot::Sender<Result<crate::providers::login::Grant, Fault>>,
    },
    Login(Box<login::Progress>),
    SubscribeLogin {
        caller: NodeId,
        id: sailry_protocol::RequestId,
        mcp: bool,
        reply: oneshot::Sender<Result<Box<dyn Subscription>, Fault>>,
    },
    Agent(Box<agent::Operation>),
    TerminalInfo(sailry_protocol::terminal::Info),
    Request {
        caller: NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<Result<Admission, Fault>>,
    },
    Shutdown,
    Completed(Box<external::Completed>),
    Transferred(Box<external::Completed>),
    Executed(Box<external::Completed>),
    Resources(Box<external::Completed>),
    PluginExecution(Box<external::Completed>),
    Ssh(Box<external::Completed>),
    Database(Box<external::Completed>),
    BindLink {
        link: sailry_link::LinkHandle,
        reply: oneshot::Sender<()>,
    },
    ValidateRemoval {
        id: sailry_protocol::WorktreeId,
        root: PathBuf,
        reply: std::sync::mpsc::Sender<Result<(), Fault>>,
    },
    ValidateRelocation {
        paths: Vec<(PathBuf, String)>,
        reply: std::sync::mpsc::Sender<Result<(), Fault>>,
    },
    RestoreCheckpoint {
        session: sailry_protocol::SessionId,
        checkpoint: sailry_protocol::CheckpointId,
        worktree: sailry_protocol::WorktreeId,
        root: PathBuf,
        reply: std::sync::mpsc::Sender<
            Result<sailry_protocol::conversation::checkpoint::Content, Fault>,
        >,
    },
    WorktreeRoot {
        id: sailry_protocol::WorktreeId,
        reply: oneshot::Sender<Result<PathBuf, Fault>>,
    },
    TerminalSettings {
        reply: oneshot::Sender<Result<sailry_protocol::terminal::Settings, Fault>>,
    },
    TerminalOpening {
        id: sailry_protocol::TerminalId,
        reply: oneshot::Sender<Result<terminals::Opening, Fault>>,
    },
    CheckPlugin {
        caller: NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    PluginView {
        surface: sailry_protocol::plugin::desktop::Surface,
        package: sailry_protocol::plugin::Reference,
        reply: oneshot::Sender<Result<sailry_protocol::plugin::Info, Fault>>,
    },
    PluginUpdate {
        name: String,
        revision: u64,
        reply: oneshot::Sender<Result<sailry_protocol::plugin::Info, Fault>>,
    },
    ResolveCredential {
        reference: sailry_protocol::CredentialRef,
        provider: sailry_protocol::ProviderId,
        reply: oneshot::Sender<Result<sailry_protocol::Secret, Fault>>,
    },
    Provider {
        id: sailry_protocol::ProviderId,
        revision: u64,
        reply: oneshot::Sender<Result<sailry_protocol::conversation::Provider, Fault>>,
    },
    Catalog {
        download: crate::providers::catalog::Download,
        reply: oneshot::Sender<Result<sailry_protocol::conversation::catalog::Status, Fault>>,
    },
    Peers(oneshot::Sender<Result<Vec<NodeId>, Fault>>),
    Addresses(oneshot::Sender<Result<Vec<sailry_link::EndpointAddr>, Fault>>),
    Remember {
        address: sailry_link::EndpointAddr,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    SetTrust {
        peer: NodeId,
        trusted: bool,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
}

#[derive(Clone)]
pub(crate) struct Ingress {
    pub(crate) node: NodeId,
    profile: Option<PathBuf>,
    host: Arc<crate::host::Host>,
    discovery: crate::providers::Discovery,
    catalog: crate::providers::catalog::Catalog,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) authorization_endpoint: Option<String>,
    pub(crate) files: crate::files::Files,
    watches: Arc<crate::files::watch::Watches>,
    transfers: Arc<crate::files::transfers::Transfers>,
    ports: Arc<crate::ports::Ports>,
    pub(crate) browsers: Arc<crate::browser::Browsers>,
    pub(crate) computer: Arc<crate::computer::Desktop>,
    pub(crate) external_browser: Arc<crate::external_browser::Browsers>,
    git: crate::git::Git,
    terminals: Arc<crate::terminal::Terminals>,
    commands: Arc<crate::process::background::Commands>,
    sender: mpsc::Sender<Job>,
    events: broadcast::Sender<sailry_protocol::EventEnvelope>,
    accepting: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    pub(crate) agents: Arc<crate::agent::Controls>,
    pub(crate) dispatch: Arc<crate::dispatch::Controls>,
    pub(crate) plugins: crate::plugins::Host,
    closed: CancellationToken,
}

pub(crate) struct Store {
    pub(crate) ingress: Arc<Ingress>,
    worker: Option<std::thread::JoinHandle<Result<(), Error>>>,
}

impl Store {
    pub(crate) async fn open(
        profile: PathBuf,
        node: NodeId,
        catalog: crate::providers::catalog::Catalog,
        authorization: crate::providers::login::Service,
        skills: crate::plugins::github::Github,
    ) -> Result<Self, Error> {
        Self::start(
            profile.join("storage/node.sqlite3"),
            node,
            Some(profile),
            catalog,
            authorization,
            skills,
            mutations::execute,
        )
        .await
    }

    async fn start(
        path: PathBuf,
        node: NodeId,
        profile: Option<PathBuf>,
        catalog: crate::providers::catalog::Catalog,
        authorization: crate::providers::login::Service,
        skills: crate::plugins::github::Github,
        execute: impl Fn(&mutations::Roots, &Request) -> sailry_link::Response + Send + 'static,
    ) -> Result<Self, Error> {
        let (sender, mut receiver) = mpsc::channel(CAPACITY);
        let (events, _) = broadcast::channel(CAPACITY);
        let accepting = Arc::new(AtomicBool::new(true));
        let closing = Arc::new(AtomicBool::new(false));
        let agents = Arc::new(crate::agent::Controls::default());
        let lifecycle = sender.clone();
        let terminals = crate::terminal::Terminals::new(node, move |info| {
            let _ = lifecycle.blocking_send(Job::TerminalInfo(info));
        });
        let ingress = Arc::new(Ingress {
            node,
            profile: profile.clone(),
            host: crate::host::Host::new(),
            discovery: crate::providers::Discovery::new(),
            catalog,
            files: crate::files::Files::new(),
            #[cfg(any(test, feature = "test-support"))]
            authorization_endpoint: authorization.model_endpoint.clone(),
            watches: Arc::new(crate::files::watch::Watches::default()),
            transfers: crate::files::transfers::Transfers::new(profile.clone()),
            ports: Arc::new(crate::ports::Ports::default()),
            browsers: Arc::default(),
            external_browser: Arc::new(crate::external_browser::Browsers::new(profile.clone())),
            computer: Arc::new(crate::computer::Desktop::default()),
            git: crate::git::Git::new(),
            terminals: terminals.clone(),
            commands: Arc::default(),
            sender,
            events: events.clone(),
            accepting: accepting.clone(),
            closing: closing.clone(),
            agents: agents.clone(),
            plugins: crate::plugins::Host::new(profile.clone()).with_github(skills),
            closed: CancellationToken::new(),
            dispatch: Arc::new(crate::dispatch::Controls::default()),
        });
        let (ready, initialized) = oneshot::channel();
        let completed = ingress.sender.clone();
        let validation = ingress.sender.clone();
        let watches = ingress.watches.clone();
        let transfers = ingress.transfers.clone();
        let plugins = ingress.plugins.clone();
        let collection = plugins.clone();
        let git = ingress.git.clone();
        let commands = ingress.commands.clone();
        let process_runtime = tokio::runtime::Handle::current();
        let cleanup_runtime = tokio::runtime::Handle::current();
        let computer = ingress.computer.clone();
        let attachment_profile = profile.clone();
        let mut forwarder = forwarding::Forwarder::new(
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            ingress.closed.clone(),
        );
        let mut processes = processes::Worker::new(
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            ingress.closed.clone(),
            agents.clone(),
            ingress.commands.clone(),
        );
        let mut plugin_work = plugins::execution::Worker::new(ingress.clone());
        let mut resource_work = resources::Worker::new(ingress.clone());
        let mut logins = login::Worker::new(
            authorization,
            ingress.discovery.clone(),
            plugins.clone(),
            ingress.sender.clone(),
            ingress.closed.clone(),
        );
        let mut ssh = ssh::worker::Worker::new(
            ingress.transfers.clone(),
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            ingress.closed.clone(),
            terminals.clone(),
        );
        let mut databases = databases::worker::Worker::new(
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            ingress.closed.clone(),
        );
        let worker = std::thread::Builder::new()
            .name("sailry-store".into())
            .spawn(move || {
                let mut database = match Database::open(&path, node, profile) {
                    Ok(database) => database,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return Ok(());
                    }
                };
                if database.profile.is_some()
                    && let Err(error) =
                        plugins::distribution::install(&mut database.connection, &collection)
                {
                    let _ = ready.send(Err(crate::Error::from(error)));
                    return Ok(());
                }
                plugins::collection::run(&mut database.connection, &collection);
                attachments::collect(&database.connection, attachment_profile.as_deref());
                let mut mutations =
                    match mutations::Worker::start(completed, move |caller, roots, request| {
                        let root = &roots.target;
                        match &request.command {
                            sailry_protocol::Command::InstallPluginUpload {
                                stream, name, ..
                            } => {
                                return transfers
                                    .install_plugin(caller, *stream, name, &plugins)
                                    .map(sailry_protocol::Output::Plugin);
                            }
                            sailry_protocol::Command::InstallPlugin { path, name, .. } => {
                                return plugins
                                    .install(root, path, name)
                                    .map(sailry_protocol::Output::Plugin);
                            }
                            sailry_protocol::Command::InstallPluginSource {
                                source,
                                path,
                                name,
                                ..
                            } => {
                                return process_runtime
                                    .block_on(plugins.install_source(source, path, name))
                                    .map(sailry_protocol::Output::Plugin);
                            }
                            sailry_protocol::Command::InstallBundledPlugin { name, .. } => {
                                return plugins
                                    .install_bundled(name)
                                    .map(sailry_protocol::Output::Plugin);
                            }
                            sailry_protocol::Command::InstallSkill {
                                source, path, name, ..
                            } => {
                                return process_runtime
                                    .block_on(plugins.install_skill(source, path, name))
                                    .map(sailry_protocol::Output::Plugin);
                            }
                            sailry_protocol::Command::InstallMcp {
                                name, definition, ..
                            } => {
                                return plugins
                                    .install_mcp(name, definition)
                                    .map(sailry_protocol::Output::Plugin);
                            }
                            sailry_protocol::Command::SavePluginMcp {
                                package,
                                configuration,
                            } => {
                                return plugins
                                    .validate_mcp(package, configuration)
                                    .map(|()| sailry_protocol::Output::PluginMcp(
                                        sailry_protocol::plugin::mcp::State {
                                            package: package.clone(),
                                            configuration: configuration.clone(),
                                        },
                                    ));
                            }
                            sailry_protocol::Command::FinishAttachmentUpload {
                                worktree,
                                stream,
                            } => {
                                let (reply, response) = oneshot::channel();
                                validation
                                    .blocking_send(Job::WorktreeRoot {
                                        id: *worktree,
                                        reply,
                                    })
                                    .map_err(|_| unavailable())?;
                                if response.blocking_recv().map_err(|_| unavailable())?? != *root {
                                    return Err(unavailable());
                                }
                                return transfers
                                    .commit_attachment(caller, root, *worktree, *stream)
                                    .map(sailry_protocol::Output::Attachment);
                            }
                            sailry_protocol::Command::RestoreFileCheckpoint { .. } => {
                                return agent::checkpoints::restore::execute(
                                    &validation,
                                    root,
                                    &request.command,
                                );
                            }
                            sailry_protocol::Command::OpenTerminal {
                                terminal,
                                viewport,
                                appearance,
                            } => {
                                if let Some(info) = terminals.existing(*terminal)? {
                                    return Ok(sailry_protocol::Output::Terminal(info));
                                }
                                let (reply, response) = oneshot::channel();
                                validation
                                    .blocking_send(Job::TerminalOpening {
                                        id: *terminal,
                                        reply,
                                    })
                                    .map_err(|_| unavailable())?;
                                let opening =
                                    response.blocking_recv().map_err(|_| unavailable())??;
                                return terminals
                                    .open(
                                        caller,
                                        &opening.info,
                                        viewport,
                                        appearance,
                                        crate::terminal::Source::Local {
                                            root: opening.root,
                                            settings: opening.settings,
                                            tool: None,
                                        },
                                    )
                                    .map(sailry_protocol::Output::Terminal);
                            }
                            sailry_protocol::Command::CreateTerminal(launch)
                            | sailry_protocol::Command::OpenToolTerminal { launch, .. } => {
                                let (reply, response) = oneshot::channel();
                                validation
                                    .blocking_send(Job::TerminalSettings { reply })
                                    .map_err(|_| unavailable())?;
                                let settings =
                                    response.blocking_recv().map_err(|_| unavailable())??;
                                return terminals
                                    .create(
                                        caller,
                                        launch,
                                        crate::terminal::Source::Local {
                                            root: root.to_owned(),
                                            settings,
                                            tool: match &request.command {
                                                sailry_protocol::Command::OpenToolTerminal {
                                                    tool,
                                                    ..
                                                } => Some(*tool),
                                                _ => None,
                                            },
                                        },
                                    )
                                    .map(sailry_protocol::Output::Terminal);
                            }
                            sailry_protocol::Command::StopCommands { session } => {
                                return Ok(sailry_protocol::Output::Commands(
                                    process_runtime.block_on(commands.close_session(*session)),
                                ));
                            }
                            sailry_protocol::Command::StopCommand { session, id } => {
                                return commands
                                    .stop(*session, *id)
                                    .map(sailry_protocol::Output::CommandOutput);
                            }
                            sailry_protocol::Command::CloseTerminal { worktree, terminal } => {
                                return terminals
                                    .close(*worktree, *terminal)
                                    .map(sailry_protocol::Output::Terminal);
                            }
                            sailry_protocol::Command::RemoveWorktree { worktree, .. }
                                if terminals.active(*worktree) =>
                            {
                                return Err(Fault::new(
                                    ErrorCode::Conflict,
                                    "worktree still owns running terminals",
                                ));
                            }
                            _ => {}
                        }
                        if let sailry_protocol::Command::RemoveWorktree { worktree, .. } =
                            request.command
                        {
                            // Earlier physical operations must finish registration before
                            // deletion checks ownership again on the authoritative store.
                            let (reply, response) = std::sync::mpsc::channel();
                            validation
                                .blocking_send(Job::ValidateRemoval {
                                    id: worktree,
                                    root: root.to_owned(),
                                    reply,
                                })
                                .map_err(|_| unavailable())?;
                            response.recv().map_err(|_| unavailable())??;
                            watches.remove(root);
                            transfers.remove(root);
                        }
                        let paths = roots.entries(&request.command);
                        if !paths.is_empty() {
                            let (reply, response) = std::sync::mpsc::channel();
                            validation
                                .blocking_send(Job::ValidateRelocation { paths, reply })
                                .map_err(|_| unavailable())?;
                            response.recv().map_err(|_| unavailable())??;
                        }
                        if let sailry_protocol::Command::FinishFileUpload {
                            worktree,
                            ref path,
                            stream,
                        } = request.command
                        {
                            transfers
                                .commit(caller, root, worktree, path, stream)
                                .map(sailry_protocol::Output::FileWritten)
                        } else {
                            git.capture(&roots.target, || {
                                let result = execute(roots, request);
                                crate::git::Git::record(&request.command, &result);
                                result
                            })
                        }
                    }) {
                        Ok(worker) => worker,
                        Err(error) => {
                            let _ = ready.send(Err(error));
                            return database.close();
                        }
                    };
                if ready.send(Ok(())).is_err() {
                    mutations.join()?;
                    return database.close();
                }
                while let Some(job) = receiver.blocking_recv() {
                    let package_change = match &job {
                        Job::Completed(_) => true,
                        Job::Request { request, .. } => matches!(request.command,
                            sailry_protocol::Command::SetPluginEnabled { .. } | sailry_protocol::Command::RemovePlugin { .. }),
                        _ => false,
                    };
                    match job {
                        Job::Authorization {
                            reference,
                            provider,
                            authentication,
                            reply,
                        } => {
                            if accepting.load(Ordering::Acquire) {
                                logins.authorize(
                                    &mut database,
                                    reference,
                                    provider,
                                    authentication,
                                    reply,
                                );
                            } else {
                                let _ = reply.send(Err(unavailable()));
                            }
                        }
                        Job::Login(progress) => {
                            logins.progress(&mut database, *progress, &events);
                        }
                        Job::McpCredentials(operation) => mcp::execute(&mut database, operation),
                        Job::SubscribeLogin {
                            caller,
                            id,
                            mcp,
                            reply,
                        } => {
                            let _ = reply.send(if mcp {
                                logins.subscribe_mcp(caller, id)
                            } else {
                                logins.subscribe(caller, id)
                            });
                        }
                        Job::BindLink { link, reply } => {
                            // The worker owns this handle, not Ingress: no Arc ownership cycle.
                            ssh.link = Some(link.clone());
                            forwarder.link = Some(link);
                            let _ = reply.send(());
                        }
                        Job::Agent(operation) => {
                            agent::execute(&mut database, *operation, &events, &agents);
                        }
                        Job::TerminalInfo(info) => {
                            if let Err(error) = database.terminal_changed(info, &events) {
                                eprintln!("terminal lifecycle persistence failed: {error}");
                            }
                        }
                        Job::Shutdown => {
                            accepting.store(false, Ordering::Release);
                            closing.store(true, Ordering::Release);
                        }
                        Job::Completed(completed) => {
                            let result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            let _ = completed.reply.send(result);
                            mutations.pending -= 1;
                        }
                        Job::Transferred(completed) => {
                            let result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            let _ = completed.reply.send(result);
                            forwarder.pending -= 1;
                        }
                        Job::PluginExecution(completed) => {
                            let result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            let _ = completed.reply.send(result);
                            if let Some(turn) = completed
                                .request
                                .plugin
                                .as_ref()
                                .and_then(|context| context.turn)
                            {
                                agents.finish_operation(turn);
                            }
                            plugin_work.finished(completed.caller, &completed.request);
                        }
                        Job::Resources(completed) => {
                            let result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            let _ = completed.reply.send(result);
                            if let Some(turn) = resources::turn(&completed.request) {
                                agents.finish_operation(turn);
                            }
                            resource_work.pending -= 1;
                        }
                        Job::Executed(completed) => {
                            let result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            let _ = completed.reply.send(result);
                            if let sailry_protocol::Command::RunCommand { turn, .. } =
                                completed.request.command
                            {
                                agents.finish_operation(turn);
                            }
                            processes.pending -= 1;
                        }
                        Job::Ssh(completed) => {
                            let result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            let _ = completed.reply.send(result);
                            ssh.finished(completed.caller, completed.request.id);
                        }
                        Job::Database(completed) => {
                            let result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            let _ = completed.reply.send(result);
                            databases.finished(completed.caller, completed.request.id);
                        }
                        Job::CheckPlugin {
                            caller,
                            request,
                            reply,
                        } => {
                            let _ = reply.send(database.check_plugin(caller, &request));
                        }
                        Job::PluginView {
                            package,
                            surface,
                            reply,
                        } => {
                            let _ = reply.send(plugins::views::current(
                                &database.connection,
                                &package,
                                surface,
                            ));
                        }
                        Job::PluginUpdate {
                            name,
                            revision,
                            reply,
                        } => {
                            let _ = reply.send(plugins::updates::current(
                                &database.connection,
                                &name,
                                revision,
                            ));
                        }
                        Job::Provider {
                            id,
                            revision,
                            reply,
                        } => {
                            let _ =
                                reply.send(providers::inspect(&database.connection, id, revision));
                        }
                        Job::Catalog { download, reply } => {
                            let _ = reply.send(database.cache_catalog(download, &events));
                        }
                        Job::WorktreeRoot { id, reply } => {
                            let _ = reply.send(database.worktree_root(id));
                        }
                        Job::TerminalSettings { reply } => {
                            let _ = reply.send(terminals::settings::read(&database.connection));
                        }
                        Job::TerminalOpening { id, reply } => {
                            let _ = reply.send(database.terminal_opening(id));
                        }
                        Job::ValidateRemoval { id, root, reply } => {
                            let _ = reply.send(database.validate_removal(id, &root));
                        }
                        Job::ValidateRelocation { paths, reply } => {
                            let _ = reply.send(database.validate_relocation(&paths).map(|_| ()));
                        }
                        Job::RestoreCheckpoint {
                            session,
                            checkpoint,
                            worktree,
                            root,
                            reply,
                        } => {
                            let _ = reply.send(agent::checkpoints::restore::prepare(
                                &database, session, checkpoint, worktree, &root,
                            ));
                        }
                        Job::ResolveCredential {
                            reference,
                            provider,
                            reply,
                        } => {
                            let result = if accepting.load(Ordering::Acquire) {
                                database.resolve_credential(
                                    &reference,
                                    provider,
                                    sailry_protocol::Authentication::ApiKey,
                                )
                            } else {
                                Err(unavailable())
                            };
                            let _ = reply.send(result);
                        }
                        Job::Peers(reply) => {
                            let _ = reply.send(database.peers());
                        }
                        Job::Addresses(reply) => {
                            let _ = reply.send(database.addresses());
                        }
                        Job::Remember { address, reply } => {
                            let result = if accepting.load(Ordering::Acquire) {
                                database.remember(address)
                            } else {
                                Err(unavailable())
                            };
                            let _ = reply.send(result);
                        }
                        Job::SetTrust {
                            peer,
                            trusted,
                            reply,
                        } => {
                            let result = if accepting.load(Ordering::Acquire) {
                                database.set_trust(peer, trusted)
                            } else {
                                Err(unavailable())
                            };
                            let _ = reply.send(result);
                        }
                        Job::DispatchTick { recover, reply } => {
                            let result = if accepting.load(Ordering::Acquire) {
                                dispatch::tick(&mut database, &events, recover)
                            } else {
                                Err(unavailable())
                            };
                            let _ = reply.send(result);
                        }
                        Job::DispatchFinished { id, result, reply } => {
                            let _ =
                                reply.send(dispatch::finish(&mut database, id, result, &events));
                        }
                        Job::Request {
                            caller,
                            request,
                            reply,
                        } => {
                            let reply = if matches!(request.command, sailry_protocol::Command::RemoveSession { .. }) {
                                admission::release_removed(computer.clone(), &cleanup_runtime, reply)
                            } else {
                                reply
                            };
                            let launch = matches!(
                                request.command,
                                sailry_protocol::Command::SubmitTurn { .. }
                                    | sailry_protocol::Command::ContinueTurn { .. }
                                    | sailry_protocol::Command::PluginTransaction { .. }
                                    | sailry_protocol::Command::StartSession(_)
                                    | sailry_protocol::Command::ReplaceTurn { .. }
                                    | sailry_protocol::Command::CompactContext { .. }
                                    | sailry_protocol::Command::StartQueuedTurn { .. }
                                    | sailry_protocol::Command::SendQueuedTurn { .. }
                                    | sailry_protocol::Command::SetQueuePaused { .. }
                            );
                            let sending = match request.command {
                                sailry_protocol::Command::SendQueuedTurn { turn, .. } => Some(turn),
                                _ => None,
                            };
                            let stopping = match &request.command {
                                sailry_protocol::Command::StopTurn { turn } => Some(*turn),
                                sailry_protocol::Command::StopDispatchTurn { package, job } => {
                                    dispatch::jobs::turn(&database.connection, &package.name, *job)
                                        .ok()
                                }
                                _ => None,
                            };
                            let transaction_stops = match &request.command {
                                sailry_protocol::Command::PluginTransaction { operations } => operations.iter().filter_map(|operation| {
                                    if let sailry_protocol::plugin::transaction::Operation::Stop { turn } = operation {Some(*turn)} else {None}
                                }).collect::<Vec<_>>(),
                                _ => Vec::new(),
                            };
                            if !accepting.load(Ordering::Acquire) {
                                let _ = reply.send(Err(unavailable()));
                            } else if let Some(work) =
                                database.dispatch(caller, *request, reply, &events)
                            {
                                let rejected = match work {
                                    external::Work::PluginExecution(execution) => {
                                        plugin_work.submit(execution).err().map(|execution| {
                                            (execution.caller, execution.request, execution.reply)
                                        })
                                    }
                                    external::Work::Login(operation) => {
                                        logins.execute(&mut database, operation, &events);
                                        None
                                    }
                                    external::Work::Resources(execution) => {
                                        resource_work.submit(execution).err().map(|execution| {
                                            (execution.caller, execution.request, execution.reply)
                                        })
                                    }
                                    external::Work::Process(execution) => {
                                        processes.submit(execution).err().map(|execution| {
                                            (execution.caller, execution.request, execution.reply)
                                        })
                                    }
                                    external::Work::Ssh(execution) => {
                                        ssh.submit(execution).err().map(|execution| {
                                            (execution.caller, execution.request, execution.reply)
                                        })
                                    }
                                    external::Work::Database(execution) => {
                                        databases.submit(execution).err().map(|execution| {
                                            (execution.caller, execution.request, execution.reply)
                                        })
                                    }
                                    external::Work::Mutation(mutation) => {
                                        mutations.submit(mutation).err().map(|mutation| {
                                            (mutation.caller, mutation.request, mutation.reply)
                                        })
                                    }
                                    external::Work::Session(transfer) => {
                                        forwarder.submit(transfer).err().map(|transfer| {
                                            (transfer.caller, transfer.request, transfer.reply)
                                        })
                                    }
                                };
                                if let Some((caller, request, reply)) = rejected {
                                    let result = database.finish_external(
                                        caller,
                                        &request,
                                        Err(Fault::new(
                                            ErrorCode::Busy,
                                            "Node operation queue is unavailable",
                                        )),
                                        &events,
                                    );
                                    let _ = reply.send(result);
                                }
                            }
                            if let Some(turn) = stopping
                                && agent::stopping(&database.connection, turn)
                            {
                                agents.cancel(turn);
                            }
                            for turn in transaction_stops {
                                if agent::stopping(&database.connection, turn) {agents.cancel(turn);}
                            }
                            if let Some(selected) = sending
                                && let Ok(turns) =
                                    agent::queue::stopping(&database.connection, selected)
                            {
                                for turn in turns {
                                    agents.cancel(turn);
                                }
                            }
                            if launch {
                                agents.wake.notify_waiters();
                            }
                        }
                    }
                    if package_change && let Ok(turns) = agent::continuation::stopping(&database.connection) {
                        for turn in turns {agents.cancel(turn);}
                        agents.wake.notify_waiters();
                    }
                    if closing.load(Ordering::Acquire)
                        && mutations.pending
                            + forwarder.pending
                            + processes.pending
                            + plugin_work.pending
                            + resource_work.pending
                            + logins.pending
                            + ssh.pending
                            + databases.pending
                            == 0
                    {
                        receiver.close();
                    }
                }
                mutations.join()?;
                plugins::collection::run(&mut database.connection, &collection);
                attachments::collect(&database.connection, attachment_profile.as_deref());
                database.close()
            })?;
        let store = Self {
            ingress,
            worker: Some(worker),
        };
        initialized
            .await
            .map_err(|error| Error::Worker(error.to_string()))??;
        Ok(store)
    }

    pub(crate) async fn bind_link(&self, link: sailry_link::LinkHandle) -> Result<(), Error> {
        let (reply, ready) = oneshot::channel();
        self.ingress
            .sender
            .send(Job::BindLink { link, reply })
            .await
            .map_err(|_| Error::Stopped)?;
        ready.await.map_err(|_| Error::Stopped)
    }

    pub(crate) fn stop_admission(&self) {
        self.ingress.dispatch.stop.cancel();
        self.ingress.accepting.store(false, Ordering::Release);
        self.ingress.closed.cancel();
        self.ingress.external_browser.stop.cancel();
        self.ingress.computer.stop.cancel();
        self.ingress.watches.shutdown();
        self.ingress.transfers.shutdown();
        self.ingress.ports.stop();
        self.ingress.terminals.stop();
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), Error> {
        self.stop_admission();
        let browsers = self.ingress.external_browser.shutdown().await;
        self.ingress.computer.shutdown().await;
        self.ingress.ports.join().await;
        self.ingress.commands.close().await;
        let terminals = self.ingress.terminals.clone();
        tokio::task::spawn_blocking(move || terminals.join())
            .await
            .map_err(|error| Error::Worker(error.to_string()))?;
        self.ingress.closing.store(true, Ordering::Release);
        let _ = self.ingress.sender.send(Job::Shutdown).await;
        let worker = self
            .worker
            .take()
            .expect("store owns its worker until shutdown");
        tokio::task::spawn_blocking(move || {
            worker
                .join()
                .map_err(|_| Error::Worker("storage worker panicked".into()))?
        })
        .await
        .map_err(|error| Error::Worker(error.to_string()))??;
        browsers.map_err(|error| Error::Worker(error.to_string()))
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.stop_admission();
        self.ingress.closing.store(true, Ordering::Release);
        // A full queue already wakes the worker, which observes the admission flag.
        let _ = self.ingress.sender.try_send(Job::Shutdown);
    }
}

impl Handler for Ingress {
    fn open(
        &self,
        caller: NodeId,
        resource: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        Box::pin(async move {
            if self.closed.is_cancelled() {
                return Err(unavailable());
            }
            if let Some(stream) = self.ports.open(caller, resource).await? {
                Ok(stream)
            } else {
                self.transfers.open(caller, resource)
            }
        })
    }

    fn dispatch(&self, caller: NodeId, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.dispatch_request(caller, request, false)
    }
    fn subscribe(
        &self,
        caller: NodeId,
        topic: sailry_protocol::Topic,
    ) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            if self.closed.is_cancelled() {
                return Err(unavailable());
            }
            if let sailry_protocol::Topic::ProviderLogin(id)
            | sailry_protocol::Topic::McpLogin(id) = topic
            {
                let (reply, response) = oneshot::channel();
                self.sender
                    .try_send(Job::SubscribeLogin {
                        caller,
                        id,
                        mcp: matches!(topic, sailry_protocol::Topic::McpLogin(_)),
                        reply,
                    })
                    .map_err(|_| unavailable())?;
                return response.await.map_err(|_| unavailable())?;
            }
            if let sailry_protocol::Topic::Commands(session) = topic {
                return Ok(self
                    .commands
                    .subscribe(self.node, session, self.closed.clone()));
            }
            if matches!(topic, sailry_protocol::Topic::Browser) {
                return self.browsers.subscribe(caller);
            }
            if let sailry_protocol::Topic::Terminal(id) = topic {
                return self.terminals.subscribe(id);
            }
            if let sailry_protocol::Topic::Conversation(id) = topic {
                return self.conversation(id).await;
            }
            if let sailry_protocol::Topic::Files(worktree) = topic {
                let root = self.worktree_root(worktree).await?;
                let expected = root.clone();
                let watches = self.watches.clone();
                let node = self.node;
                let closed = self.closed.child_token();
                let preparation = closed.clone().drop_guard();
                let subscription = tokio::task::spawn_blocking(move || {
                    watches.subscribe(root, node, worktree, closed)
                })
                .await
                .map_err(|_| unavailable())??;
                // A removal may have started after lookup but before watcher creation.
                if self.worktree_root(worktree).await? != expected {
                    return Err(unavailable());
                }
                preparation.disarm();
                return Ok(subscription);
            }
            // Subscribe before snapshotting so commits in between cannot be lost.
            let receiver = self.events.subscribe();
            let admission = self
                .dispatch(
                    caller,
                    Request::new(self.node, sailry_protocol::Command::Snapshot),
                )
                .await?;
            let output = admission.completion.await.map_err(|_| unavailable())??;
            let sailry_protocol::Output::Snapshot(snapshot) = output else {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "snapshot response expected",
                ));
            };
            Ok(Box::new(Events {
                initial: Some(Update::Snapshot(snapshot)),
                receiver,
                closed: self.closed.clone(),
            }) as Box<dyn Subscription>)
        })
    }
}

impl Ingress {
    pub(crate) async fn resolve_credential(
        &self,
        reference: sailry_protocol::CredentialRef,
        provider: sailry_protocol::ProviderId,
    ) -> Result<sailry_protocol::Secret, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::ResolveCredential {
                reference,
                provider,
                reply,
            })
            .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
}

impl PeerStore for Ingress {
    fn addresses(&self) -> Pending<'_, Result<Vec<sailry_link::EndpointAddr>, Fault>> {
        Box::pin(async move {
            let (reply, response) = oneshot::channel();
            self.sender
                .try_send(Job::Addresses(reply))
                .map_err(|_| unavailable())?;
            response.await.map_err(|_| unavailable())?
        })
    }

    fn remember(&self, address: sailry_link::EndpointAddr) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async move {
            let (reply, response) = oneshot::channel();
            self.sender
                .try_send(Job::Remember { address, reply })
                .map_err(|_| unavailable())?;
            response.await.map_err(|_| unavailable())?
        })
    }
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>> {
        Box::pin(async move {
            let (reply, response) = oneshot::channel();
            self.sender
                .try_send(Job::Peers(reply))
                .map_err(|_| unavailable())?;
            response.await.map_err(|_| unavailable())?
        })
    }

    fn set_trust(&self, peer: NodeId, trusted: bool) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async move {
            if !self.accepting.load(Ordering::Acquire) {
                return Err(unavailable());
            }
            let (reply, response) = oneshot::channel();
            self.sender
                .try_send(Job::SetTrust {
                    peer,
                    trusted,
                    reply,
                })
                .map_err(|_| unavailable())?;
            response.await.map_err(|_| unavailable())?
        })
    }
}

struct Events {
    initial: Option<Update>,
    receiver: broadcast::Receiver<sailry_protocol::EventEnvelope>,
    closed: CancellationToken,
}

impl Subscription for Events {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if self.closed.is_cancelled() {
                return Err(unavailable());
            }
            if let Some(initial) = self.initial.take() {
                return Ok(initial);
            }
            let result = tokio::select! {
                biased;
                _ = self.closed.cancelled() => return Err(unavailable()),
                result = self.receiver.recv() => result,
            };
            match result {
                Ok(event) => Ok(Update::Event(event)),
                Err(broadcast::error::RecvError::Lagged(_)) => Ok(Update::ResetRequired),
                Err(broadcast::error::RecvError::Closed) => Err(unavailable()),
            }
        })
    }
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "Node service is unavailable")
}

#[cfg(test)]
mod tests;
