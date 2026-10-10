//! Bounded settings resources without live views, scripts, or configuration data.
use super::cache::{Cache, prepare};
use futures::{
    FutureExt as _,
    future::{BoxFuture, Shared},
};
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{Reference, Summary, desktop::Surface};
use std::{collections::VecDeque, sync::Arc};
#[cfg(test)]
mod tests;

// Bundle validation also bounds each entry's source and image bytes.
pub(crate) const CAPACITY: usize = 4;
type Job = Shared<BoxFuture<'static, Option<Result<Arc<Cache>, &'static str>>>>;

#[derive(Clone)]
pub(crate) struct Prepared {
    pub(super) package: Reference,
    job: Job,
}

impl Prepared {
    pub(super) async fn load(self) -> Option<Result<Arc<Cache>, &'static str>> {
        self.job.await
    }
}

struct Entry {
    prepared: Prepared,
    stop: CancellationToken,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub(crate) struct Resources {
    client: Arc<Client>,
    runtime: tokio::runtime::Handle,
    entries: VecDeque<Entry>,
}

impl Resources {
    pub(crate) fn new(client: Arc<Client>, runtime: tokio::runtime::Handle) -> Self {
        Self {
            client,
            runtime,
            entries: VecDeque::new(),
        }
    }

    pub(crate) fn remaining(&self) -> usize {
        CAPACITY - self.entries.len()
    }

    pub(crate) fn retain(&mut self, summaries: &[Summary]) {
        self.entries.retain(|entry| {
            summaries
                .iter()
                .any(|summary| summary.reference() == entry.prepared.package)
        });
    }

    pub(crate) fn prepare(&mut self, package: Reference) -> Prepared {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.prepared.package == package)
        {
            let entry = self.entries.remove(index).expect("existing resource entry");
            if !matches!(entry.prepared.job.peek(), Some(None | Some(Err(_)))) {
                let prepared = entry.prepared.clone();
                self.entries.push_back(entry);
                return prepared;
            }
        }
        while self.entries.len() >= CAPACITY {
            self.entries.pop_front();
        }
        let stop = CancellationToken::new();
        let job = prepare(
            self.client.clone(),
            self.runtime.clone(),
            package.clone(),
            Surface::Settings,
            stop.clone(),
        )
        .boxed()
        .shared();
        // Poll the same future for preloading and opening; neither duplicates IO.
        self.runtime.spawn({
            let job = job.clone();
            async move {
                let _ = job.await;
            }
        });
        let prepared = Prepared { package, job };
        self.entries.push_back(Entry {
            prepared: prepared.clone(),
            stop,
        });
        prepared
    }
}
