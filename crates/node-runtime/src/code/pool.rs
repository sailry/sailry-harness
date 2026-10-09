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
    pub(super) settings: std::sync::RwLock<code::Settings>,
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
            closed,
            state: Mutex::new(State::default()),
            settings: Default::default(),
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
    ) -> Result<code::Installation, Fault> {
        self.install.install(install, stop).await
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
        let path = self.install.path(package, version)?;
        let mut state = self.state.lock().await;
        let keys = state
            .slots
            .iter()
            .filter(|(_, slot)| slot.server.plan.executable.starts_with(&path))
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(slot) = state.slots.remove(&key) {
                slot.server.rpc.shutdown().await;
            }
        }
        self.install.uninstall(package, version)
    }

    pub(crate) fn configure(&self, settings: code::Settings) {
        *self
            .settings
            .write()
            .expect("code settings lock is healthy") = settings;
    }
    pub(crate) fn after_edit(&self) -> bool {
        self.settings
            .read()
            .expect("code settings lock is healthy")
            .after_edit
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
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let Some(path) = query.path() else {
            return Ok(None);
        };
        let profile = self.profile.clone();
        let settings = self
            .settings
            .read()
            .expect("code settings lock is healthy")
            .clone();
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
        let Some(plan) = resolved.plan else {
            return Ok(None);
        };
        let startup = plan.startup.clone();
        let mut state = self.state.lock().await;
        state.cache(plan)?;
        let key = key(&startup);
        let warm = if let Some(slot) = state.slots.get(&key) {
            slot.server.plan.startup == startup
                && matches!(
                    slot.server.rpc.state.lock().await.status,
                    Status::Ready | Status::Indexing
                )
        } else {
            false
        };
        Ok((!warm).then_some(startup))
    }

    pub(crate) async fn validate(&self, startup: &Startup, root: &Path) -> Result<(), Fault> {
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
        self.validate(startup, root).await?;
        let mut state = self.state.lock().await;
        let key = key(startup);
        let mut restarts = state
            .failures
            .get(&key)
            .filter(|(revision, _)| revision == &startup.revision)
            .map_or(0, |(_, count)| *count);
        if let Some(slot) = state.slots.get(&key)
            && slot.server.plan.startup == *startup
        {
            let status = slot.server.rpc.state.lock().await.status;
            if matches!(status, Status::Ready | Status::Indexing) {
                return info(&slot.server).await;
            }
            restarts = slot.restarts + 1;
            if restarts > 3 {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "language server stopped after three restart attempts",
                ));
            }
        }
        if restarts > 3 {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "language server stopped after three restart attempts",
            ));
        }
        if restarts > 0 {
            tokio::select! { biased; _ = stop.cancelled() => return Err(cancelled()), _ = self.closed.cancelled() => return Err(unavailable()), _ = tokio::time::sleep(Duration::from_millis(200 << (restarts-1))) => {} }
        }
        if let Some(slot) = state.slots.remove(&key) {
            slot.server.rpc.shutdown().await;
        }
        if state.slots.len() >= CAPACITY {
            let mut idle = None;
            for (key, slot) in &state.slots {
                if Arc::strong_count(&slot.server) == 1 {
                    let used = *slot.server.used.lock().await;
                    if idle.as_ref().is_none_or(|(_, earlier)| used < *earlier) {
                        idle = Some((key.clone(), used));
                    }
                }
            }
            let (key, _) =
                idle.ok_or_else(|| Fault::new(ErrorCode::Busy, "all language servers are in use"))?;
            if let Some(slot) = state.slots.remove(&key) {
                slot.server.rpc.shutdown().await;
            }
        }
        let plan = state.prepared[&startup.revision].clone();
        let server = match Server::start(plan, &self.environment, self.closed.clone(), stop).await {
            Ok(server) => server,
            Err(error) => {
                state
                    .failures
                    .insert(key, (startup.revision.clone(), restarts + 1));
                return Err(error);
            }
        };
        state.failures.remove(&key);
        let result = info(&server).await?;
        state.slots.insert(key, Slot { server, restarts });
        Ok(result)
    }

    pub(crate) async fn query(
        &self,
        worktree: WorktreeId,
        root: PathBuf,
        query: &Query,
        overlay: Option<&Overlay>,
        stop: &CancellationToken,
    ) -> Result<code::Result, Fault> {
        let candidate = if let Some(path) = query.path() {
            let profile = self.profile.clone();
            let settings = self
                .settings
                .read()
                .expect("code settings lock is healthy")
                .clone();
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
        if let Some(plan) = candidate
            .as_ref()
            .and_then(|candidate| candidate.plan.as_ref())
        {
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
        let status = server.rpc.state.lock().await.status;
        if !matches!(status, Status::Ready | Status::Indexing) {
            let mut result = operations::empty(query, None);
            result.status = status;
            result.startup = Some(server.plan.startup.clone());
            result.server = Some(server.plan.startup.server.clone());
            result.notice = Some("Language server has stopped".into());
            return Ok(result);
        }
        operations::read(&server, query, overlay, stop).await
    }

    pub(crate) async fn list(&self, worktree: WorktreeId) -> Result<Vec<code::Server>, Fault> {
        let state = self.state.lock().await;
        let mut servers = Vec::new();
        for ((id, _, _), slot) in &state.slots {
            if *id == worktree {
                servers.push(info(&slot.server).await?);
            }
        }
        Ok(servers)
    }

    pub(crate) async fn changed(
        &self,
        worktree: WorktreeId,
        paths: &[String],
        stop: &CancellationToken,
    ) -> Result<Vec<code::Diagnostic>, Fault> {
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
            if !matches!(
                server.rpc.state.lock().await.status,
                Status::Ready | Status::Indexing
            ) {
                continue;
            }
            for path in paths {
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
                    Err(error) => return Err(error),
                }
                if !self.after_edit() {
                    continue;
                }
                server.settle(stop).await;
                errors.extend(
                    diagnostics::read(&server, path, Some(code::Severity::Error), stop)
                        .await?
                        .into_iter()
                        .take(20 - errors.len()),
                );
                if errors.len() >= 20 {
                    return Ok(errors);
                }
            }
        }
        Ok(errors)
    }

    pub(crate) async fn watched(&self, worktree: WorktreeId) {
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
        let mut state = self.state.lock().await;
        let keys = state
            .slots
            .keys()
            .filter(|(id, _, _)| *id == worktree)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(slot) = state.slots.remove(&key) {
                slot.server.rpc.shutdown().await;
            }
        }
        state
            .prepared
            .retain(|_, plan| plan.startup.worktree != worktree);
        state.failures.retain(|(id, _, _), _| *id != worktree);
    }

    async fn reap(&self) {
        let mut state = self.state.lock().await;
        let mut stale = Vec::new();
        for (key, slot) in &state.slots {
            if Arc::strong_count(&slot.server) == 1
                && Instant::now().duration_since(*slot.server.used.lock().await) >= IDLE
            {
                stale.push(key.clone());
            }
        }
        for key in stale {
            if let Some(slot) = state.slots.remove(&key) {
                slot.server.rpc.shutdown().await;
            }
        }
    }

    pub(crate) async fn shutdown(&self) {
        self.install.shutdown();
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
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "language startup was cancelled")
}

#[cfg(test)]
mod tests;
