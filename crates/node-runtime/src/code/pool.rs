use super::{
    diagnostics, documents, operations,
    resolve::{self, Plan},
    server::Server,
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault, WorktreeId,
    code::{self, Overlay, Query, Startup, Status},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const CAPACITY: usize = 4;
const IDLE: Duration = Duration::from_secs(10 * 60);
type Key = (WorktreeId, String, String);

struct Slot {
    server: Arc<Server>,
    restarts: u8,
    active: CancellationToken,
}
struct Configuration {
    settings: code::Settings,
    stop: CancellationToken,
}
#[derive(Default)]
struct State {
    prepared: BTreeMap<String, Plan>,
    failures: BTreeMap<Key, (String, u8)>,
    slots: BTreeMap<Key, Slot>,
}

impl State {
    fn cache(&mut self, plan: Plan) -> Result<(), Fault> {
        let revision = &plan.startup.revision;
        if self.prepared.len() >= 512 && !self.prepared.contains_key(revision) {
            return Err(Fault::new(
                ErrorCode::Busy,
                "language discovery cache is full",
            ));
        }
        self.prepared.entry(revision.clone()).or_insert(plan);
        Ok(())
    }
}

pub(crate) struct Pool {
    pub(super) profile: Option<PathBuf>,
    pub(super) environment: Arc<crate::process::Environment>,
    identities: Arc<super::identity::Cache>,
    closed: CancellationToken,
    state: Mutex<State>,
    lifecycle: Mutex<()>,
    configuration: std::sync::RwLock<Configuration>,
    pub(super) rules: Arc<super::rules::Registry>,
    pub(super) install: Arc<super::install::Manager>,
}

impl Pool {
    pub(crate) fn new(
        profile: Option<PathBuf>,
        environment: Arc<crate::process::Environment>,
        closed: CancellationToken,
    ) -> Arc<Self> {
        let network = Arc::new(super::network::Network::default());
        let rules = Arc::new(super::rules::Registry::new(
            profile.clone(),
            network.clone(),
        ));
        let install = Arc::new(super::install::Manager::new(
            profile.clone(),
            environment.clone(),
            rules.clone(),
            network,
        ));
        let pool = Arc::new(Self {
            profile,
            environment,
            identities: Arc::default(),
            configuration: std::sync::RwLock::new(Configuration {
                settings: code::Settings::default(),
                stop: closed.child_token(),
            }),
            closed,
            state: Mutex::new(State::default()),
            lifecycle: Mutex::new(()),
            rules,
            install,
        });
        let weak = Arc::downgrade(&pool);
        let closed = pool.closed.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! { biased; _ = closed.cancelled() => return, _ = tokio::time::sleep(Duration::from_secs(30)) => {} }
                let Some(pool) = weak.upgrade() else {
                    return;
                };
                pool.reap().await;
            }
        });
        pool
    }

    pub(crate) fn package(&self, name: &str) -> Result<Option<code::Package>, Fault> {
        Ok(self
            .rules
            .get()?
            .packages
            .iter()
            .find(|package| package.name == name)
            .map(|package| super::install::package(package, &self.environment)))
    }
    pub(crate) fn validate_install(&self, install: &code::Install) -> Result<(), Fault> {
        self.install.validate(install)
    }
    pub(crate) fn installations(&self) -> Result<Vec<code::Installation>, Fault> {
        self.install.list()
    }
    pub(crate) fn cancel_install(&self, package: &str) -> Result<code::Installation, Fault> {
        self.install.cancel(package)
    }
    pub(crate) async fn install(
        &self,
        install: &code::Install,
        stop: &CancellationToken,
        reporter: Option<&crate::installation::Reporter>,
    ) -> Result<code::Installation, Fault> {
        self.install.install(install, stop, reporter).await
    }
    pub(crate) fn rules_status(&self) -> Result<code::Rules, Fault> {
        self.rules.status()
    }
    pub(crate) async fn update_rules(
        &self,
        stop: &CancellationToken,
    ) -> Result<code::Rules, Fault> {
        self.rules.refresh(stop).await
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fixture(&self, endpoint: &str) {
        self.install.network.fixture(endpoint)
    }
    pub(crate) async fn uninstall(&self, package: &str, version: &str) -> Result<(), Fault> {
        let _lifecycle = self.lifecycle.lock().await;
        let mut state = self.state.lock().await;
        let path = self.install.validate_uninstall(package, version)?;
        let keys = state
            .slots
            .iter()
            .filter(|(_, slot)| slot.server.plan.executable.starts_with(&path))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        if keys.iter().any(|key| {
            state
                .slots
                .get(key)
                .is_some_and(|slot| Arc::strong_count(&slot.server) > 1)
        }) {
            return Err(Fault::new(ErrorCode::Busy, "language package is in use"));
        }
        // Keep starts excluded until the owned processes and files have been removed.
        state
            .prepared
            .retain(|_, plan| !plan.executable.starts_with(&path));
        let removed = keys
            .into_iter()
            .filter_map(|key| state.slots.remove(&key))
            .collect::<Vec<_>>();
        drop(state);
        for slot in removed {
            slot.server.rpc.shutdown().await;
        }
        self.install.uninstall(package, version)
    }

    /// Publish policy synchronously; process cleanup never holds up the Store actor.
    pub(crate) fn configure(
        self: &Arc<Self>,
        settings: code::Settings,
    ) -> impl std::future::Future<Output = ()> + Send + 'static + use<> {
        {
            let mut configuration = self
                .configuration
                .write()
                .expect("code settings lock is healthy");
            if !settings.enabled {
                configuration.stop.cancel();
            } else if !configuration.settings.enabled {
                configuration.stop = self.closed.child_token();
            }
            configuration.settings = settings;
        }
        let pool = self.clone();
        async move {
            let _lifecycle = pool.lifecycle.lock().await;
            let removed = {
                let mut state = pool.state.lock().await;
                // A delayed cleanup observes current policy, never its stale caller's settings.
                let settings = pool
                    .configuration
                    .read()
                    .expect("code settings lock is healthy")
                    .settings
                    .clone();
                let allowed = |server: &str| {
                    settings.enabled
                        && !settings
                            .servers
                            .get(server)
                            .is_some_and(|value| value.disabled)
                };
                state
                    .prepared
                    .retain(|_, plan| allowed(&plan.startup.server));
                state.failures.retain(|(_, _, server), _| allowed(server));
                let keys = state
                    .slots
                    .iter()
                    .filter(|((_, _, server), slot)| !allowed(server) || slot.active.is_cancelled())
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>();
                keys.into_iter()
                    .filter_map(|key| state.slots.remove(&key))
                    .collect::<Vec<_>>()
            };
            for slot in removed {
                slot.server.rpc.shutdown().await;
            }
        }
    }
    fn available(&self, server: &str) -> Result<(), Fault> {
        let configuration = self
            .configuration
            .read()
            .expect("code settings lock is healthy");
        if !configuration.settings.enabled
            || configuration
                .settings
                .servers
                .get(server)
                .is_some_and(|value| value.disabled)
        {
            Err(disabled())
        } else {
            Ok(())
        }
    }
    pub(super) fn settings(&self) -> code::Settings {
        self.configuration
            .read()
            .expect("code settings lock is healthy")
            .settings
            .clone()
    }
    pub(crate) fn after_edit(&self) -> bool {
        let configuration = self
            .configuration
            .read()
            .expect("code settings lock is healthy");
        let settings = &configuration.settings;
        settings.enabled && settings.after_edit
    }
    pub(crate) fn enabled(&self) -> bool {
        self.configuration
            .read()
            .expect("code settings lock is healthy")
            .settings
            .enabled
    }
    fn capture(&self) -> Result<(code::Settings, CancellationToken), Fault> {
        let configuration = self
            .configuration
            .read()
            .expect("code settings lock is healthy");
        if !configuration.settings.enabled {
            return Err(disabled());
        }
        if self.closed.is_cancelled() || configuration.stop.is_cancelled() {
            return Err(unavailable());
        }
        Ok((configuration.settings.clone(), configuration.stop.clone()))
    }
    fn current(&self, stop: &CancellationToken) -> Result<(), Fault> {
        if !self.enabled() {
            Err(disabled())
        } else if self.closed.is_cancelled() || stop.is_cancelled() {
            Err(unavailable())
        } else {
            Ok(())
        }
    }

    pub(super) fn resolver(&self, rules: Arc<super::rules::Rules>) -> resolve::Resolver {
        resolve::Resolver {
            rules,
            identities: self.identities.clone(),
        }
    }

    pub(crate) async fn prepare(
        &self,
        worktree: WorktreeId,
        root: PathBuf,
        query: &Query,
    ) -> Result<Option<Startup>, Fault> {
        let (settings, active) = self.capture()?;
        let Some(path) = query.path() else {
            return Ok(None);
        };
        let profile = self.profile.clone();
        let environment = self.environment.clone();
        let path = path.to_owned();
        let resolver = self.resolver(self.rules.get()?);
        let resolved = tokio::task::spawn_blocking(move || {
            resolver.resolve(
                worktree,
                &root,
                profile.as_deref(),
                &environment,
                &path,
                &settings,
            )
        })
        .await
        .map_err(|error| Fault::invalid(error.to_string()))??;
        self.current(&active)?;
        let Some(plan) = resolved.plan else {
            return Ok(None);
        };
        let startup = plan.startup.clone();
        let mut state = self.state.lock().await;
        self.current(&active)?;
        self.available(&startup.server)?;
        state.cache(plan)?;
        let key = key(&startup);
        let server = state
            .slots
            .get(&key)
            .filter(|slot| !slot.active.is_cancelled())
            .map(|slot| slot.server.clone());
        drop(state);
        let warm = if let Some(server) = server {
            server.plan.startup == startup
                && matches!(
                    server.rpc.state.lock().await.status,
                    Status::Ready | Status::Indexing
                )
        } else {
            false
        };
        self.current(&active)?;
        Ok((!warm).then_some(startup))
    }

    pub(crate) async fn validate(&self, startup: &Startup, root: &Path) -> Result<(), Fault> {
        let (_, active) = self.capture()?;
        let plan = self
            .state
            .lock()
            .await
            .prepared
            .get(&startup.revision)
            .cloned()
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::PermissionDenied,
                    "language startup was not prepared by this Node",
                )
            })?;
        if &plan.startup != startup || plan.root != root {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "language startup differs from the registered worktree",
            ));
        }
        documents::directory(root, &startup.root)?;
        let executable = plan.executable.clone();
        let expected = plan.executable_revision.clone();
        let identities = self.identities.clone();
        let actual = tokio::task::spawn_blocking(move || identities.revision(&executable))
            .await
            .map_err(|error| Fault::invalid(error.to_string()))??;
        self.current(&active)?;
        if actual != expected {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "language executable changed after startup approval",
            ));
        }
        Ok(())
    }

    pub(crate) async fn start(
        &self,
        startup: &Startup,
        root: &Path,
        stop: &CancellationToken,
    ) -> Result<code::Server, Fault> {
        let (_, active) = self.capture()?;
        let _lifecycle = tokio::select! {
            biased;
            _=stop.cancelled()=>return Err(cancelled()),
            _=active.cancelled()=>return Err(self.current(&active).unwrap_err()),
            guard=self.lifecycle.lock()=>guard,
        };
        self.available(&startup.server)?;
        self.validate(startup, root).await?;
        let key = key(startup);
        let (existing, mut restarts, plan) = {
            let state = self.state.lock().await;
            self.current(&active)?;
            let existing = state.slots.get(&key).map(|slot| {
                (
                    slot.server.clone(),
                    slot.restarts,
                    !slot.active.is_cancelled(),
                )
            });
            let restarts = state
                .failures
                .get(&key)
                .filter(|(revision, _)| revision == &startup.revision)
                .map_or(0, |(_, count)| *count);
            let plan = state
                .prepared
                .get(&startup.revision)
                .cloned()
                .ok_or_else(|| {
                    Fault::new(
                        ErrorCode::PermissionDenied,
                        "language startup was not prepared by this Node",
                    )
                })?;
            (existing, restarts, plan)
        };
        if let Some((server, attempts, valid)) = existing
            && valid
            && server.plan.startup == *startup
        {
            let status = server.rpc.state.lock().await.status;
            if matches!(status, Status::Ready | Status::Indexing) {
                let result = info(&server).await;
                self.current(&active)?;
                self.available(&startup.server)?;
                return result;
            }
            restarts = attempts + 1;
        }
        if restarts > 3 {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "language server stopped after three restart attempts",
            ));
        }
        if restarts > 0 {
            tokio::select! {biased;_=stop.cancelled()=>return Err(cancelled()),_=active.cancelled()=>return Err(self.current(&active).unwrap_err()),_=tokio::time::sleep(Duration::from_millis(200 << (restarts-1)))=>{}}
        }
        let removed = self.state.lock().await.slots.remove(&key);
        if let Some(slot) = removed {
            slot.server.rpc.shutdown().await;
        }
        let candidates = {
            let state = self.state.lock().await;
            if state.slots.len() < CAPACITY {
                None
            } else {
                Some(
                    state
                        .slots
                        .iter()
                        .filter(|(_, slot)| Arc::strong_count(&slot.server) == 1)
                        .map(|(key, slot)| (key.clone(), slot.server.clone()))
                        .collect::<Vec<_>>(),
                )
            }
        };
        if let Some(candidates) = candidates {
            let mut idle = Vec::new();
            for (key, server) in candidates {
                let used = *server.used.lock().await;
                idle.push((used, key, server));
            }
            idle.sort_by_key(|(used, _, _)| *used);
            let removed = {
                let mut state = self.state.lock().await;
                let key = idle
                    .iter()
                    .find(|(_, _, server)| Arc::strong_count(server) == 2)
                    .map(|(_, key, _)| key.clone())
                    .ok_or_else(|| {
                        Fault::new(ErrorCode::Busy, "all language servers are in use")
                    })?;
                state.slots.remove(&key)
            };
            if let Some(slot) = removed {
                slot.server.rpc.shutdown().await;
            }
        }
        self.current(&active)?;
        self.available(&startup.server)?;
        let server = match Server::start(plan, &self.environment, active.clone(), stop).await {
            Ok(server) => server,
            Err(error) => {
                let mut state = self.state.lock().await;
                self.current(&active)?;
                self.available(&startup.server)?;
                state
                    .failures
                    .insert(key, (startup.revision.clone(), restarts + 1));
                return Err(error);
            }
        };
        let result = info(&server).await;
        if let Err(error) = self
            .current(&active)
            .and_then(|()| self.available(&startup.server))
        {
            server.rpc.shutdown().await;
            return Err(error);
        }
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                server.rpc.shutdown().await;
                return Err(error);
            }
        };
        let mut state = self.state.lock().await;
        // Policy may change while waiting for the metadata lock.
        if let Err(error) = self
            .current(&active)
            .and_then(|()| self.available(&startup.server))
        {
            drop(state);
            server.rpc.shutdown().await;
            return Err(error);
        }
        state.failures.remove(&key);
        state.slots.insert(
            key,
            Slot {
                server,
                restarts,
                active,
            },
        );
        Ok(result)
    }

    pub(crate) async fn stop(&self, startup: &Startup, root: &Path) -> Result<code::Server, Fault> {
        let _lifecycle = self.lifecycle.lock().await;
        let slot = {
            let mut state = self.state.lock().await;
            let key = key(startup);
            let slot = state.slots.get(&key).ok_or_else(|| {
                Fault::new(ErrorCode::NotFound, "language server is no longer running")
            })?;
            if slot.server.plan.startup != *startup {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "language server changed before stopping",
                ));
            }
            if slot.server.plan.root != root {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "language server belongs to another registered worktree",
                ));
            }
            let slot = state
                .slots
                .remove(&key)
                .expect("checked language slot exists");
            state.failures.remove(&key);
            slot
        };
        slot.server.rpc.shutdown().await;
        let mut stopped = info(&slot.server).await?;
        stopped.status = Status::Stopped;
        stopped.progress = None;
        Ok(stopped)
    }

    pub(crate) async fn query(
        &self,
        worktree: WorktreeId,
        root: PathBuf,
        query: &Query,
        overlay: Option<&Overlay>,
        stop: &CancellationToken,
    ) -> Result<code::Result, Fault> {
        let (settings, active) = self.capture()?;
        let candidate = if let Some(path) = query.path() {
            let profile = self.profile.clone();
            let environment = self.environment.clone();
            let path = path.to_owned();
            let discovery_root = root.clone();
            let resolver = self.resolver(self.rules.get()?);
            Some(
                tokio::task::spawn_blocking(move || {
                    resolver.resolve(
                        worktree,
                        &discovery_root,
                        profile.as_deref(),
                        &environment,
                        &path,
                        &settings,
                    )
                })
                .await
                .map_err(|error| Fault::invalid(error.to_string()))??,
            )
        } else {
            None
        };
        let mut state = self.state.lock().await;
        self.current(&active)?;
        if let Some(plan) = candidate
            .as_ref()
            .and_then(|candidate| candidate.plan.as_ref())
        {
            self.available(&plan.startup.server)?;
            state.cache(plan.clone())?;
        }
        let server = if let Some(candidate) = &candidate {
            candidate
                .plan
                .as_ref()
                .and_then(|plan| state.slots.get(&key(&plan.startup)))
                .filter(|slot| {
                    candidate
                        .plan
                        .as_ref()
                        .is_some_and(|plan| plan.startup == slot.server.plan.startup)
                })
                .map(|slot| slot.server.clone())
        } else {
            state
                .slots
                .iter()
                .find(|((id, _, _), _)| *id == worktree)
                .map(|(_, slot)| slot.server.clone())
        };
        drop(state);
        let Some(server) = server else {
            let mut result = operations::empty(
                query,
                candidate
                    .as_ref()
                    .and_then(|candidate| candidate.package.clone()),
            );
            if let Some(plan) = candidate.and_then(|candidate| candidate.plan) {
                result.status = Status::Stopped;
                result.server = Some(plan.startup.server.clone());
                result.startup = Some(plan.startup);
                result.notice = Some("Language server startup requires approval".into());
            }
            return Ok(result);
        };
        self.available(&server.plan.startup.server)?;
        let (status, failure) = {
            let state = server.rpc.state.lock().await;
            (state.status, state.failure.clone())
        };
        self.current(&active)?;
        if !matches!(status, Status::Ready | Status::Indexing) {
            let mut result = operations::empty(
                query,
                candidate
                    .as_ref()
                    .and_then(|candidate| candidate.package.clone()),
            );
            result.status = status;
            result.startup = Some(server.plan.startup.clone());
            result.server = Some(server.plan.startup.server.clone());
            result.notice = failure.or_else(|| Some("Language server has stopped".into()));
            return Ok(result);
        }
        let result = operations::read(&server, query, overlay, stop).await;
        self.current(&active)?;
        result
    }

    pub(crate) async fn list(&self, worktree: WorktreeId) -> Result<Vec<code::Server>, Fault> {
        let current = self
            .state
            .lock()
            .await
            .slots
            .iter()
            .filter(|((id, _, _), _)| *id == worktree)
            .map(|(_, slot)| slot.server.clone())
            .collect::<Vec<_>>();
        let mut servers = Vec::new();
        for server in current {
            servers.push(info(&server).await?);
        }
        Ok(servers)
    }

    pub(crate) async fn changed(
        &self,
        worktree: WorktreeId,
        paths: &[String],
        stop: &CancellationToken,
    ) -> Result<Vec<code::Diagnostic>, Fault> {
        let Ok((settings, active)) = self.capture() else {
            return Ok(Vec::new());
        };
        let servers = self
            .state
            .lock()
            .await
            .slots
            .iter()
            .filter(|((id, _, _), _)| *id == worktree)
            .map(|(_, slot)| slot.server.clone())
            .collect::<Vec<_>>();
        let mut errors = Vec::new();
        for server in servers {
            if self.current(&active).is_err() {
                return Ok(Vec::new());
            }
            if !matches!(
                server.rpc.state.lock().await.status,
                Status::Ready | Status::Indexing
            ) {
                continue;
            }
            for path in paths {
                if self.current(&active).is_err() {
                    return Ok(Vec::new());
                }
                if !server
                    .plan
                    .root
                    .join(path)
                    .starts_with(&server.plan.directory)
                {
                    continue;
                }
                if let Ok(text) = documents::read(&server.plan.root, path)
                    && server
                        .plan
                        .rules
                        .language(path, &text.text)
                        .is_none_or(|language| {
                            language.id != server.plan.language
                                && !language.servers.contains(&server.plan.startup.server)
                        })
                {
                    continue;
                }
                match server.synchronize(path, None, true).await {
                    Ok(()) => {}
                    Err(error) if error.code == ErrorCode::NotFound => {
                        server.close(path).await?;
                        continue;
                    }
                    Err(_) if self.current(&active).is_err() => return Ok(Vec::new()),
                    Err(error) => return Err(error),
                }
                if !settings.after_edit {
                    continue;
                }
                server.settle(stop).await;
                let diagnostics =
                    diagnostics::read(&server, path, Some(code::Severity::Error), stop).await;
                if self.current(&active).is_err() {
                    return Ok(Vec::new());
                }
                errors.extend(diagnostics?.into_iter().take(20 - errors.len()));
                if errors.len() >= 20 {
                    return Ok(errors);
                }
            }
        }
        Ok(errors)
    }

    pub(crate) async fn watched(&self, worktree: WorktreeId) {
        let Ok((_, active)) = self.capture() else {
            return;
        };
        let servers = self
            .state
            .lock()
            .await
            .slots
            .iter()
            .filter(|((id, _, _), _)| *id == worktree)
            .map(|(_, slot)| slot.server.clone())
            .collect::<Vec<_>>();
        for server in servers {
            if self.current(&active).is_err() {
                return;
            }
            let paths = server
                .documents
                .lock()
                .await
                .0
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            let changes = paths
                .into_iter()
                .filter_map(|path| documents::uri(&server.plan.root, &path).ok())
                .map(|uri| serde_json::json!({"uri":uri,"type":2}))
                .collect::<Vec<_>>();
            let _ = server
                .rpc
                .notify(
                    "workspace/didChangeWatchedFiles",
                    serde_json::json!({"changes":changes}),
                )
                .await;
        }
    }

    pub(crate) async fn remove(&self, worktree: WorktreeId) {
        let _lifecycle = self.lifecycle.lock().await;
        let removed = {
            let mut state = self.state.lock().await;
            let keys = state
                .slots
                .keys()
                .filter(|(id, _, _)| *id == worktree)
                .cloned()
                .collect::<Vec<_>>();
            state
                .prepared
                .retain(|_, plan| plan.startup.worktree != worktree);
            state.failures.retain(|(id, _, _), _| *id != worktree);
            keys.into_iter()
                .filter_map(|key| state.slots.remove(&key))
                .collect::<Vec<_>>()
        };
        for slot in removed {
            slot.server.rpc.shutdown().await;
        }
    }

    async fn reap(&self) {
        let _lifecycle = self.lifecycle.lock().await;
        let candidates = self
            .state
            .lock()
            .await
            .slots
            .iter()
            .filter(|(_, slot)| Arc::strong_count(&slot.server) == 1)
            .map(|(key, slot)| (key.clone(), slot.server.clone()))
            .collect::<Vec<_>>();
        let mut stale = Vec::new();
        for (key, server) in candidates {
            if Instant::now().duration_since(*server.used.lock().await) >= IDLE {
                stale.push((key, server));
            }
        }
        let removed = {
            let mut state = self.state.lock().await;
            stale
                .iter()
                .filter(|(_, server)| Arc::strong_count(server) == 2)
                .filter_map(|(key, _)| state.slots.remove(key))
                .collect::<Vec<_>>()
        };
        for slot in removed {
            slot.server.rpc.shutdown().await;
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.configuration
            .read()
            .expect("code settings lock is healthy")
            .stop
            .cancel();
        self.install.shutdown();
        let _lifecycle = self.lifecycle.lock().await;
        let slots = std::mem::take(&mut self.state.lock().await.slots);
        futures::future::join_all(
            slots
                .into_values()
                .map(|slot| async move { slot.server.rpc.shutdown().await }),
        )
        .await;
    }
}

fn key(startup: &Startup) -> Key {
    (
        startup.worktree,
        startup.root.clone(),
        startup.server.clone(),
    )
}
async fn info(server: &Server) -> Result<code::Server, Fault> {
    let state = server.rpc.state.lock().await;
    Ok(code::Server {
        startup: server.plan.startup.clone(),
        status: state.status,
        progress: state.progress.clone(),
        diagnostics_revision: state.diagnostics_revision,
    })
}
fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "language services are unavailable")
}
pub(crate) fn disabled() -> Fault {
    Fault::new(ErrorCode::Unavailable, "language services are disabled")
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "language startup was cancelled")
}

#[cfg(test)]
mod tests;
