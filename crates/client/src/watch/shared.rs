//! One Node reducer per live transport, with independently cancellable observers.
use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

#[derive(Default)]
pub struct Observers {
    entries: Arc<Mutex<BTreeMap<usize, Weak<Entry>>>>,
}

struct Entry {
    updates: watch::Receiver<View>,
    stop: CancellationToken,
    key: usize,
    owner: Weak<Mutex<BTreeMap<usize, Weak<Entry>>>>,
    _transport: Arc<dyn sailry_link::Transport>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(owner) = self.owner.upgrade() {
            let mut entries = owner.lock().unwrap();
            if entries
                .get(&self.key)
                .is_some_and(|entry| std::ptr::eq(entry.as_ptr(), self))
            {
                entries.remove(&self.key);
            }
        }
    }
}

impl Observers {
    fn subscribe(&self, client: &Client) -> Arc<Entry> {
        // Node identity alone must not combine separately authenticated callers.
        let key = Arc::as_ptr(&client.transport) as *const () as usize;
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, entry| entry.strong_count() > 0);
        if let Some(entry) = entries.get(&key).and_then(Weak::upgrade) {
            return entry;
        }
        let stop = CancellationToken::new();
        let (sender, updates) = watch::channel(View::default());
        let entry = Arc::new(Entry {
            updates,
            stop: stop.clone(),
            key,
            owner: Arc::downgrade(&self.entries),
            _transport: client.transport.clone(),
        });
        entries.insert(key, Arc::downgrade(&entry));
        let client = Client::new(client.transport.clone());
        tokio::spawn(async move {
            let _ = client.watch_node(sender, stop).await;
        });
        entry
    }

    pub(super) async fn watch(
        &self,
        client: &Client,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let _closing = Closing(updates.clone());
        let entry = self.subscribe(client);
        let mut receiver = entry.updates.clone();
        loop {
            let view = receiver.borrow_and_update().clone();
            let failure = view
                .error
                .clone()
                .filter(|error| !matches!(error.code, ErrorCode::Busy | ErrorCode::Unavailable));
            updates.send_replace(view);
            if let Some(error) = failure {
                return Err(error);
            }
            tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                result = receiver.changed() => result.map_err(|_| {
                    Fault::new(ErrorCode::Unavailable, "Node observation stopped")
                })?,
            }
        }
    }
}

#[cfg(test)]
mod tests;
