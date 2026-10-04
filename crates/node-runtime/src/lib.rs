//! Node lifecycle foundation. Behavioral references: sailry-code 23bc118c,
//! `harbor-host-runtime/src/{layout.rs,host_tasks.rs,runtime/shutdown.rs}`.
//! Only ownership and shutdown semantics are carried forward here; the old Agent,
//! service assemblers and transport implementations are not imported by this stage.

mod agent;
mod browser;
mod computer;
mod databases;
mod dispatch;
mod error;
mod external_browser;
mod files;
mod git;
mod host;
mod office;
mod plugins;
mod ports;
mod process;
mod profile;
mod providers;
mod ssh;
mod store;
mod tasks;
mod terminal;

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;

pub use computer::Worker as ComputerWorker;
pub use error::Error;
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

impl Node {
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
            network,
            Default::default(),
            Default::default(),
            Default::default(),
            computer,
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    /// Uses an isolated reference-catalog fixture for service and controller acceptance.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_with_catalog(path: impl AsRef<Path>, endpoint: &str) -> Result<Self, Error> {
        tokio::spawn(Self::start_owned(
            path.as_ref().to_owned(),
            NetworkScope::default(),
            providers::catalog::Catalog::new(endpoint.to_owned()),
            Default::default(),
            Default::default(),
            None,
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
            NetworkScope::default(),
            Default::default(),
            providers::login::Service::fixture(endpoint),
            Default::default(),
            None,
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
            NetworkScope::default(),
            Default::default(),
            Default::default(),
            crate::plugins::github::Github::fixture(endpoint),
            None,
        ))
        .await
        .map_err(|error| Error::Worker(error.to_string()))?
    }

    async fn start_owned(
        path: PathBuf,
        network_scope: NetworkScope,
        catalog: providers::catalog::Catalog,
        authorization: providers::login::Service,
        skills: crate::plugins::github::Github,
        computer: Option<ComputerWorker>,
    ) -> Result<Self, Error> {
        let (profile, identity) = tokio::task::spawn_blocking(move || {
            let profile = Profile::acquire(&path)?;
            let identity = sailry_link::Identity::open(&profile.path)?;
            Ok::<_, Error>((profile, identity))
        })
        .await
        .map_err(|error| Error::Worker(error.to_string()))??;
        let path = profile.path.clone();
        let identity = std::sync::Arc::new(identity);
        let store =
            Store::open(path.clone(), identity.id(), catalog, authorization, skills).await?;
        let ingress = store.ingress.clone();
        ingress.computer.configure_worker(computer).await;
        let network = match sailry_link::Link::bind(
            &identity,
            network_scope,
            ingress.clone(),
            ingress.clone(),
        )
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
            state.send_replace(State::Stopping);
            store.stop_admission();
            agents.stop();
            let result = supervisor.shutdown(SHUTDOWN_GRACE).await;
            let connections = agents.close().await.map_err(Error::Link);
            let network = network.close().await.map_err(Error::Link);
            let storage = store.shutdown().await;
            drop(profile);
            state.send_replace(State::Stopped);
            result.and(connections).and(network).and(storage)
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
