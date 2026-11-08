use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, Output};

type Key = (ProviderId, String);
#[cfg(test)]
mod tests;

#[derive(Default)]
struct Sources(BTreeMap<NodeId, WeakEntity<Host>>);
impl Global for Sources {}

#[derive(Default)]
pub(super) struct Cache {
    pub status: catalog::ScoreStatus,
    loaded: BTreeSet<Key>,
    rows: BTreeMap<Key, Vec<catalog::EffortScore>>,
}

impl Cache {
    fn accept(&mut self, status: catalog::ScoreStatus) {
        if status.revision < self.status.revision
            || status.revision == self.status.revision
                && status.checked_at_ms < self.status.checked_at_ms
        {
            return;
        }
        if self.status.revision != status.revision {
            self.loaded.clear();
            self.rows.clear();
        }
        self.status = status;
    }

    fn merge(&mut self, references: &[catalog::ModelRef], scores: catalog::Scores) {
        if self.status.revision != scores.revision {
            return;
        }
        self.loaded.extend(references.iter().map(key));
        for score in scores.scores {
            let reference = (score.provider, score.model);
            if self.loaded.contains(&reference) {
                self.rows.insert(reference, score.efforts);
            }
        }
    }
}

fn key(reference: &catalog::ModelRef) -> Key {
    (reference.provider, reference.model.clone())
}

fn stale(status: &catalog::ScoreStatus, now: u64) -> bool {
    status
        .checked_at_ms
        .is_none_or(|checked| now.saturating_sub(checked) >= 86_400_000)
}

pub(super) struct Host {
    binding: Binding,
    pub cache: Cache,
    connected: bool,
    known: bool,
    pending: BTreeSet<Key>,
    reading: bool,
    refresh_requested: bool,
    refreshing: bool,
    stop: CancellationToken,
    watcher: Option<Task<()>>,
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(super) fn get(binding: &Binding, cx: &mut App) -> Entity<Host> {
    if !cx.has_global::<Sources>() {
        cx.set_global(Sources::default());
    }
    let node = binding.client.target();
    if let Some(host) = cx
        .global::<Sources>()
        .0
        .get(&node)
        .and_then(WeakEntity::upgrade)
    {
        return host;
    }
    let host = cx.new(|_| Host {
        binding: binding.clone(),
        cache: Default::default(),
        connected: false,
        known: false,
        pending: Default::default(),
        reading: false,
        refresh_requested: false,
        refreshing: false,
        stop: CancellationToken::new(),
        watcher: None,
    });
    cx.global_mut::<Sources>().0.insert(node, host.downgrade());
    host
}

impl Host {
    pub fn scores(&self, reference: &catalog::ModelRef) -> Option<&[catalog::EffortScore]> {
        self.cache.rows.get(&key(reference)).map(Vec::as_slice)
    }

    pub fn request(&mut self, references: Vec<catalog::ModelRef>, cx: &mut Context<Self>) {
        if references.is_empty() {
            return;
        }
        self.pending.extend(references.iter().map(key));
        self.refresh_requested = true;
        if self.watcher.is_none() {
            let (sender, mut receiver) =
                tokio::sync::watch::channel(sailry_client::View::default());
            let client = self.binding.client.clone();
            let stop = self.stop.clone();
            self.binding.runtime.spawn(async move {
                let _ = client.watch(sender, stop).await;
            });
            self.watcher = Some(cx.spawn(async move |owner, cx| {
                while receiver.changed().await.is_ok() {
                    let view = receiver.borrow_and_update().clone();
                    let _ = owner.update(cx, |host, cx| {
                        let Some(snapshot) = view.snapshot else {
                            return;
                        };
                        let status = snapshot.model_catalog.scores;
                        let changed = !host.known
                            || host.connected != view.connected
                            || host.cache.status != status;
                        host.known = true;
                        host.connected = view.connected;
                        if changed {
                            if host.known && host.cache.status.revision != status.revision {
                                host.pending.clear();
                            }
                            host.cache.accept(status);
                            host.flush(cx);
                            cx.notify();
                        }
                    });
                }
            }));
        }
        self.flush(cx);
    }

    fn flush(&mut self, cx: &mut Context<Self>) {
        if !self.known || !self.connected {
            return;
        }
        if self.refresh_requested && !self.refreshing {
            self.refresh_requested = false;
            let now = u64::try_from(chrono::Utc::now().timestamp_millis()).unwrap_or_default();
            if stale(&self.cache.status, now) {
                self.refreshing = true;
                let client = self.binding.client.clone();
                let stop = self.stop.clone();
                let job = self.binding.runtime.spawn(async move {
                    tokio::select! {
                        _ = stop.cancelled() => None,
                        result = client.execute(client.prepare(Command::RefreshModelCatalog)) => Some(result),
                    }
                });
                cx.spawn(async move |owner, cx| {
                    let result = job.await;
                    let _ = owner.update(cx, |host, cx| {
                        host.refreshing = false;
                        if let Ok(Some(Ok(Output::CatalogStatus(status)))) = result {
                            host.cache.accept(status.scores);
                            host.flush(cx);
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        if self.reading {
            return;
        }
        let references = self
            .pending
            .difference(&self.cache.loaded)
            .map(|(provider, model)| catalog::ModelRef {
                provider: *provider,
                model: model.clone(),
            })
            .collect::<Vec<_>>();
        if references.is_empty() {
            return;
        }
        self.reading = true;
        let requested = references.clone();
        let client = self.binding.client.clone();
        let stop = self.stop.clone();
        let revision = self.cache.status.revision;
        let job = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                result = client.execute(client.prepare(Command::ReadModelScores(requested))) => Some(result),
            }
        });
        cx.spawn(async move |owner, cx| {
            let result = job.await;
            let _ = owner.update(cx, |host, cx| {
                host.reading = false;
                if revision == host.cache.status.revision {
                    for reference in &references {
                        host.pending.remove(&key(reference));
                    }
                }
                if let Ok(Some(Ok(Output::ModelScores(scores)))) = result {
                    let current = scores.revision == host.cache.status.revision;
                    host.cache.merge(&references, scores);
                    if current || revision != host.cache.status.revision {
                        host.flush(cx);
                    }
                } else {
                    host.flush(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}
