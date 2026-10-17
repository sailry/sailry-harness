//! Execution Node ownership, durable admission and shared resource services.
//! Desktop and Host use the same runtime, session history and shutdown lifecycle.

mod agent;
mod artifacts;
mod backup;
mod browser;
mod code;
mod computer;
mod hooks;
#[cfg(all(target_os = "macos", feature = "computer-worker"))]
pub use computer::worker_host as computer_worker;
mod databases;
mod dispatch;
mod error;
mod external_browser;
mod files;
mod git;
mod host;
mod installation;
mod knowledge;
pub mod logging;
mod mcp_transport;
mod models;
mod ocr;
mod office;
mod plugins;
mod ports;
mod process;
mod profile;
mod providers;
mod python;
mod ssh;
mod store;
mod tasks;
mod terminal;
mod web;
mod workspaces;

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

#[cfg(any(test, feature = "test-support"))]
pub use code::fixtures::CodeServerFixture;
pub use computer::Worker as ComputerWorker;
#[cfg(any(test, feature = "test-support"))]
pub use computer::{HIDDEN_PARAMETERS as COMPUTER_HIDDEN, TOOLS as COMPUTER_TOOLS};
pub use error::Error;
#[cfg(any(test, feature = "test-support"))]
pub use process::environment::EnvironmentFixture;
use profile::Profile;
pub use profile::default_data_dir;
pub use sailry_link::{EndpointAddr, NetworkScope};
use store::Store;
use tasks::{Supervisor, Tasks};

const TASK_CAPACITY: usize = 64;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Stopping,
    Stopped,
}

/// A local observer cannot stop the Node or acquire another profile/endpoint.
#[derive(Clone)]
pub struct Observer {
    state: watch::Receiver<State>,
    tasks: Tasks,
}

impl Observer {
    pub fn state(&self) -> State {
        *self.state.borrow()
    }

    pub async fn changed(&mut self) -> Result<State, Error> {
        self.state.changed().await.map_err(|_| Error::Stopped)?;
        Ok(self.state())
    }

    /// Checks the actual service executor, rather than only reading cached state.
    pub async fn probe(&self) -> Result<(), Error> {
        self.tasks
            .spawn(async {})?
            .await
            .map_err(|_| Error::Stopped)
    }
}

/// Shared process-independent bootstrap for Desktop and the headless Host.
/// Keep the calling Tokio runtime alive until shutdown completes.
pub struct Node {
    path: PathBuf,
    observer: Observer,
    stop: Option<oneshot::Sender<()>>,
    worker: JoinHandle<Result<(), Error>>,
    identity: std::sync::Arc<sailry_link::Identity>,
    ingress: std::sync::Arc<store::Ingress>,
    link: sailry_link::LinkHandle,
}

#[derive(Default)]
struct Bootstrap {
    network: NetworkScope,
    catalog: providers::catalog::Catalog,
    authorization: providers::login::Service,
    skills: crate::plugins::github::Github,
    computer: Option<ComputerWorker>,
    environment: process::environment::Source,
    skip_bundled: bool,
}

impl Node {
    #[cfg(any(test, feature = "test-support"))]
    pub fn indexed_knowledge_inputs(&self) -> u64 {
        self.ingress
            .knowledge
            .get()
            .map_or(0, |service| service.models.indexed_inputs())
    }

    pub async fn start(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::start_with_network(path, NetworkScope::default()).await
    }

    pub async fn start_with_network(
        path: impl AsRef<Path>,
        network: NetworkScope,
    ) -> Result<Self, Error> {
        Self::start_with_computer(path, network, None).await
    }

    /// Installs the execution host's optional graphical computer worker before
    /// exposing the Node. Headless library consumers keep the direct backend.
    pub async fn start_with_computer(
        path: impl AsRef<Path>,
        network: NetworkScope,
        computer: Option<ComputerWorker>,
    ) -> Result<Self, Error> {
        let path = path.as_ref().to_owned();
        // Startup owns cleanup even if its caller stops waiting midway through storage I/O.
        tokio::spawn(Self::start_owned(
            path,
            Bootstrap {
                network,
                computer,
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Starts an isolated service fixture without external reference-catalog reads.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_isolated(path: impl AsRef<Path>) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Starts an isolated service fixture without automatically installing bundled packages.
    /// Existing data and explicitly installed packages retain their ordinary owners.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty(path: impl AsRef<Path>) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                skip_bundled: true,
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Keeps an isolated plugin catalog while fixtures install only their required packages.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty_with_plugin_catalog(
        path: impl AsRef<Path>,
        endpoint: &str,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                skills: crate::plugins::github::Github::catalog_fixture(endpoint),
                skip_bundled: true,
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Uses an isolated HTTP reference catalog for service and controller acceptance.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_with_catalog(path: impl AsRef<Path>, endpoint: &str) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::new(endpoint.to_owned()),
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Routes authorization HTTP to an isolated fixture without changing protocol endpoints.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_with_authorization(
        path: impl AsRef<Path>,
        endpoint: &str,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                authorization: providers::login::Service::fixture(endpoint),
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Routes isolated authorization fixtures without installing unrelated bundled packages.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty_with_authorization(
        path: impl AsRef<Path>,
        endpoint: &str,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                authorization: providers::login::Service::fixture(endpoint),
                skip_bundled: true,
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Routes skill repository reads to an isolated HTTP fixture.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_with_skill_source(
        path: impl AsRef<Path>,
        endpoint: &str,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                skills: crate::plugins::github::Github::fixture(endpoint),
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Routes only the official plugin index to an isolated HTTP fixture.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_with_plugin_catalog(
        path: impl AsRef<Path>,
        endpoint: &str,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                skills: crate::plugins::github::Github::catalog_fixture(endpoint),
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Captures an explicit temporary login environment through the real Node bootstrap.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_with_environment(
        path: impl AsRef<Path>,
        environment: EnvironmentFixture,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                environment: process::environment::Source::Fixture(environment),
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Rewrites official code downloads to a declared loopback test fixture.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_with_code(
        path: impl AsRef<Path>,
        environment: EnvironmentFixture,
        endpoint: &str,
    ) -> Result<Self, Error> {
        let node = Self::start_with_environment(path, environment).await?;
        node.ingress.code.fixture(endpoint);
        Ok(node)
    }

    /// Preserves an isolated login environment while fixtures install their required packages.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty_with_environment(
        path: impl AsRef<Path>,
        environment: EnvironmentFixture,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                environment: process::environment::Source::Fixture(environment),
                skip_bundled: true,
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Preserves the isolated model catalog without installing unrelated bundled packages.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty_with_catalog(
        path: impl AsRef<Path>,
        endpoint: &str,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::new(endpoint.to_owned()),
                skip_bundled: true,
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Preserves the isolated skill source while fixtures install their required packages.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty_with_skill_source(
        path: impl AsRef<Path>,
        endpoint: &str,
    ) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                skills: crate::plugins::github::Github::fixture(endpoint),
                skip_bundled: true,
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Preserves the injected computer worker without unrelated bundled packages.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty_with_computer(
        path: impl AsRef<Path>,
        network: NetworkScope,
        computer: Option<ComputerWorker>,
    ) -> Result<Self, Error> {
        let path = path.as_ref().to_owned();
        // Startup owns cleanup even if its caller stops waiting midway through storage I/O.
        tokio::spawn(Self::start_owned(
            path,
            Bootstrap {
                catalog: providers::catalog::Catalog::empty(),
                skip_bundled: true,
                network,
                computer,
                ..Default::default()
            },
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Keeps the isolated network scope while fixtures install their required packages.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty_with_network(
        path: impl AsRef<Path>,
        network: NetworkScope,
    ) -> Result<Self, Error> {
        Self::start_empty_with_computer(path, network, None).await
    }

    /// Preserves isolated environment and code downloads without unrelated bundled packages.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_empty_with_code(
        path: impl AsRef<Path>,
        environment: EnvironmentFixture,
        endpoint: &str,
    ) -> Result<Self, Error> {
        let node = Self::start_empty_with_environment(path, environment).await?;
        node.ingress.code.fixture(endpoint);
        Ok(node)
    }

    /// Pins a public test hostname to an isolated HTTP server after public-IP validation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_web_fixture(&self, host: &str, address: std::net::SocketAddr) {
        self.ingress.web.fixture(host, address);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_plugin_oauth_fixture(
        &self,
        provider: sailry_protocol::plugin::oauth::Provider,
        token_url: &str,
    ) {
        self.ingress.oauth.fixture(provider, token_url);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_plugin_registration_fixture(&self, endpoint: &str) {
        self.ingress.registration.fixture(endpoint);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_plugin_application_fixture(
        &self,
        provider: sailry_protocol::plugin::settings::Provider,
        token_url: &str,
        resource_origin: &str,
    ) {
        self.ingress
            .plugins
            .applications
            .fixture(provider, token_url, resource_origin);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_python_runtime_fixture(&self, path: &Path) -> Result<(), sailry_protocol::Fault> {
        self.ingress.plugins.set_python_runtime_fixture(path)
    }

    /// Registers sealed ready assets for UI fixtures; the interpreter cannot run.
    /// Available only to tests and the explicit `test-support` feature.
    #[cfg(any(test, feature = "test-support"))]
    pub fn register_python_environment_fixture(
        &self,
        name: &str,
    ) -> Result<std::sync::Arc<std::sync::atomic::AtomicUsize>, sailry_protocol::Fault> {
        self.ingress
            .plugins
            .register_python_environment_fixture(name, &self.ingress.environment)
    }

    /// Routes registry fixtures through the same Node-owned acquisition boundary.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_skill_registry_fixture(&self, endpoint: &str) -> Result<(), sailry_protocol::Fault> {
        self.ingress.plugins.registry.set_fixture(endpoint)
    }

    /// Confirms and claims an exact captured tool call through the real approval owner.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn authorize_tool_fixture(
        &self,
        confirmation: adk_core::ToolConfirmationRequest,
        operation: sailry_protocol::tool::Operation,
        mut request: sailry_protocol::Request,
    ) -> Result<sailry_protocol::Request, sailry_protocol::Fault> {
        let context = request
            .plugin
            .as_ref()
            .ok_or_else(|| sailry_protocol::Fault::invalid("tool fixture requires a plugin"))?;
        let turn = context
            .turn
            .ok_or_else(|| sailry_protocol::Fault::invalid("tool fixture requires a turn"))?;
        let binding = store::agent::ApprovalBinding {
            owner: context.package.clone(),
            tool: confirmation.tool_name.clone(),
            operation: Some(operation),
        };
        let decision = self
            .ingress
            .confirm(turn, confirmation.clone(), Some(binding))
            .await?;
        if !matches!(
            decision,
            sailry_protocol::conversation::Decision::Approve
                | sailry_protocol::conversation::Decision::ApproveSession
                | sailry_protocol::conversation::Decision::ApprovePrefix(_)
        ) {
            return Err(sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::PermissionDenied,
                "tool fixture was denied",
            ));
        }
        request.id = self
            .ingress
            .authorize(turn, confirmation, Some(request.command.clone()))
            .await?;
        Ok(request)
    }

    #[cfg(feature = "test-support")]
    pub fn set_web_proxy_fixture(
        &self,
        host: &str,
        proxy: hyper_util::client::proxy::matcher::Matcher,
    ) {
        self.ingress.web.fixture_proxy(host, proxy);
    }

    #[cfg(feature = "test-support")]
    pub fn set_web_dns_fixture(&self, host: &str, addresses: Vec<std::net::IpAddr>) {
        self.ingress.web.fixture_dns(host, addresses);
    }

    /// Replaces a search driver's endpoint with an isolated loopback service.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_search_fixture(&self, driver: sailry_protocol::search::Driver, endpoint: &str) {
        self.ingress.search.fixture(driver, endpoint);
    }

    /// Routes model metadata requests to an explicit loopback test catalog.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_model_catalog_fixture(
        &self,
        hub: sailry_protocol::models::Hub,
        endpoint: &str,
    ) -> Result<(), sailry_protocol::Fault> {
        self.ingress.model_catalog_fixtures.set(hub, endpoint)
    }

    async fn start_owned(path: PathBuf, bootstrap: Bootstrap) -> Result<Self, Error> {
        let Bootstrap {
            network,
            catalog,
            authorization,
            skills,
            computer,
            environment,
            skip_bundled,
        } = bootstrap;
        let mut entry = logging::Entry::new(logging::Area::Node, logging::Phase::Begin);
        entry.reason = Some("startup");
        let mut diagnostic = logging::Span::new(entry);
        let result = async {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let (profile, identity) = tokio::task::spawn_blocking(move || {
                let profile = Profile::acquire(&path)?;
                let identity = sailry_link::Identity::open(&profile.path)?;
                Ok::<_, Error>((profile, identity))
            })
            .await
            .map_err(|error| Error::Worker(error.to_string()))??;
            let path = profile.path.clone();
            let identity = std::sync::Arc::new(identity);
            let node = identity.id();
            diagnostic.entry.context.node = Some(node);
            let store = Store::open(
                path.clone(),
                identity.id(),
                catalog,
                authorization,
                skills,
                environment,
                !skip_bundled,
            )
            .await?;
            let ingress = store.ingress.clone();
            ingress.computer.configure_worker(computer).await;
            let network =
                match sailry_link::Link::bind(&identity, network, ingress.clone(), ingress.clone())
                    .await
                {
                    Ok(network) => network,
                    Err(error) => {
                        store.shutdown().await?;
                        return Err(error.into());
                    }
                };
            let link = network.handle();
            if let Err(error) = store.bind_link(link.clone()).await {
                let _ = network.close().await;
                let _ = store.shutdown().await;
                return Err(error);
            }
            let supervisor = Supervisor::start(TASK_CAPACITY);
            if let Err(error) = agent::start(ingress.clone(), &supervisor.tasks)
                .and_then(|()| dispatch::start(ingress.clone(), &supervisor.tasks))
            {
                ingress.agents.stop();
                ingress.dispatch.stop.cancel();
                let _ = supervisor.shutdown(SHUTDOWN_GRACE).await;
                let _ = ingress.agents.close().await;
                let _ = network.close().await;
                let _ = store.shutdown().await;
                return Err(error);
            }
            let (state, receiver) = watch::channel(State::Running);
            let observer = Observer {
                state: receiver,
                tasks: supervisor.tasks.clone(),
            };
            let (stop, stopping) = oneshot::channel();
            let agents = ingress.agents.clone();
            let worker = tokio::spawn(async move {
                let _ = stopping.await;
                let mut entry = logging::Entry::new(logging::Area::Node, logging::Phase::Begin);
                entry.context.node = Some(node);
                entry.reason = Some("shutdown");
                let mut diagnostic = logging::Span::new(entry);
                state.send_replace(State::Stopping);
                store.stop_admission();
                agents.stop();
                let result = supervisor.shutdown(SHUTDOWN_GRACE).await;
                let connections = agents.close().await.map_err(Error::Link);
                let network = network.close().await.map_err(Error::Link);
                let storage = store.shutdown().await;
                drop(profile);
                state.send_replace(State::Stopped);
                let result = result.and(connections).and(network).and(storage);
                match &result {
                    Ok(()) => diagnostic.finish(logging::Phase::Completed, None),
                    Err(error) => {
                        diagnostic.finish(logging::Phase::Failed, Some(logging::fault(error)))
                    }
                }
                result
            });
            Ok(Self {
                path,
                observer,
                stop: Some(stop),
                worker,
                identity,
                ingress,
                link,
            })
        }
        .await;
        match &result {
            Ok(_) => diagnostic.finish(logging::Phase::Completed, None),
            Err(error) => diagnostic.finish(logging::Phase::Failed, Some(logging::fault(error))),
        }
        result
    }

    pub fn profile(&self) -> &Path {
        &self.path
    }

    pub fn observe(&self) -> Observer {
        self.observer.clone()
    }

    pub fn id(&self) -> sailry_protocol::NodeId {
        self.identity.id()
    }

    pub fn local(&self) -> std::sync::Arc<dyn sailry_link::Transport> {
        std::sync::Arc::new(sailry_link::Local::new(
            self.id(),
            self.id(),
            self.ingress.clone(),
        ))
    }

    pub fn link(&self) -> sailry_link::LinkHandle {
        self.link.clone()
    }

    /// Execution-engine access only. No Client command exposes stored secret values.
    pub async fn resolve_credential(
        &self,
        reference: sailry_protocol::CredentialRef,
        provider: sailry_protocol::ProviderId,
    ) -> Result<sailry_protocol::Secret, sailry_protocol::Fault> {
        self.ingress.resolve_credential(reference, provider).await
    }

    pub async fn shutdown(mut self) -> Result<(), Error> {
        self.stop.take();
        (&mut self.worker)
            .await
            .map_err(|error| Error::Worker(error.to_string()))?
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        // Cleanup owns the lock even if shutdown's caller or the Node handle is dropped.
        self.stop.take();
    }
}

#[cfg(test)]
mod tests;
