mod admission;
pub(crate) mod agent;
mod artifacts;
mod attachments;
mod background_settings;
mod backup;
mod captures;
mod code_index;
mod commands;
mod connections;
pub(crate) mod database;
mod databases;
pub(crate) mod dispatch;
mod external;
mod forwarding;
pub(crate) mod hooks;
mod installations;
pub(crate) mod knowledge;
mod local_models;
pub(crate) mod login;
mod mcp;
pub(crate) mod media;
mod models;
mod mutations;
mod network;
mod notifications;
mod operations;
mod outcomes;
mod plugins;
mod ports;
mod power;
mod processes;
mod projects;
mod providers;
mod publication;
mod python;
mod reads;
mod requests;
mod resources;
mod roles;
mod routing;
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
use sailry_protocol::{ErrorCode, Fault, NodeId, Request, TurnId, Update};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::Error;
use database::Database;

const EVENT_CAPACITY: usize = 128;
const CONTROL_CAPACITY: usize = 32;
// External execution workers retain their existing bounded resource queues.
const CAPACITY: usize = 128;

enum Job {
    Read {
        caller: NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<Result<sailry_protocol::Output, Fault>>,
    },
    CheckPlugin {
        caller: NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    Request {
        access: crate::files::Access,
        caller: NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<Result<Admission, Fault>>,
    },
    PowerFailed(i64),
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
        reply: oneshot::Sender<Result<backup::Source, Fault>>,
    },
    LocalModels {
        models: Vec<sailry_protocol::models::Model>,
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
    ConnectorTarget {
        id: sailry_protocol::plugin::connector::Id,
        revision: u64,
        checking: bool,
        reply: oneshot::Sender<Result<crate::plugins::connectors::Target, Fault>>,
    },
    ConnectorGrant {
        id: sailry_protocol::plugin::connector::Id,
        revision: u64,
        grant_revision: u64,
        grant: Option<crate::providers::login::plugin::Issued>,
        reply: oneshot::Sender<Result<u64, Fault>>,
    },
    Authorization {
        reference: sailry_protocol::CredentialRef,
        provider: sailry_protocol::ProviderId,
        authentication: sailry_protocol::Authentication,
        options: sailry_protocol::conversation::oauth::Options,
        reply: oneshot::Sender<Result<login::Authorization, Fault>>,
    },
    ChannelPresets {
        catalog: Option<sailry_protocol::conversation::presets::Catalog>,
        unavailable: bool,
        reply: oneshot::Sender<Result<sailry_protocol::conversation::presets::Listing, Fault>>,
    },
    ProviderQuota(providers::quota::Observation),
    Login(Box<login::Progress>),
    SubscribeLogin {
        caller: NodeId,
        id: sailry_protocol::RequestId,
        connector: bool,
        reply: oneshot::Sender<Result<Box<dyn Subscription>, Fault>>,
    },
    Agent(Box<agent::Operation>),
    TerminalInfo(sailry_protocol::terminal::Info),
    ProjectRemovalPlan {
        caller: NodeId,
        request: sailry_protocol::RequestId,
        expected: sailry_protocol::Project,
        reply: oneshot::Sender<Result<Vec<sailry_protocol::terminal::Info>, Fault>>,
    },
    ProjectTerminalClosed {
        worktree: sailry_protocol::WorktreeId,
        terminal: sailry_protocol::TerminalId,
        result: Result<sailry_protocol::terminal::Info, Fault>,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    Execute(Box<external::Work>),
    Shutdown,
    Completed(Box<external::Completed>),
    Installed {
        completed: Box<external::Completed>,
        operation: Option<TurnId>,
        release: oneshot::Sender<()>,
    },
    Transferred(Box<external::Completed>),
    Executed(Box<external::Completed>),
    Resources(Box<external::Completed>),
    PluginExecution(Box<external::Completed>),
    Ssh(Box<external::Completed>),
    Database(Box<external::Completed>),
    Network(Box<external::Completed>),
    NetworkScope {
        fallback: sailry_link::NetworkScope,
        reply: oneshot::Sender<Result<sailry_link::NetworkScope, Fault>>,
    },
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
        access: crate::files::Access,
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
    TerminalOpening {
        id: sailry_protocol::TerminalId,
        reply: oneshot::Sender<Result<terminals::Opening, Fault>>,
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
        reply:
            oneshot::Sender<Result<(sailry_protocol::Credential, sailry_protocol::Secret), Fault>>,
    },
    Provider {
        id: sailry_protocol::ProviderId,
        revision: u64,
        reply: oneshot::Sender<Result<sailry_protocol::conversation::Provider, Fault>>,
    },
    ProviderHttp {
        caller: NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<Result<Box<crate::plugins::http::Input>, Fault>>,
    },
    ModelAdapter {
        provider: Box<sailry_protocol::conversation::Provider>,
        messages: bool,
        reply: oneshot::Sender<Result<sailry_protocol::plugin::model_adapter::Declaration, Fault>>,
    },
    CaptureChannel {
        provider: Box<sailry_protocol::conversation::Provider>,
        reply: oneshot::Sender<Result<sailry_protocol::conversation::Provider, Fault>>,
    },
    Catalog {
        download: crate::providers::catalog::Download,
        reply: oneshot::Sender<Result<sailry_protocol::conversation::catalog::Status, Fault>>,
    },
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
    pub(crate) power: Arc<crate::power::Service>,
    profile: Option<PathBuf>,
    backup_resources: Arc<tokio::sync::RwLock<()>>,
    host: Arc<crate::host::Host>,
    discovery: crate::providers::Discovery,
    pub(crate) presets: Arc<crate::providers::presets::Directory>,
    catalog: crate::providers::catalog::Catalog,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) authorization_endpoint: Arc<std::sync::Mutex<Option<String>>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) model_catalog_fixtures: Arc<crate::models::Fixtures>,
    #[cfg(any(test, feature = "test-support"))]
    forwarding_wait: Arc<std::sync::Mutex<Option<forwarding::Wait>>>,
    pub(crate) files: crate::files::Files,
    watches: Arc<crate::files::watch::Watches>,
    transfers: Arc<crate::files::transfers::Transfers>,
    ports: Arc<crate::ports::Ports>,
    pub(crate) browsers: Arc<crate::browser::Browsers>,
    pub(crate) external_browser: Arc<crate::external_browser::Browsers>,
    pub(crate) git: crate::git::Git,
    pub(crate) code_index: Arc<crate::code_index::Service>,
    pub(crate) ocr: Arc<crate::ocr::Service>,
    pub(crate) local_models: Arc<crate::models::registry::Registry>,
    pub(crate) knowledge: Arc<OnceLock<Arc<crate::knowledge::Service>>>,
    pub(crate) code_queries: Arc<crate::code_index::Queries>,
    pub(crate) web: Arc<crate::web::Fetcher>,
    pub(crate) search: Arc<crate::web::Searcher>,
    terminals: Arc<crate::terminal::Terminals>,
    commands: Arc<crate::process::background::Commands>,
    pub(crate) environment: Arc<crate::process::Environment>,
    sender: mpsc::Sender<Job>,
    events: broadcast::Sender<sailry_protocol::EventEnvelope>,
    storage: Arc<OnceLock<Arc<database::Storage>>>,
    operations: crate::tasks::Tasks,
    accepting: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    pub(crate) agents: Arc<crate::agent::Controls>,
    pub(crate) model_clients: Arc<crate::agent::model::registry::Registry>,
    feeds: agent::Feeds,
    pub(crate) dispatch: Arc<crate::dispatch::Controls>,
    pub(crate) plugins: crate::plugins::Host,
    installations: Arc<installations::Registry>,
    pub(crate) login_service: crate::providers::login::Service,
    closed: CancellationToken,
}

struct Services {
    catalog: crate::providers::catalog::Catalog,
    authorization: crate::providers::login::Service,
    skills: crate::plugins::github::Github,
    environment: Arc<crate::process::Environment>,
    bundled: bool,
}

pub(crate) struct Store {
    pub(crate) ingress: Arc<Ingress>,
    worker: Option<tokio::task::JoinHandle<Result<(), Error>>>,
}

impl Store {
    pub(crate) async fn open(
        profile: PathBuf,
        node: NodeId,
        catalog: crate::providers::catalog::Catalog,
        authorization: crate::providers::login::Service,
        skills: crate::plugins::github::Github,
        source: crate::process::environment::Source,
        bundled: bool,
    ) -> Result<Self, Error> {
        let environment = Arc::new(crate::process::Environment::capture(source).await);
        Self::start_ready(
            profile.join("storage/node.turso"),
            node,
            Some(profile),
            Services {
                catalog,
                authorization,
                skills,
                environment,
                bundled,
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
                bundled: true,
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
            bundled,
        } = services;
        let operations = crate::tasks::Supervisor::start(32);
        let (sender, mut receiver) = mpsc::channel(CONTROL_CAPACITY);
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let accepting = Arc::new(AtomicBool::new(true));
        let closing = Arc::new(AtomicBool::new(false));
        let agents = Arc::new(crate::agent::Controls::default());
        let closed = CancellationToken::new();
        let lifecycle = sender.clone();
        let terminals = crate::terminal::Terminals::new(node, move |info| {
            let _ = lifecycle.blocking_send(Job::TerminalInfo(info));
        });
        let local_models =
            crate::models::registry::Registry::new(profile.clone(), closed.child_token());
        let ocr = crate::ocr::Service::new(local_models.clone());
        let failures = sender.clone();
        let power_runtime = tokio::runtime::Handle::current();
        let power = crate::power::Service::new(move |code| {
            let failures = failures.clone();
            power_runtime.spawn(async move {
                let _ = failures.send(Job::PowerFailed(code)).await;
            });
        });
        let watches = Arc::new(crate::files::watch::Watches::default());
        let ingress = Arc::new(Ingress {
            storage: Arc::default(),
            operations: operations.tasks.clone(),
            power: power.clone(),
            node,
            profile: profile.clone(),
            backup_resources: Arc::default(),
            host: crate::host::Host::new(),
            discovery: crate::providers::Discovery::new(),
            presets: Arc::default(),
            catalog,
            files: crate::files::Files::new().with_ocr(ocr.clone()),
            #[cfg(any(test, feature = "test-support"))]
            authorization_endpoint: Arc::new(std::sync::Mutex::new(
                authorization.model_endpoint.clone(),
            )),
            #[cfg(any(test, feature = "test-support"))]
            model_catalog_fixtures: Arc::default(),
            #[cfg(any(test, feature = "test-support"))]
            forwarding_wait: Arc::default(),
            watches: watches.clone(),
            transfers: crate::files::transfers::Transfers::new(profile.clone()),
            ports: Arc::new(crate::ports::Ports::default()),
            browsers: Arc::default(),
            external_browser: Arc::new(crate::external_browser::Browsers::new(profile.clone())),
            git: crate::git::Git::new(),
            code_queries: Arc::new(crate::code_index::Queries::default()),
            web: Arc::default(),
            search: Arc::default(),
            code_index: crate::code_index::Service::new(
                profile.clone(),
                node,
                watches.clone(),
                closed.clone(),
            ),
            ocr,
            local_models,
            knowledge: Arc::new(OnceLock::new()),
            terminals: terminals.clone(),
            commands: Arc::default(),
            environment: environment.clone(),
            login_service: authorization.clone(),
            sender,
            events: events.clone(),
            accepting: accepting.clone(),
            closing: closing.clone(),
            agents: agents.clone(),
            model_clients: Arc::default(),
            feeds: Default::default(),
            plugins: crate::plugins::Host::new(profile.clone()).with_github(skills),
            installations: Arc::default(),
            closed,
            dispatch: Arc::new(crate::dispatch::Controls::default()),
        });
        let (finished, mut completions) = mpsc::unbounded_channel();
        ingress.commands.set_sink(finished);
        let completion_sender = ingress.sender.clone();
        let completion_closed = ingress.closed.clone();
        let completion_agents = ingress.agents.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = completion_closed.cancelled() => break,
                    completed = completions.recv() => {
                        let Some(completed) = completed else { break };
                        if completion_sender.send(Job::Agent(Box::new(agent::Operation::CommandFinished(completed)))).await.is_err() { break; }
                        completion_agents.wake.notify_one();
                    }
                }
            }
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
        let attachment_profile = profile.clone();
        let terminal_profile = profile.clone();
        let mut network_work = network::Worker::new(
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            ingress.closed.clone(),
        );
        let mut forwarder = forwarding::Forwarder::new(
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            ingress.closed.clone(),
            #[cfg(any(test, feature = "test-support"))]
            ingress.forwarding_wait.clone(),
        );
        let mut processes = processes::Worker::new(
            tokio::runtime::Handle::current(),
            ingress.sender.clone(),
            &ingress,
        );
        let mut plugin_work = plugins::execution::Worker::new(ingress.clone());
        let mut installations = installations::Worker::new(ingress.clone());
        let installing = ingress.clone();
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
        let code_index = ingress.code_index.clone();
        let searcher = ingress.search.clone();
        let feeds = ingress.feeds.clone();
        let model_clients = ingress.model_clients.clone();
        let dispatch_controls = ingress.dispatch.clone();
        let knowledge_databases = ingress.knowledge.clone();
        let storage_ready = ingress.storage.clone();
        let database_ingress = (*ingress).clone();
        let worker = tokio::spawn(async move {
            let mut database = match Database::open_with_plugins(
                &path,
                node,
                profile.clone(),
                (bundled && profile.is_some()).then_some(&collection),
            )
            .await
            {
                Ok(database) => database,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return Ok(());
                }
            };
            match power::read(&database.connection).await {
                Ok(settings) => power.apply_settings(settings),
                Err(error) => {
                    let _ = ready.send(Err(crate::Error::from(error)));
                    return Ok(());
                }
            }
            if let Err(error) = database.configure(feeds, model_clients) {
                let _ = ready.send(Err(crate::Error::from(error)));
                return database.close().await;
            }
            if let Err(error) = database.models(installing.local_models.clone()).await {
                let _ = ready.send(Err(crate::Error::from(error)));
                return Ok(());
            }
            match search::read(&database.connection).await {
                Ok(settings) => searcher.configure(settings),
                Err(error) => {
                    let _ = ready.send(Err(crate::Error::from(error)));
                    return Ok(());
                }
            }
            plugins::collection::run(&mut database.connection, &collection).await;
            if let Err(error) = plugins::retain_python(&database.connection, &collection).await {
                let _ = ready.send(Err(crate::Error::from(error)));
                return Ok(());
            }
            match code_index::initialize(&mut database.connection).await {
                Ok(settings) => {
                    code_index.configure(settings.enabled);
                }
                Err(error) => {
                    let _ = ready.send(Err(crate::Error::from(error)));
                    return Ok(());
                }
            }
            attachments::collect(&database.connection, attachment_profile.as_deref()).await;
            let mut mutations = match mutations::Worker::start(
                completed,
                watches.clone(),
                code_index.clone(),
                move |caller, roots, request| {
                    let root = &roots.target;
                    match &request.command {
                        sailry_protocol::Command::RemoveProject { expected } => {
                            return projects::removal::execute(
                                &validation,
                                &terminals,
                                caller,
                                request,
                                expected,
                            );
                        }
                        sailry_protocol::Command::SavePluginMcp {
                            package,
                            configuration,
                        } => {
                            return plugins.validate_mcp(package, configuration).map(|()| {
                                sailry_protocol::Output::PluginMcp(
                                    sailry_protocol::plugin::mcp::State {
                                        package: package.clone(),
                                        configuration: configuration.clone(),
                                    },
                                )
                            });
                        }
                        sailry_protocol::Command::FinishAttachmentUpload { worktree, stream } => {
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
                                &validation,
                                root,
                                selection,
                                request.id,
                            )
                            .map(sailry_protocol::Output::HunkApplied);
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
                            let opening = response.blocking_recv().map_err(|_| unavailable())??;
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
                                        access: opening.info.access.ok_or_else(|| {
                                            sailry_protocol::Fault::invalid(
                                                "local terminal has no access policy",
                                            )
                                        })?,
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
                            return commands.write(*session, *id, input).map(|bytes| {
                                sailry_protocol::Output::CommandInputAccepted { id: *id, bytes }
                            });
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
                            .blocking_send(Job::ValidateRelocation {
                                paths,
                                reply,
                                access: roots.access,
                            })
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
                },
            ) {
                Ok(worker) => worker,
                Err(error) => {
                    let _ = ready.send(Err(error));
                    return database.close().await;
                }
            };
            let storage = Arc::new(database.into_storage());
            assert!(
                storage_ready.set(storage.clone()).is_ok(),
                "storage initializes once"
            );
            if ready.send(Ok(())).is_err() {
                mutations.join()?;
                return storage.close().await;
            }
            while let Some(job) = receiver.recv().await {
                if operations::accepts(&job) {
                    let ingress = database_ingress.clone();
                    let _ = operations
                        .tasks
                        .spawn_child(&CancellationToken::new(), async move {
                            operations::execute(ingress, job).await
                        })
                        .await;
                    continue;
                }
                let package_request = matches!(
                    &job,
                    Job::Request { request, .. }
                        if matches!(
                            &request.command,
                            sailry_protocol::Command::SetPluginEnabled { enabled: false, .. }
                                | sailry_protocol::Command::RemovePlugin { .. }
                        )
                );
                let job = match job {
                    Job::Request {
                        access,
                        caller,
                        request,
                        reply,
                    } => database_ingress
                        .persist_request(access, caller, request, reply, None)
                        .await
                        .map(|work| Job::Execute(Box::new(work))),
                    Job::BackupSnapshot {
                        revision,
                        cancel,
                        reply,
                    } => {
                        let knowledge = knowledge_databases.get().map(|service| service.database());
                        let _ = reply.send(
                            backup::source(&storage, knowledge.as_ref(), revision, &cancel).await,
                        );
                        continue;
                    }
                    #[cfg(test)]
                    Job::Barrier { entered, release } => {
                        entered.send(()).unwrap();
                        tokio::task::spawn_blocking(move || {
                            release.recv_timeout(std::time::Duration::from_secs(10))
                        })
                        .await
                        .expect("storage barrier worker failed")
                        .expect("storage barrier was not released");
                        continue;
                    }
                    other => Some(other),
                };
                let mut job = job;
                let completed = match &mut job {
                    Some(
                        Job::Completed(completed)
                        | Job::Transferred(completed)
                        | Job::Executed(completed)
                        | Job::Resources(completed)
                        | Job::PluginExecution(completed)
                        | Job::Ssh(completed)
                        | Job::Database(completed)
                        | Job::Network(completed)
                        | Job::Installed { completed, .. },
                    ) => Some(completed),
                    _ => None,
                };
                if let Some(completed) = completed
                    && let Ok(output) = &completed.result
                    && let Err(error) = storage
                        .prepare_plugin(&completed.request.command, output)
                        .await
                {
                    completed.result = Err(database::storage_error(error));
                }
                let mut database = storage.context(false).await?;
                let package_change = package_request
                    || matches!(&job, Some(Job::Completed(_) | Job::Installed { .. }));
                // Poll one operation at a time without accumulating every handler's
                // async temporaries on the storage task's stack.
                let handling: Pending<'_, ()> = if let Some(job) = job {
                    let database = &mut database;
                    let network_work = &mut network_work;
                    let forwarder = &mut forwarder;
                    let processes = &mut processes;
                    let plugin_work = &mut plugin_work;
                    let installations = &mut installations;
                    let resource_work = &mut resource_work;
                    let logins = &mut logins;
                    let ssh = &mut ssh;
                    let databases = &mut databases;
                    let mutations = &mut mutations;
                    let events = &events;
                    let agents = &agents;
                    let code_index = &code_index;
                    let accepting = &accepting;
                    let closing = &closing;
                    let dispatch_controls = &dispatch_controls;
                    let installing = &installing;
                    match job {
                        Job::Agent(operation) => Box::pin(async move {
                            agent::execute(database, *operation, events, agents).await;
                        }),
                        #[cfg(test)]
                        Job::Barrier { .. } => Box::pin(async move {
                            unreachable!("barriers run before connection acquisition")
                        }),
                        Job::BackupSnapshot { .. } => Box::pin(async move {
                            unreachable!("backup runs before connection acquisition")
                        }),

                        Job::Authorization {
                            reference,
                            provider,
                            authentication,
                            options,
                            reply,
                        } => Box::pin(async move {
                            if accepting.load(Ordering::Acquire) {
                                logins
                                    .authorize(
                                        database,
                                        reference,
                                        provider,
                                        authentication,
                                        options,
                                        reply,
                                    )
                                    .await;
                            } else {
                                let _ = reply.send(Err(unavailable()));
                            }
                        }),
                        Job::Login(progress) => Box::pin(async move {
                            logins.progress(database, *progress, events).await;
                        }),
                        Job::ProviderQuota(observation) => Box::pin(async move {
                            if let Err(error) =
                                providers::quota::observe(database, observation, events).await
                            {
                                eprintln!("provider quota persistence failed: {error}");
                            }
                        }),
                        Job::TerminalInfo(info) => Box::pin(async move {
                            if let Err(error) = database.terminal_changed(info, events).await {
                                eprintln!("terminal lifecycle persistence failed: {error}");
                            }
                        }),

                        Job::SubscribeLogin {
                            caller,
                            id,
                            connector,
                            reply,
                        } => Box::pin(async move {
                            let _ = reply.send(if connector {
                                logins.subscribe_connector(caller, id)
                            } else {
                                logins.subscribe(caller, id)
                            });
                        }),

                        Job::BindLink { link, reply } => Box::pin(async move {
                            *database.network_link.lock().unwrap() = Some(link.clone());
                            // Shutdown releases this shared back-reference after the worker drains.
                            ssh.link = Some(link.clone());
                            forwarder.link = Some(link);
                            let _ = reply.send(());
                        }),

                        Job::Shutdown => Box::pin(async move {
                            accepting.store(false, Ordering::Release);
                            closing.store(true, Ordering::Release);
                        }),
                        Job::Completed(completed) => Box::pin(async move {
                            if completed.result.is_ok()
                                && let sailry_protocol::Command::RemoveWorktree { worktree, .. } =
                                    completed.request.command
                            {
                                code_index.remove(worktree);
                            }
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
                            if matches!(&result, Ok(sailry_protocol::Output::Plugin(_))) {
                                dispatch_controls.wake.notify_one();
                            }
                            if result.is_ok() && code_index::configures(&completed.request.command)
                            {
                                code_index::refresh(&database.connection, code_index).await;
                            }
                            let _ = completed.reply.send(result);
                            mutations.pending -= 1;
                        }),
                        Job::Installed {
                            completed,
                            operation,
                            release,
                        } => Box::pin(async move {
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
                            if matches!(&result, Ok(sailry_protocol::Output::Plugin(_))) {
                                dispatch_controls.wake.notify_one();
                            }
                            if result.is_ok() && code_index::configures(&completed.request.command)
                            {
                                code_index::refresh(&database.connection, code_index).await;
                            }
                            installing.installations.finish(
                                completed.caller,
                                completed.request.id,
                                &result,
                            );
                            let _ = completed.reply.send(result);
                            installations.pending -= 1;
                            if let Some(turn) = operation {
                                agents.finish_operation(turn);
                            }
                            let _ = release.send(());
                        }),
                        Job::Network(completed) => Box::pin(async move {
                            network::release(database, &completed.request).await;
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
                            let _ = completed.reply.send(result);
                            network_work.pending -= 1;
                        }),
                        Job::Transferred(completed) => Box::pin(async move {
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
                            let _ = completed.reply.send(result);
                            forwarder.pending -= 1;
                        }),
                        Job::PluginExecution(completed) => Box::pin(async move {
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
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
                        }),
                        Job::Resources(completed) => Box::pin(async move {
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
                            let _ = completed.reply.send(result);
                            if let Some(turn) = resources::turn(&completed.request) {
                                agents.finish_operation(turn);
                            }
                            resource_work.pending -= 1;
                        }),
                        Job::Executed(completed) => Box::pin(async move {
                            if let sailry_protocol::Command::RunCommand { turn, .. } = &completed.request.command
                                && let Ok(worktree) = (async { let mut statement = database.connection.prepare("SELECT r.worktree FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1").await?;
let row = statement.query_row([turn.to_string()]).await?;
let mapped: turso::Result<_> = row.get::<String>(0);
mapped }).await
                                && let Ok(worktree) = worktree.parse()
                            {
                                code_index.changed(worktree,None);
                            }
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
                            let _ = completed.reply.send(result);
                            if let sailry_protocol::Command::RunCommand { turn, .. } =
                                completed.request.command
                            {
                                agents.finish_operation(turn);
                            }
                            processes.pending -= 1;
                        }),
                        Job::Ssh(completed) => Box::pin(async move {
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
                            let _ = completed.reply.send(result);
                            ssh.finished(completed.caller, completed.request.id);
                        }),
                        Job::Database(completed) => Box::pin(async move {
                            let result = database
                                .finish_external(
                                    completed.caller,
                                    &completed.request,
                                    completed.result,
                                    events,
                                )
                                .await;
                            let _ = completed.reply.send(result);
                            databases.finished(completed.caller, completed.request.id);
                        }),

                        Job::Execute(work) => Box::pin(async move {
                            let rejected = match *work {
                                external::Work::Network(execution) => {
                                    network_work.submit(execution).err().map(|execution| {
                                        (execution.caller, execution.request, execution.reply)
                                    })
                                }
                                external::Work::PluginExecution(execution) => {
                                    plugin_work.submit(*execution).err().map(|execution| {
                                        (execution.caller, execution.request, execution.reply)
                                    })
                                }
                                external::Work::Login(operation) => {
                                    logins.execute(database, operation, events).await;
                                    None
                                }
                                external::Work::Resources(execution) => {
                                    if installations::models::handles(&execution.request.command) {
                                        installations::models::submit(
                                            installations,
                                            database,
                                            events,
                                            installing.clone(),
                                            execution,
                                        )
                                        .await
                                        .err()
                                        .map(|execution| {
                                            (execution.caller, execution.request, execution.reply)
                                        })
                                    } else {
                                        resource_work.submit(execution).err().map(|execution| {
                                            (execution.caller, execution.request, execution.reply)
                                        })
                                    }
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
                                    if plugins::installing::handles(&mutation.request.command) {
                                        plugins::installing::submit(
                                            installations,
                                            installing.clone(),
                                            mutation,
                                        )
                                        .err()
                                        .map(|execution| {
                                            (execution.caller, execution.request, execution.reply)
                                        })
                                    } else {
                                        mutations.submit(mutation).err().map(|mutation| {
                                            (mutation.caller, mutation.request, mutation.reply)
                                        })
                                    }
                                }
                                external::Work::Session(transfer) => {
                                    forwarder.submit(transfer).err().map(|transfer| {
                                        (transfer.caller, transfer.request, transfer.reply)
                                    })
                                }
                            };
                            if let Some((caller, request, reply)) = rejected {
                                network::release(database, &request).await;
                                let result = database
                                    .finish_external(
                                        caller,
                                        &request,
                                        Err(Fault::new(
                                            ErrorCode::Busy,
                                            "Node operation queue is unavailable",
                                        )),
                                        events,
                                    )
                                    .await;
                                installing.installations.finish(caller, request.id, &result);
                                let _ = reply.send(result);
                            }
                        }),
                        _ => {
                            Box::pin(async move { unreachable!("SQL jobs execute independently") })
                        }
                    }
                } else {
                    Box::pin(std::future::ready(()))
                };
                handling.await;
                if let Err(error) = database::conflict::rollback(&database.connection).await {
                    eprintln!("storage lifecycle cleanup failed: {error}");
                }
                if package_change
                    && let Err(error) =
                        plugins::retain_python(&database.connection, &collection).await
                {
                    eprintln!("Python package references could not be refreshed: {error}");
                }
                if package_change {
                    match agent::continuation::stopping(&database.connection).await {
                        Ok(turns) => {
                            for turn in turns {
                                agents.cancel(turn);
                            }
                            agents.wake.notify_waiters();
                        }
                        Err(error) => {
                            eprintln!(
                                "automatic input cancellation could not be reconciled: {error}"
                            );
                        }
                    }
                }
                if closing.load(Ordering::Acquire)
                    && mutations.pending
                        + network_work.pending
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
            operations
                .shutdown(std::time::Duration::from_secs(30))
                .await?;
            tokio::task::spawn_blocking(move || mutations.join())
                .await
                .map_err(|error| Error::Worker(error.to_string()))??;
            {
                let mut database = storage.context(false).await?;
                plugins::collection::run(&mut database.connection, &collection).await;
                attachments::collect(&database.connection, attachment_profile.as_deref()).await;
            }
            storage.close().await
        });
        let store = Self {
            ingress,
            worker: Some(worker),
        };
        initialized
            .await
            .map_err(|error| Error::Worker(error.to_string()))??;
        if let Err(error) = local_models::start(&store.ingress).await {
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

    pub(crate) async fn network_scope(
        &self,
        fallback: sailry_link::NetworkScope,
    ) -> Result<sailry_link::NetworkScope, Fault> {
        let (reply, result) = oneshot::channel();
        self.ingress
            .submit(Job::NetworkScope { fallback, reply })
            .await
            .map_err(|_| unavailable())?;
        result.await.map_err(|_| unavailable())?
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
        self.ingress.model_clients.clear();
        self.ingress.external_browser.stop.cancel();
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
        self.ingress.local_models.shutdown().await;
        let browsers = self.ingress.external_browser.shutdown().await;
        self.ingress.ports.join().await;
        self.ingress.commands.close().await;
        self.ingress.code_index.shutdown();
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
        let result = worker
            .await
            .map_err(|error| Error::Worker(error.to_string()));
        let link = self.ingress.storage.get().and_then(|storage| {
            let mut slot = storage
                .network_link
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            slot.take()
        });
        drop(link);
        result??;
        if let Some(knowledge) = self.ingress.knowledge.get() {
            drop(knowledge.database().pause().await?);
        }
        self.ingress.power.close();
        browsers.map_err(|error| Error::Worker(error.to_string()))
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.ingress.power.close();
        self.stop_admission();
        self.ingress.closing.store(true, Ordering::Release);
        // A full queue already wakes the worker, which observes the admission flag.
        let _ = self.ingress.sender.try_send(Job::Shutdown);
    }
}

impl Handler for Ingress {
    fn product_version(&self) -> Option<&str> {
        Some(env!("SAILRY_HOST_VERSION"))
    }

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
            if matches!(topic, sailry_protocol::Topic::Connections) {
                return plugins::connectors::watch::subscribe(self.clone(), caller).await;
            }
            if let sailry_protocol::Topic::ProviderLogin(id)
            | sailry_protocol::Topic::ConnectorLogin(id) = topic
            {
                let (reply, response) = oneshot::channel();
                self.sender
                    .try_send(Job::SubscribeLogin {
                        caller,
                        id,
                        connector: matches!(topic, sailry_protocol::Topic::ConnectorLogin(_)),
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
            if matches!(topic, sailry_protocol::Topic::Installations) {
                return Ok(self
                    .installations
                    .subscribe(self.node, caller, self.closed.clone()));
            }
            if let sailry_protocol::Topic::ScopedInstallations(request) = &topic {
                if request.target != self.node || request.version != sailry_protocol::VERSION {
                    return Err(Fault::new(
                        ErrorCode::WrongTarget,
                        "Node or protocol version mismatch",
                    ));
                }
                let sailry_protocol::Command::ReadInstallations { requests } = &request.command
                else {
                    return Err(Fault::invalid("installation observation expected"));
                };
                let context = request
                    .plugin
                    .clone()
                    .ok_or_else(|| Fault::invalid("plugin scope expected"))?;
                self.check_plugin(caller, request.as_ref().clone()).await?;
                return self.installations.subscribe_for(
                    self.node,
                    caller,
                    requests.clone(),
                    context,
                    self.closed.clone(),
                );
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
            let cursor = snapshot.cursor;
            Ok(Box::new(Events {
                initial: Some(Update::Snapshot(snapshot)),
                receiver,
                closed: self.closed.clone(),
                storage: self.storage.get().cloned().ok_or_else(unavailable)?,
                node: self.node,
                cursor,
                through: cursor,
                pending: Default::default(),
            }) as Box<dyn Subscription>)
        })
    }
}

impl Ingress {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn with_knowledge<T: Send + 'static>(
        &self,
        work: impl for<'a> FnOnce(&'a mut turso::Connection) -> Pending<'a, Result<T, Fault>>
        + Send
        + 'static,
    ) -> Result<T, Fault> {
        let service = self.knowledge.get().ok_or_else(unavailable)?;
        let handle = service.database();
        let mut connection = handle.lease(true).await.map_err(database::storage_error)?;
        work(connection.connection_mut()).await
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn with_storage<T: Send + 'static>(
        &self,
        work: impl for<'a> FnOnce(&'a mut turso::Connection) -> Pending<'a, Result<T, Fault>>
        + Send
        + 'static,
    ) -> Result<T, Fault> {
        let storage = self.storage.get().ok_or_else(unavailable)?;
        let mut database = storage
            .context(false)
            .await
            .map_err(database::storage_error)?;
        work(database.connection.connection_mut()).await
    }
}

impl Ingress {
    pub(crate) async fn connector_target(
        &self,
        id: sailry_protocol::plugin::connector::Id,
        revision: u64,
        checking: bool,
    ) -> Result<crate::plugins::connectors::Target, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::ConnectorTarget {
            id,
            revision,
            checking,
            reply,
        })
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn save_connector_grant(
        &self,
        id: sailry_protocol::plugin::connector::Id,
        revision: u64,
        grant_revision: u64,
        grant: Option<crate::providers::login::plugin::Issued>,
    ) -> Result<u64, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::ConnectorGrant {
            id,
            revision,
            grant_revision,
            grant,
            reply,
        })
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) fn profile(&self) -> Option<&std::path::Path> {
        self.profile.as_deref()
    }

    pub(crate) async fn resolve_credential(
        &self,
        reference: sailry_protocol::CredentialRef,
        provider: sailry_protocol::ProviderId,
    ) -> Result<sailry_protocol::Secret, Fault> {
        self.model_credential(reference, provider)
            .await
            .map(|(_, secret)| secret)
    }

    pub(crate) async fn model_credential(
        &self,
        reference: sailry_protocol::CredentialRef,
        provider: sailry_protocol::ProviderId,
    ) -> Result<(sailry_protocol::Credential, sailry_protocol::Secret), Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::ResolveCredential {
            reference,
            provider,
            reply,
        })
        .await
        .map_err(|_| unavailable())?;
        response.await.map_err(|_| unavailable())?
    }
}

impl PeerStore for Ingress {
    fn addresses(&self) -> Pending<'_, Result<Vec<sailry_link::EndpointAddr>, Fault>> {
        Box::pin(async move {
            let storage = self.storage.get().ok_or_else(unavailable)?;
            let database = storage
                .context(true)
                .await
                .map_err(database::storage_error)?;
            database.addresses().await
        })
    }

    fn remember(&self, address: sailry_link::EndpointAddr) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async move {
            let (reply, response) = oneshot::channel();
            self.submit(Job::Remember { address, reply })
                .await
                .map_err(|_| unavailable())?;
            response.await.map_err(|_| unavailable())?
        })
    }
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>> {
        Box::pin(async move {
            let storage = self.storage.get().ok_or_else(unavailable)?;
            let database = storage
                .context(true)
                .await
                .map_err(database::storage_error)?;
            database.peers().await
        })
    }

    fn set_trust(&self, peer: NodeId, trusted: bool) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async move {
            if !self.accepting.load(Ordering::Acquire) {
                return Err(unavailable());
            }
            let (reply, response) = oneshot::channel();
            self.submit(Job::SetTrust {
                peer,
                trusted,
                reply,
            })
            .await
            .map_err(|_| unavailable())?;
            response.await.map_err(|_| unavailable())?
        })
    }
}

struct Events {
    initial: Option<Update>,
    receiver: broadcast::Receiver<sailry_protocol::EventEnvelope>,
    closed: CancellationToken,
    storage: Arc<database::Storage>,
    node: NodeId,
    cursor: u64,
    through: u64,
    pending: std::collections::VecDeque<sailry_protocol::EventEnvelope>,
}

impl Events {
    async fn catch_up(&mut self, through: Option<u64>) -> Result<(), Fault> {
        let node = self.node;
        let after = self.cursor;
        let (through, events) = self
            .storage
            .read(move |connection| Box::pin(publication::read(connection, node, after, through)))
            .await?;
        self.through = through;
        self.pending.extend(events);
        Ok(())
    }
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
            loop {
                if let Some(event) = self.pending.pop_front() {
                    if event.cursor != self.cursor.saturating_add(1) {
                        self.pending.clear();
                        self.cursor = self.through;
                        return Ok(Update::ResetRequired);
                    }
                    self.cursor = event.cursor;
                    return Ok(Update::Event(event));
                }
                if self.cursor < self.through {
                    if self.catch_up(Some(self.through)).await.is_err() || self.pending.is_empty() {
                        self.cursor = self.through;
                        return Ok(Update::ResetRequired);
                    }
                    continue;
                }
                let result = tokio::select! {
                    biased;
                    _ = self.closed.cancelled() => return Err(unavailable()),
                    result = self.receiver.recv() => result,
                };
                match result {
                    Ok(event) if event.cursor <= self.cursor => continue,
                    Ok(event) if event.cursor == self.cursor.saturating_add(1) => {
                        self.cursor = event.cursor;
                        return Ok(Update::Event(event));
                    }
                    Ok(event) => {
                        self.through = event.cursor;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if self.catch_up(None).await.is_err() {
                            return Ok(Update::ResetRequired);
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return Err(unavailable()),
                }
            }
        })
    }
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "Node service is unavailable")
}

#[cfg(test)]
mod tests;
