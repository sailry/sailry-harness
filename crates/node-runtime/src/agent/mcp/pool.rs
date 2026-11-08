use super::*;
use crate::plugins::resources::mcp::{Configured, Launch};
use std::{
    collections::BTreeMap,
    sync::{Mutex, Weak},
    time::Instant,
};

use tokio::sync::{Notify, OnceCell};

const MAX_CONNECTIONS: usize = 32;
const IDLE_TTL: Duration = Duration::from_secs(30 * 60);
type Services = BTreeMap<(String, String), Weak<tokio::sync::Mutex<()>>>;
pub(super) type Cleanup = Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    package: String,
    digest: String,
    settings: u64,
    server: String,
    authorization: [u8; 32],
    launch: [u8; 32],
}

impl Key {
    fn new(
        config: &Configured,
        launch: &Launch,
        connector: Option<&crate::plugins::connectors::mcp::Authorization>,
    ) -> Result<Self, Fault> {
        let authorization = connector.map(|connector| {
            let injected: Vec<_> = connector
                .targets
                .iter()
                .filter_map(|(_, injection)| {
                    let name = injection.env.as_deref()?;
                    match launch {
                        Launch::Stdio { env, .. } => Some((name, env.get(name))),
                        _ => None,
                    }
                })
                .collect();
            json!([connector.key(), injected])
        });
        let launch = match launch {
            Launch::Stdio {
                command,
                args,
                env,
                cwd,
                settings,
                computer,
                ..
            } => {
                let mut env = env.clone();
                if settings.is_some() {
                    env.remove("SAILRY_PLUGIN_SETTINGS_FILE");
                }
                json!([command, args, env, cwd, computer])
            }
            Launch::Http { url, headers, sse } => json!([url, headers, sse]),
        };
        let fingerprint = |value: &Value| {
            serde_json::to_vec(value)
                .map(|bytes| *blake3::hash(&bytes).as_bytes())
                .map_err(|_| unavailable())
        };
        Ok(Self {
            package: config.reference.name.clone(),
            digest: config.reference.digest.clone(),
            settings: config.reference.settings_revision,
            server: config.name.clone(),
            authorization: fingerprint(&json!(authorization))?,
            launch: fingerprint(&launch)?,
        })
    }
}

struct Entry {
    connection: OnceCell<Result<Arc<Connected>, Fault>>,
    ready: Notify,
    stop: CancellationToken,
    created: Instant,
}

impl Entry {
    fn failed(&self) -> bool {
        self.stop.is_cancelled()
            || self
                .connection
                .get()
                .is_some_and(|result| !result.as_ref().is_ok_and(|connection| connection.healthy()))
    }

    fn idle(&self) -> Option<Instant> {
        self.connection.get().and_then(|result| match result {
            Ok(connection) => {
                (Arc::strong_count(connection) == 1).then(|| *connection.used.lock().unwrap())
            }
            Err(_) => Some(self.created),
        })
    }

    async fn get(&self) -> Result<Arc<Connected>, Fault> {
        loop {
            let ready = self.ready.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            if let Some(connection) = self.connection.get() {
                return connection.clone();
            }
            ready.await;
        }
    }

    async fn close(&self, force: bool) -> Result<(), Fault> {
        self.stop.cancel();
        match self.get().await {
            Ok(connection) => {
                connection.retired.store(true, Ordering::Relaxed);
                if connection.closed.load(Ordering::Relaxed)
                    || (!force && Arc::strong_count(&connection) > 2)
                {
                    Ok(())
                } else {
                    connection.close().await
                }
            }
            Err(_) => Ok(()),
        }
    }
}

#[derive(Default)]
pub(crate) struct Pool {
    entries: Mutex<BTreeMap<Key, Arc<Entry>>>,
    current: Mutex<BTreeMap<String, sailry_protocol::plugin::Summary>>,
    initialized: AtomicBool,
    services: Mutex<Services>,
    cleanup: Cleanup,
    status: status::Status,
    stopped: CancellationToken,
}

impl Pool {
    pub(crate) fn observe(&self, summary: &sailry_protocol::plugin::Summary) -> bool {
        let mut current = self.current.lock().unwrap();
        if let Some(previous) = current.get(&summary.name)
            && (previous.revision, previous.settings_revision)
                > (summary.revision, summary.settings_revision)
        {
            return false;
        }
        let changed = current.get(&summary.name).is_none_or(|previous| {
            previous.reference() != summary.reference()
                || previous.available() != summary.available()
        });
        current.insert(summary.name.clone(), summary.clone());
        if changed {
            self.status.remove(Some(&summary.name));
        }
        changed
    }

    pub(crate) fn project(&self, info: &mut sailry_protocol::plugin::Info) {
        self.status.project(info);
    }

    pub(crate) async fn changes(&self) -> Vec<String> {
        self.status.changes().await
    }

    pub(super) fn begin(&self, reference: &Reference, server: &str) -> Option<status::Attempt> {
        self.reusable(reference)
            .then(|| self.status.begin(reference, server))
    }

    pub(super) fn started(&self, reference: &Reference, server: &str) {
        self.status.started(reference, server);
    }

    pub(super) fn finish(&self, attempt: Option<status::Attempt>, result: Result<usize, Fault>) {
        if let Some(attempt) = attempt {
            self.status.finish(attempt, result);
        }
    }

    pub(super) fn failed(&self, connection: &Arc<Connected>) {
        if connection.healthy() {
            return;
        }
        let retired = self
            .entries
            .lock()
            .unwrap()
            .extract_if(.., |_, entry| {
                entry.connection.get().is_some_and(|result| {
                    result
                        .as_ref()
                        .is_ok_and(|current| Arc::ptr_eq(current, connection))
                })
            })
            .map(|(key, entry)| {
                self.status.failed(&connection.reference, &key.server);
                entry
            })
            .collect();
        self.retire(retired);
    }

    pub(super) fn reconnect(&self, package: &str, server: &str) {
        let retired = self
            .entries
            .lock()
            .unwrap()
            .extract_if(.., |key, _| key.package == package && key.server == server)
            .map(|(_, entry)| entry)
            .collect();
        self.retire(retired);
    }

    pub(super) fn reusable(&self, reference: &Reference) -> bool {
        self.current
            .lock()
            .unwrap()
            .get(&reference.name)
            .map_or(!self.initialized.load(Ordering::Relaxed), |summary| {
                summary.available() && summary.reference() == *reference
            })
    }

    pub(crate) fn finish_inventory(&self) {
        self.initialized.store(true, Ordering::Relaxed);
    }

    pub(super) fn service(&self, package: &str, server: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut services = self.services.lock().unwrap();
        services.retain(|_, lock| lock.strong_count() > 0);
        let key = (package.to_owned(), server.to_owned());
        if let Some(lock) = services.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        services.insert(key, Arc::downgrade(&lock));
        lock
    }

    pub(super) async fn lease(
        &self,
        config: &Configured,
        launch: Launch,
        connector: Option<crate::plugins::connectors::mcp::Authorization>,
        ingress: &Arc<Ingress>,
        stop: &CancellationToken,
        retry_failed: bool,
    ) -> Result<(Arc<Connected>, bool), Fault> {
        let key = Key::new(config, &launch, connector.as_ref())?;
        let mut retired = Vec::new();
        let selected = (|| {
            let mut entries = self.entries.lock().unwrap();
            if self.stopped.is_cancelled() || stop.is_cancelled() {
                return Err(cancelled());
            }
            retired.extend(
                entries
                    .extract_if(.., |candidate, entry| {
                        (candidate == &key
                            && ((retry_failed && entry.failed())
                                || entry.idle().is_some_and(|used| used.elapsed() >= IDLE_TTL)))
                            || (candidate.package == key.package
                                && candidate.server == key.server
                                && ((candidate.digest == key.digest
                                    && candidate.settings == key.settings
                                    && candidate.authorization != key.authorization)
                                    || ((candidate.digest != key.digest
                                        || candidate.settings != key.settings)
                                        && entry.idle().is_some())))
                    })
                    .map(|(_, entry)| entry),
            );
            if let Some(entry) = entries.get(&key) {
                return Ok((entry.clone(), true));
            }
            if entries.len() >= MAX_CONNECTIONS {
                let oldest = entries
                    .iter()
                    .filter_map(|(key, entry)| entry.idle().map(|used| (key.clone(), used)))
                    .min_by_key(|(_, used)| *used);
                if let Some((key, _)) = oldest {
                    retired.extend(entries.remove(&key));
                }
            }
            if entries.len() >= MAX_CONNECTIONS {
                return Err(Fault::new(ErrorCode::Busy, "MCP connection pool is busy"));
            }
            let entry = Arc::new(Entry {
                connection: OnceCell::new(),
                ready: Notify::new(),
                stop: self.stopped.child_token(),
                created: Instant::now(),
            });
            entries.insert(key, entry.clone());
            let opening = entry.clone();
            let reference = config.reference.clone();
            let environment = ingress.environment.clone();
            let cleanup = self.cleanup.clone();
            tokio::spawn(async move {
                let result = Connected::open(
                    reference,
                    launch,
                    connector,
                    true,
                    &opening.stop,
                    &environment,
                )
                .await
                .map(|mut connection| {
                    connection.cleanup = Some(cleanup);
                    Arc::new(connection)
                });
                let _ = opening.connection.set(result);
                opening.ready.notify_waiters();
            });
            Ok((entry, false))
        })();
        self.retire(retired);
        let (entry, reused) = selected?;
        if !reused {
            self.started(&config.reference, &config.name);
        }
        let connection = tokio::select! {
            _ = stop.cancelled() => return Err(cancelled()),
            _ = self.stopped.cancelled() => return Err(cancelled()),
            result = entry.get() => result?,
        };
        if entry.stop.is_cancelled() || !connection.healthy() {
            return Err(unavailable());
        }
        *connection.used.lock().unwrap() = Instant::now();
        Ok((connection, reused))
    }

    pub(crate) async fn invalidate(&self, package: Option<&str>) -> Result<(), Fault> {
        self.status.remove(package);
        if let Some(package) = package
            && let Some(summary) = self.current.lock().unwrap().get_mut(package)
        {
            summary.enabled = false;
        }
        let retired = {
            let mut entries = self.entries.lock().unwrap();
            entries
                .extract_if(.., |key, _| {
                    package.is_none_or(|package| key.package == package)
                })
                .map(|(_, entry)| entry)
                .collect()
        };
        if self.stopped.is_cancelled() {
            close(retired, true).await
        } else {
            self.retire(retired);
            Ok(())
        }
    }

    pub(crate) async fn refresh(
        &self,
        package: &sailry_protocol::plugin::Summary,
    ) -> Result<bool, Fault> {
        let changed = self.observe(package);
        let package = self.current.lock().unwrap()[&package.name].clone();
        self.refresh_matching(|key| {
            (key.package == package.name).then_some(
                package.available()
                    && key.digest == package.digest
                    && key.settings == package.settings_revision,
            )
        })
        .await?;
        Ok(changed)
    }

    pub(super) async fn reconcile(&self) -> Result<(), Fault> {
        let current = self.current.lock().unwrap().clone();
        self.refresh_matching(|key| {
            current.get(&key.package).map(|summary| {
                summary.available()
                    && key.digest == summary.digest
                    && key.settings == summary.settings_revision
            })
        })
        .await
    }

    async fn refresh_matching(&self, current: impl Fn(&Key) -> Option<bool>) -> Result<(), Fault> {
        let candidates: Vec<_> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(key, entry)| {
                current(key).map(|valid| (key.clone(), entry.clone(), valid))
            })
            .collect();
        let retired = futures::future::join_all(candidates.into_iter().map(
            |(key, entry, valid)| async move {
                let invalid = if !valid {
                    true
                } else {
                    match entry.get().await {
                        Ok(connection) => match &connection.connector {
                            Some(connector) => !matches!(
                                tokio::time::timeout(Duration::from_secs(10), connector.validate())
                                    .await,
                                Ok(Ok(()))
                            ),
                            None => !connection.healthy(),
                        },
                        Err(_) => false,
                    }
                };
                if invalid {
                    let mut entries = self.entries.lock().unwrap();
                    if entries
                        .get(&key)
                        .is_some_and(|current| Arc::ptr_eq(current, &entry))
                    {
                        if valid
                            && let Some(Ok(connection)) = entry.connection.get()
                            && !connection.healthy()
                        {
                            self.status.failed(&connection.reference, &key.server);
                        }
                        return entries.remove(&key);
                    }
                }
                None
            },
        ))
        .await;
        self.retire(retired.into_iter().flatten().collect());
        Ok(())
    }

    pub(crate) async fn expire(&self) -> Result<(), Fault> {
        let retired = {
            let mut entries = self.entries.lock().unwrap();
            entries
                .extract_if(.., |_, entry| {
                    entry.idle().is_some_and(|used| used.elapsed() >= IDLE_TTL)
                })
                .map(|(_, entry)| entry)
                .collect()
        };
        self.retire(retired);
        Ok(())
    }

    pub(crate) async fn close(&self) -> Result<(), Fault> {
        self.stopped.cancel();
        let result = self.invalidate(None).await;
        loop {
            let cleanup = std::mem::take(&mut *self.cleanup.lock().unwrap());
            if cleanup.is_empty() {
                break;
            }
            for task in cleanup {
                let _ = task.await;
            }
        }
        result
    }

    fn retire(&self, entries: Vec<Arc<Entry>>) {
        let mut cleanup = self.cleanup.lock().unwrap();
        cleanup.retain(|task| !task.is_finished());
        for entry in entries {
            entry.stop.cancel();
            if let Some(Ok(connection)) = entry.connection.get() {
                connection.retired.store(true, Ordering::Relaxed);
            }
            cleanup.push(tokio::spawn(async move {
                if let Err(error) = entry.close(false).await {
                    let mut log = crate::logging::Entry::new(
                        crate::logging::Area::Run,
                        crate::logging::Phase::Failed,
                    );
                    log.stage = Some("mcp_server");
                    log.reason = Some("connection_cleanup");
                    log.code = Some(error.code);
                    log.emit_warn();
                }
            }));
        }
    }
}

async fn close(entries: Vec<Arc<Entry>>, force: bool) -> Result<(), Fault> {
    futures::future::join_all(
        entries
            .into_iter()
            .map(|entry| async move { entry.close(force).await }),
    )
    .await
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests;
