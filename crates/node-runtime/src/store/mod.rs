mod admission;
pub(crate) mod agent;
mod artifacts;
mod attachments;
mod backup;
mod code;
mod commands;
mod connections;
mod database;
mod databases;
pub(crate) mod dispatch;
mod external;
mod forwarding;
pub(crate) mod hooks;
pub(crate) mod knowledge;
mod login;
mod mcp;
pub(crate) mod media;
mod models;
mod mutations;
mod notifications;
mod ocr;
mod outcomes;
mod plugin_oauth;
mod plugins;
mod ports;
mod processes;
mod projects;
mod providers;
mod python;
mod reads;
mod resources;
mod roles;
mod sandbox;
mod search;
mod sessions;
mod ssh;
mod terminals;
mod transfers;
mod web;
mod worktrees;

use std::{
    path::PathBuf,
    sync::{
        Arc, OnceLock,
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
    #[cfg(test)]
    Barrier {
        entered: mpsc::UnboundedSender<()>,
        release: std::sync::mpsc::Receiver<()>,
    },
    BackupPlan {
        expected: Option<u64>,
        reply: oneshot::Sender<Result<crate::backup::Plan, Fault>>,
    },
    BackupSnapshot {
        revision: u64,
        cancel: sailry_link::CancellationToken,
        reply: oneshot::Sender<Result<backup::Capture, Fault>>,
    },
    OcrCatalog {
        reply: oneshot::Sender<Result<ocr::Catalog, Fault>>,
    },
    OcrModels {
        models: Vec<sailry_protocol::ocr::Model>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    PythonSettings {
        reply: oneshot::Sender<Result<sailry_protocol::python::Settings, Fault>>,
    },
    PluginPreparation {
        name: String,
        revision: u64,
        reply: oneshot::Sender<Result<sailry_protocol::plugin::Info, Fault>>,
    },
    KnowledgeCatalog {
        reply: oneshot::Sender<Result<knowledge::Catalog, Fault>>,
    },
    KnowledgeAllowed {
        request: Box<Request>,
        reply: oneshot::Sender<Result<Vec<knowledge::Stored>, Fault>>,
    },
    KnowledgeProgress {
        id: sailry_protocol::KnowledgeId,
        generation: u64,
        progress: sailry_protocol::knowledge::Progress,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    KnowledgeModels {
        models: Vec<sailry_protocol::knowledge::ModelInfo>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    KnowledgeSources {
        stored: Box<knowledge::Stored>,
        after: Option<String>,
        limit: u16,
        stop: CancellationToken,
        reply: oneshot::Sender<Result<knowledge::storage::PageRaw, Fault>>,
    },
    KnowledgeSourceCurrent {
        stored: Box<knowledge::Stored>,
        record: Box<knowledge::storage::Record>,
        stop: CancellationToken,
        reply: oneshot::Sender<Result<bool, Fault>>,
    },
    KnowledgeSource {
        stored: Box<knowledge::Stored>,
        key: String,
        stop: CancellationToken,
        reply: oneshot::Sender<Result<Option<knowledge::storage::Record>, Fault>>,
    },
    KnowledgeSemanticPlan {
        request: Box<Request>,
        stop: CancellationToken,
        reply: oneshot::Sender<Result<knowledge::semantic::Plan, Fault>>,
    },
    KnowledgeSemanticPage {
        request: Box<Request>,
        plan: Box<knowledge::semantic::Plan>,
        selection: Box<knowledge::semantic::Selection>,
        stop: CancellationToken,
        reply: oneshot::Sender<Result<knowledge::semantic::Materialized, Fault>>,
    },
    KnowledgeRegister {
        stop: CancellationToken,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
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
    PluginOAuth(plugin_oauth::Operation),
    Authorization {
        reference: sailry_protocol::CredentialRef,
        provider: sailry_protocol::ProviderId,
        authentication: sailry_protocol::Authentication,
        options: sailry_protocol::conversation::oauth::Options,
        reply: oneshot::Sender<Result<login::Authorization, Fault>>,
    },
    ProviderQuota(providers::quota::Observation),
    Login(Box<login::Progress>),
    SubscribeLogin {
        caller: NodeId,
        id: sailry_protocol::RequestId,
        mcp: bool,
        plugin: bool,
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
    Installed {
        completed: Box<external::Completed>,
        release: oneshot::Sender<()>,
    },
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
    PrepareHunk {
        selection: sailry_protocol::conversation::checkpoint::hunk::Selection,
        root: PathBuf,
        reply: std::sync::mpsc::Sender<Result<agent::checkpoints::hunks::Change, Fault>>,
    },
    WorktreeRoot {
        id: sailry_protocol::WorktreeId,
        reply: oneshot::Sender<Result<PathBuf, Fault>>,
    },
    TerminalSettings {
        reply: oneshot::Sender<Result<sailry_protocol::terminal::Settings, Fault>>,
    },
    SearchPlan {
        reply: oneshot::Sender<Result<crate::web::search::Plan, Fault>>,
    },
    GithubTarget {
        package: sailry_protocol::plugin::Reference,
        reply: oneshot::Sender<Result<Option<crate::plugins::github::login::Identity>, Fault>>,
    },
    TerminalOpening {
        id: sailry_protocol::TerminalId,
        reply: oneshot::Sender<Result<terminals::Opening, Fault>>,
    },
    CodeApproved {
        startup: sailry_protocol::code::Startup,
        reply: oneshot::Sender<Result<bool, Fault>>,
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
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) model_catalog_fixtures: Arc<crate::models::Fixtures>,
    pub(crate) files: crate::files::Files,
    watches: Arc<crate::files::watch::Watches>,
    transfers: Arc<crate::files::transfers::Transfers>,
    ports: Arc<crate::ports::Ports>,
    pub(crate) browsers: Arc<crate::browser::Browsers>,
    pub(crate) computer: Arc<crate::computer::Desktop>,
    pub(crate) external_browser: Arc<crate::external_browser::Browsers>,
    pub(crate) git: crate::git::Git,
    pub(crate) code: Arc<crate::code::Pool>,
    pub(crate) ocr: Arc<crate::ocr::Service>,
    pub(crate) knowledge: Arc<OnceLock<Arc<crate::knowledge::Service>>>,
    code_queries: Arc<crate::code::Queries>,
    pub(crate) web: Arc<crate::web::Fetcher>,
    pub(crate) search: Arc<crate::web::Searcher>,
    terminals: Arc<crate::terminal::Terminals>,
    commands: Arc<crate::process::background::Commands>,
    pub(crate) environment: Arc<crate::process::Environment>,
    sender: mpsc::Sender<Job>,
    events: broadcast::Sender<sailry_protocol::EventEnvelope>,
    accepting: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    pub(crate) agents: Arc<crate::agent::Controls>,
    feeds: agent::Feeds,
    pub(crate) dispatch: Arc<crate::dispatch::Controls>,
    pub(crate) plugins: crate::plugins::Host,
    pub(crate) oauth: Arc<crate::plugins::oauth::Service>,
    closed: CancellationToken,
}

struct Services {
    catalog: crate::providers::catalog::Catalog,
    authorization: crate::providers::login::Service,
    skills: crate::plugins::github::Github,
    environment: Arc<crate::process::Environment>,
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
        source: crate::process::environment::Source,
    ) -> Result<Self, Error> {
        let environment = Arc::new(crate::process::Environment::capture(source).await);
        Self::start_ready(
            profile.join("storage/node.sqlite3"),
            node,
            Some(profile),
            Services {
                catalog,
                authorization,
                skills,
                environment,
            },
            mutations::execute,
        )
        .await
    }

    #[cfg(test)]
    async fn start(
        path: PathBuf,
        node: NodeId,
        profile: Option<PathBuf>,
        catalog: crate::providers::catalog::Catalog,
        authorization: crate::providers::login::Service,
        skills: crate::plugins::github::Github,
        execute: impl Fn(&mutations::Roots, &Request) -> sailry_link::Response + Send + 'static,
    ) -> Result<Self, Error> {
        Self::start_ready(
            path,
            node,
            profile,
            Services {
                catalog,
                authorization,
                skills,
                environment: Arc::new(crate::process::Environment::ambient()),
            },
            execute,
        )
        .await
    }

    async fn start_ready(
        path: PathBuf,
        node: NodeId,
        profile: Option<PathBuf>,
        services: Services,
        execute: impl Fn(&mutations::Roots, &Request) -> sailry_link::Response + Send + 'static,
    ) -> Result<Self, Error> {
        let Services {
            catalog,
            authorization,
            skills,
            environment,
        } = services;
        let (sender, mut receiver) = mpsc::channel(CAPACITY);
        let (events, _) = broadcast::channel(CAPACITY);
        let accepting = Arc::new(AtomicBool::new(true));
        let closing = Arc::new(AtomicBool::new(false));
        let agents = Arc::new(crate::agent::Controls::default());
        let closed = CancellationToken::new();
        let lifecycle = sender.clone();
        let terminals = crate::terminal::Terminals::new(node, move |info| {
            let _ = lifecycle.blocking_send(Job::TerminalInfo(info));
        });
        let ocr = crate::ocr::Service::new(profile.clone(), closed.child_token());
        let ingress = Arc::new(Ingress {
            node,
            profile: profile.clone(),
            host: crate::host::Host::new(),
            discovery: crate::providers::Discovery::new(),
            catalog,
            files: crate::files::Files::new().with_ocr(ocr.clone()),
            oauth: Arc::default(),
            #[cfg(any(test, feature = "test-support"))]
            authorization_endpoint: authorization.model_endpoint.clone(),
            #[cfg(any(test, feature = "test-support"))]
            model_catalog_fixtures: Arc::default(),
            watches: Arc::new(crate::files::watch::Watches::default()),
            transfers: crate::files::transfers::Transfers::new(profile.clone()),
            ports: Arc::new(crate::ports::Ports::default()),
            browsers: Arc::default(),
            external_browser: Arc::new(crate::external_browser::Browsers::new(profile.clone())),
            computer: Arc::new(crate::computer::Desktop::default()),
            git: crate::git::Git::new(),
            code_queries: Arc::new(crate::code::Queries::default()),
            web: Arc::default(),
            search: Arc::default(),
            code: crate::code::Pool::new(profile.clone(), environment.clone(), closed.clone()),
            ocr,
            knowledge: Arc::new(OnceLock::new()),
            terminals: terminals.clone(),
            commands: Arc::default(),
            environment: environment.clone(),
            sender,
            events: events.clone(),
            accepting: accepting.clone(),
            closing: closing.clone(),
            agents: agents.clone(),
            feeds: Default::default(),
            plugins: crate::plugins::Host::new(profile.clone()).with_github(skills),
            closed,
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
        let cleanup_commands = ingress.commands.clone();
        let process_runtime = tokio::runtime::Handle::current();
        let cleanup_runtime = tokio::runtime::Handle::current();
        let computer = ingress.computer.clone();
        let attachment_profile = profile.clone();
        let terminal_profile = profile.clone();
        let mut forwarder = forwarding::Forwarder::new(
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            ingress.closed.clone(),
        );
        let mut processes = processes::Worker::new(
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            &ingress,
        );
        let mut plugin_work = plugins::execution::Worker::new(ingress.clone());
        let mut installations = plugins::installing::Worker::new(ingress.clone());
        let mut resource_work = resources::Worker::new(ingress.clone());
        let mut logins = login::Worker::new(authorization, &ingress, events.clone());
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
        let code_pool = ingress.code.clone();
        let searcher = ingress.search.clone();
        let feeds = ingress.feeds.clone();
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
                database.feeds = feeds;
                match search::read(&database.connection) { Ok(settings) => searcher.configure(settings), Err(error) => {let _=ready.send(Err(crate::Error::from(error)));return Ok(());} }
                if database.profile.is_some()
                    && let Err(error) =
                        plugins::distribution::install(&mut database.connection, &collection)
                {
                    let _ = ready.send(Err(crate::Error::from(error)));
                    return Ok(());
                }
                plugins::collection::run(&mut database.connection, &collection);
                if let Err(error) = plugins::retain_python(&database.connection, &collection) { let _ = ready.send(Err(crate::Error::from(error))); return Ok(()); }
                match code::initialize(&mut database.connection) {Ok(settings)=>cleanup_runtime.block_on(code_pool.configure(settings)),Err(error)=>{let _=ready.send(Err(crate::Error::from(error)));return Ok(());}}
                attachments::collect(&database.connection, attachment_profile.as_deref());
                let mut mutations =
                    match mutations::Worker::start(completed, move |caller, roots, request| {
                        let root = &roots.target;
                        match &request.command {
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
                            sailry_protocol::Command::ApplyTurnHunk(selection) => {
                                return agent::checkpoints::hunks::execute(
                                    &validation, root, selection, request.id,
                                ).map(sailry_protocol::Output::HunkApplied);
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
                                            environment: environment.clone(),
                                            profile: terminal_profile.clone(),
                                            access: opening.info.access.ok_or_else(|| sailry_protocol::Fault::invalid("local terminal has no access policy"))?,
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
                                            environment: environment.clone(),
                                            profile: terminal_profile.clone(),
                                            access: launch.access,
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
                            sailry_protocol::Command::WriteCommand { session, id, input } => {
                                return commands.write(*session, *id, input)
                                    .map(|bytes| sailry_protocol::Output::CommandInputAccepted { id: *id, bytes });
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
                        Job::Completed(_) | Job::Installed { .. } => true,
                        Job::Request { request, .. } => matches!(request.command,
                            sailry_protocol::Command::SetPluginEnabled { .. } | sailry_protocol::Command::RemovePlugin { .. }),
                        _ => false,
                    };
                    match job {
                        #[cfg(test)]
                        Job::Barrier { entered, release } => {
                            entered.send(()).unwrap();
                            release.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
                        }
                        Job::BackupPlan { expected, reply } => { let _ = reply.send(backup::plan(&database.connection, expected)); }
                        Job::BackupSnapshot { revision, cancel, reply } => { let _ = reply.send(backup::capture(&database, revision, &cancel)); }
                        Job::OcrCatalog { reply } => { let _ = reply.send(ocr::read(&database.connection)); }
                        Job::OcrModels { models, reply } => { let _ = reply.send(ocr::models(&mut database, models, &events)); }
                        Job::PythonSettings { reply } => {
                            let _ = reply.send(python::read(&database.connection));
                        }
                        Job::PluginPreparation { name, revision, reply } => {
                            let _ = reply.send(plugins::prepare_enable(&database.connection, &name, revision));
                        }
                        Job::KnowledgeCatalog { reply } => {
                            let _ = reply.send(knowledge::catalog(&database.connection));
                        }
                        Job::KnowledgeAllowed { request, reply } => {
                            let _ = reply.send(knowledge::allowed(&database.connection, &request));
                        }
                        Job::KnowledgeProgress { id, generation, progress, reply } => {
                            let _ = reply.send(knowledge::runtime::progress(&mut database, id, generation, &progress, &events));
                        }
                        Job::KnowledgeModels { models, reply } => {
                            let _ = reply.send(knowledge::runtime::models(&mut database, models, &events));
                        }
                        Job::KnowledgeSources { stored, after, limit, stop, reply } => {
                            let _ = reply.send(knowledge::storage::page(&database.connection, &stored, after.as_deref(), limit, &stop));
                        }
                        Job::KnowledgeSourceCurrent { stored, record, stop, reply } => {
                            let _ = reply.send(knowledge::storage::unchanged(&database.connection, &stored, &record, &stop));
                        }
                        Job::KnowledgeSource { stored, key, stop, reply } => {
                            let _ = reply.send(knowledge::storage::record(&database.connection, &stored, &key, &stop));
                        }
                        Job::KnowledgeSemanticPlan { request, stop, reply } => {
                            let _ = reply.send(knowledge::runtime::storage::prepare(&mut database, &request, &stop, &events));
                        }
                        Job::KnowledgeSemanticPage { request, plan, selection, stop, reply } => {
                            let _ = reply.send(knowledge::semantic::materialize(&database.connection, &request, &plan, &selection, &stop));
                        }
                        Job::KnowledgeRegister { stop, reply } => {
                            let _ = reply.send(knowledge::runtime::storage::register(&mut database, &stop, &events));
                        }
                        Job::Authorization {
                            reference,
                            provider,
                            authentication,
                            options,
                            reply,
                        } => {
                            if accepting.load(Ordering::Acquire) {
                                logins.authorize(
                                    &mut database,
                                    reference,
                                    provider,
                                    authentication,
                                    options,
                                    reply,
                                );
                            } else {
                                let _ = reply.send(Err(unavailable()));
                            }
                        }
                        Job::Login(progress) => {
                            logins.progress(&mut database, *progress, &events);
                        }
                        Job::ProviderQuota(observation) => {
                            let _ = providers::quota::observe(&mut database, observation, &events);
                        }
                        Job::McpCredentials(operation) => mcp::execute(&mut database, operation),
                        Job::PluginOAuth(operation) => plugin_oauth::execute(&mut database, operation),
                        Job::SubscribeLogin {
                            caller,
                            id,
                            mcp,
                            plugin,
                            reply,
                        } => {
                            let _ = reply.send(if plugin {
                                logins.subscribe_plugin(caller, id)
                            } else if mcp {
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
                            code::observe(&database,&completed.request,&completed.result,&code_pool,&cleanup_runtime);
                            let mut result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            if result.is_ok() && code::configures(&completed.request.command) {
                                match code::settings(&database.connection) {
                                    Ok(settings) => cleanup_runtime.block_on(code_pool.configure(settings)),
                                    Err(error) => result = Err(error),
                                }
                            }
                            let _ = completed.reply.send(result);
                            mutations.pending -= 1;
                        }
                        Job::Installed { completed, release } => {
                            let mut result = database.finish_external(
                                completed.caller,
                                &completed.request,
                                completed.result,
                                &events,
                            );
                            if result.is_ok() && code::configures(&completed.request.command) {
                                match code::settings(&database.connection) {
                                    Ok(settings) => cleanup_runtime.block_on(code_pool.configure(settings)),
                                    Err(error) => result = Err(error),
                                }
                            }
                            let _ = completed.reply.send(result);
                            installations.pending -= 1;
                            let _ = release.send(());
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
                            if matches!(completed.result, Ok(sailry_protocol::Output::CommandResult(_)))
                                && let sailry_protocol::Command::RunCommand { turn, .. } = &completed.request.command
                                && let Ok(worktree) = database.connection.query_row(
                                    "SELECT r.worktree FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1",
                                    [turn.to_string()], |row| row.get::<_, String>(0),
                                )
                                && let Ok(worktree) = worktree.parse()
                            {
                                let pool = code_pool.clone();
                                cleanup_runtime.spawn(async move { pool.watched(worktree).await; });
                            }
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
                        Job::CodeApproved {startup,reply}=>{let _=reply.send(code::approved(&database.connection,&startup));},
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
                        Job::SearchPlan { reply } => {
                            let _ = reply.send(search::plan(&database.connection));
                        }
                        Job::GithubTarget {package,reply} => {let _=reply.send(plugins::github::prepare(&database.connection,&collection,&package));}
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
                        Job::PrepareHunk { selection, root, reply } => {
                            let _ = reply.send(agent::checkpoints::hunks::prepare(
                                &database, &selection, &root,
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
                            let (reply, artifact_cleanup) = if matches!(request.command, sailry_protocol::Command::RemoveSession { .. }) {
                                let (send, cleanup) = oneshot::channel();
                                (admission::release_removed(computer.clone(), cleanup_commands.clone(), attachment_profile.clone(), &cleanup_runtime, reply, cleanup), Some(send))
                            } else {
                                (reply, None)
                            };
                            let code_settings = code::configures(&request.command);
                            let (reply, code_completion) = if code_settings {
                                let (complete, response) = oneshot::channel();
                                (complete, Some((reply, response)))
                            } else {
                                (reply, None)
                            };
                            let search_settings=matches!(request.command,sailry_protocol::Command::SaveSearchSettings(_));
                            let launch = matches!(
                                request.command,
                                sailry_protocol::Command::SubmitTurn { .. }
                                    | sailry_protocol::Command::SubmitTask { .. }
                                    | sailry_protocol::Command::ContinueTurn { .. }
                                    | sailry_protocol::Command::PluginTransaction { .. }
                                    | sailry_protocol::Command::StartSession(_)
                                    | sailry_protocol::Command::ReplaceTurn { .. }
                                    | sailry_protocol::Command::CompactContext { .. }
                                    | sailry_protocol::Command::StartQueuedTurn { .. }
                                    | sailry_protocol::Command::SendQueuedTurn { .. }
                                    | sailry_protocol::Command::GuideQueuedTurn { .. }
                                    | sailry_protocol::Command::GuideTurn { .. }
                                    | sailry_protocol::Command::SetQueuePaused { .. }
                            );
                            let guiding = match &request.command {
                                sailry_protocol::Command::GuideTurn { recipient, .. }
                                | sailry_protocol::Command::GuideQueuedTurn { recipient, .. } => {
                                    Some(*recipient)
                                }
                                _ => None,
                            };
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
                                        let result = if plugins::installing::handles(&mutation.request.command) {
                                            installations.submit(mutation)
                                        } else {
                                            mutations.submit(mutation)
                                        };
                                        result.err().map(|mutation| {
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
                            if let Some(cleanup) = artifact_cleanup {
                                let _ = cleanup.send(artifacts::cleanup(&database.connection));
                            }
                            if let Some((reply, completion)) = code_completion {
                                let result = completion.blocking_recv().unwrap_or_else(|_| Err(unavailable()));
                                if result.is_ok() {
                                    match code::settings(&database.connection) {
                                        Ok(settings) => cleanup_runtime.block_on(code_pool.configure(settings)),
                                        Err(error) => {let _ = reply.send(Err(error)); continue;}
                                    }
                                }
                                let _ = reply.send(result);
                            }
                            if search_settings && let Ok(settings)=search::read(&database.connection) {searcher.configure(settings);}
                            if let Some(turn) = stopping
                                && agent::stopping(&database.connection, turn)
                            {
                                agents.cancel(turn);
                            }
                            for turn in transaction_stops {
                                if agent::stopping(&database.connection, turn) {agents.cancel(turn);}
                            }
                            if let Some(recipient) = guiding
                                && let Err(error) = agents.interrupt_model(recipient, || {
                                    agent::queue::guidance::yield_run(&mut database, recipient, &events)
                                })
                            {
                                eprintln!("guidance could not interrupt the model response: {error}");
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
                    if package_change && let Err(error) = plugins::retain_python(&database.connection, &collection) {
                        eprintln!("Python package references could not be refreshed: {error}");
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
                            + installations.pending
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
        if let Err(error) = ocr::start(&store.ingress).await {
            let _ = store.shutdown().await;
            return Err(Error::from(error));
        }
        let configured = async {
            let settings = store.ingress.python_settings().await?;
            store.ingress.plugins.configure_python(settings.index)
        }
        .await;
        if let Err(error) = configured {
            let _ = store.shutdown().await;
            return Err(Error::from(error));
        }
        if let Err(error) = knowledge::runtime::start(&store.ingress).await {
            let _ = store.shutdown().await;
            return Err(Error::from(error));
        }
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
        self.ingress.ocr.shutdown();
        self.ingress.external_browser.stop.cancel();
        self.ingress.computer.stop.cancel();
        self.ingress.watches.shutdown();
        self.ingress.transfers.shutdown();
        self.ingress.ports.stop();
        self.ingress.terminals.stop();
    }

    pub(crate) async fn shutdown(mut self) -> Result<(), Error> {
        self.stop_admission();
        if let Some(knowledge) = self.ingress.knowledge.get() {
            knowledge.shutdown().await;
        }
        let browsers = self.ingress.external_browser.shutdown().await;
        self.ingress.computer.shutdown().await;
        self.ingress.ports.join().await;
        self.ingress.commands.close().await;
        self.ingress.code.shutdown().await;
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
            | sailry_protocol::Topic::McpLogin(id)
            | sailry_protocol::Topic::PluginLogin(id) = topic
            {
                let (reply, response) = oneshot::channel();
                self.sender
                    .try_send(Job::SubscribeLogin {
                        caller,
                        id,
                        mcp: matches!(topic, sailry_protocol::Topic::McpLogin(_)),
                        plugin: matches!(topic, sailry_protocol::Topic::PluginLogin(_)),
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
            if matches!(topic, sailry_protocol::Topic::Python) {
                return Ok(python::subscribe(
                    self.node,
                    self.plugins.python_progress(),
                    self.closed.clone(),
                ));
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
            if matches!(topic, sailry_protocol::Topic::CodeInstallations) {
                return Ok(self.code.subscribe(self.node, None, self.closed.clone()));
            }
            if let sailry_protocol::Topic::CodeServers(worktree) = topic {
                self.worktree_root(worktree).await?;
                return Ok(self
                    .code
                    .subscribe(self.node, Some(worktree), self.closed.clone()));
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
    pub(crate) fn profile(&self) -> Option<&std::path::Path> {
        self.profile.as_deref()
    }

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
