//! One presentation declares its references; Client owns their loading and lifetime.
use super::*;
use sailry_link::CancellationToken;
use std::time::Duration;
use tokio::time::Instant;
use tokio::{runtime::Handle, sync::watch, task::AbortHandle};

#[derive(Clone, Debug)]
pub enum State {
    Loading,
    Ready(Arc<Detail>),
    Failed {
        fault: Fault,
        retry_at: Option<Instant>,
    },
    TooLarge(Limit),
    Superseded,
}

pub struct Tracker(Arc<Tracked>);

struct Tracked {
    client: Arc<Client>,
    runtime: Handle,
    slots: Mutex<BTreeMap<Reference, Slot>>,
    fixed: bool,
    changed: watch::Sender<u64>,
    stop: CancellationToken,
}

enum Status {
    Ready(Weak<Detail>),
    Other(State),
}

struct Slot {
    state: Status,
    task: Option<AbortHandle>,
    generation: Arc<()>,
    revision: Option<u64>,
    reported: bool,
}

impl Tracker {
    pub fn new(client: Arc<Client>, runtime: Handle) -> Self {
        Self::create(client, runtime, true)
    }

    /// Observed history uses the ordinary LRU; an evicted old result is skipped until explicitly retried.
    pub fn observe(client: Arc<Client>, runtime: Handle) -> Self {
        Self::create(client, runtime, false)
    }

    fn create(client: Arc<Client>, runtime: Handle, fixed: bool) -> Self {
        let tracked = Arc::new(Tracked {
            client,
            runtime,
            fixed,
            slots: Mutex::default(),
            changed: watch::channel(0).0,
            stop: CancellationToken::new(),
        });
        let weak = Arc::downgrade(&tracked);
        let stop = tracked.stop.clone();
        let mut changes = tracked.client.details.changes.subscribe();
        tracked.runtime.spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.cancelled() => return,
                    changed = changes.changed() => if changed.is_err() { return; },
                }
                let Some(tracked) = weak.upgrade() else {
                    return;
                };
                let mut slots = tracked.slots.lock().expect("tool tracker lock");
                let mut changed = false;
                for (reference, slot) in slots.iter_mut() {
                    if matches!(slot.state, Status::Ready(_))
                        && (tracked.fixed && tracked.client.cached_tool_detail(reference).is_none()
                            || tracked
                                .client
                                .details
                                .revision(reference.session)
                                .is_some_and(|revision| Some(revision) != slot.revision))
                    {
                        if tracked.fixed {
                            tracked.client.details.unpin(reference);
                        }
                        slot.state = Status::Other(State::Superseded);
                        changed = true;
                    }
                }
                if changed {
                    tracked.changed.send_modify(|sequence| *sequence += 1);
                }
            }
        });
        Self(tracked)
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.changed.subscribe()
    }

    /// Only declared references remain pinned. Ready values share Client allocations.
    pub fn set(&self, references: impl IntoIterator<Item = Reference>) {
        let references = references
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        let previous = slots.len();
        slots.retain(|reference, slot| {
            if references.contains(reference) {
                return true;
            }
            if let Some(task) = &slot.task {
                task.abort();
            }
            if matches!(slot.state, Status::Ready(_)) {
                if self.0.fixed {
                    self.0.client.details.unpin(reference);
                }
            }
            self.0.client.details.untrack(reference.session);
            false
        });
        let removed = previous != slots.len();
        for reference in references {
            if slots.get(&reference).is_some_and(|slot| {
                let revision = self.0.client.details.revision(reference.session);
                revision.is_none_or(|revision| Some(revision) == slot.revision)
            }) {
                continue;
            }
            Self::start(&self.0, &mut slots, reference);
        }
        if removed {
            self.0.changed.send_modify(|sequence| *sequence += 1);
        }
    }

    pub fn retry(&self, reference: &Reference) {
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        if let Some(slot) = slots.get(reference) {
            if matches!(slot.state, Status::Other(State::Loading))
                || matches!(&slot.state, Status::Ready(detail) if detail.strong_count() != 0)
            {
                return;
            }
            if let Some(task) = &slot.task {
                task.abort();
            }
        }
        Self::start(&self.0, &mut slots, reference.clone());
    }

    pub fn state(&self, reference: &Reference) -> State {
        self.0
            .slots
            .lock()
            .expect("tool tracker lock")
            .get(reference)
            .map_or(State::Superseded, |slot| match &slot.state {
                Status::Ready(detail) => detail.upgrade().map_or(State::Superseded, State::Ready),
                Status::Other(state) => state.clone(),
            })
    }

    /// Presentation feedback consumes a terminal failure once per explicit attempt.
    pub fn failures(&self) -> Vec<(Reference, Fault)> {
        self.0
            .slots
            .lock()
            .expect("tool tracker lock")
            .iter_mut()
            .filter_map(|(reference, slot)| {
                if let Status::Other(State::Failed {
                    fault,
                    retry_at: None,
                }) = &slot.state
                    && !slot.reported
                {
                    slot.reported = true;
                    Some((reference.clone(), fault.clone()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn wait(&self, reference: &Reference) -> State {
        let mut changed = self.subscribe();
        loop {
            let state = self.state(reference);
            if !matches!(
                state,
                State::Loading
                    | State::Failed {
                        retry_at: Some(_),
                        ..
                    }
            ) {
                return state;
            }
            if changed.changed().await.is_err() {
                return State::Superseded;
            }
        }
    }

    fn start(tracked: &Arc<Tracked>, slots: &mut BTreeMap<Reference, Slot>, reference: Reference) {
        if let Some(previous) = slots.remove(&reference) {
            if let Some(task) = previous.task {
                task.abort();
            }
            if tracked.fixed && matches!(previous.state, Status::Ready(_)) {
                tracked.client.details.unpin(&reference);
            }
        } else {
            tracked.client.details.track(reference.session);
        }
        let generation = Arc::new(());
        let query_generation = generation.clone();
        let revision = tracked.client.details.revision(reference.session);
        let weak = Arc::downgrade(tracked);
        let client = tracked.client.clone();
        let stop = tracked.stop.clone();
        let query = reference.clone();
        let task = tracked.runtime.spawn(async move {
            for (index, delay) in [Duration::ZERO, Duration::from_millis(200), Duration::from_secs(1), Duration::from_secs(3)].into_iter().enumerate() {
                tokio::select! { _ = stop.cancelled() => return, _ = tokio::time::sleep(delay) => {} }
                let Some(tracked) = weak.upgrade() else { return; };
                if !tracked.publish(&query, &query_generation, State::Loading) { return; }
                drop(tracked);
                let result = tokio::select! { _ = stop.cancelled() => return, result = client.detail(query.clone()) => result };
                let Some(tracked) = weak.upgrade() else { return; };
                let state = match result {
                    Ok(Read::Ready(detail)) if !tracked.fixed || tracked.client.details.pin(&query, &detail) => State::Ready(detail),
                    Ok(Read::Ready(_)) => State::TooLarge(Limit::Memory),
                    Ok(Read::TooLarge(limit)) => State::TooLarge(limit),
                    Ok(Read::Superseded) => State::Superseded,
                    Err(fault) => State::Failed { fault, retry_at: [Duration::from_millis(200), Duration::from_secs(1), Duration::from_secs(3)].get(index).map(|delay| Instant::now() + *delay) },
                };
                let retry = matches!(state, State::Failed { retry_at: Some(_), .. });
                if !tracked.publish(&query, &query_generation, state) || !retry { return; }
            }
        });
        slots.insert(
            reference,
            Slot {
                state: Status::Other(State::Loading),
                task: Some(task.abort_handle()),
                generation,
                revision,
                reported: false,
            },
        );
        tracked.changed.send_modify(|sequence| *sequence += 1);
    }
}

impl Tracked {
    fn publish(&self, reference: &Reference, generation: &Arc<()>, state: State) -> bool {
        let mut slots = self.slots.lock().expect("tool tracker lock");
        let Some(slot) = slots.get_mut(reference) else {
            if self.fixed && matches!(state, State::Ready(_)) {
                self.client.details.unpin(reference);
            }
            return false;
        };
        if !Arc::ptr_eq(&slot.generation, generation) {
            if self.fixed && matches!(state, State::Ready(_)) {
                self.client.details.unpin(reference);
            }
            return false;
        }
        slot.state = match state {
            State::Ready(detail) => Status::Ready(Arc::downgrade(&detail)),
            state => Status::Other(state),
        };
        self.changed.send_modify(|sequence| *sequence += 1);
        true
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.stop.cancel();
        for (reference, slot) in self.slots.get_mut().expect("tool tracker lock") {
            if let Some(task) = &slot.task {
                task.abort();
            }
            if matches!(slot.state, Status::Ready(_)) {
                if self.fixed {
                    self.client.details.unpin(reference);
                }
            }
            self.client.details.untrack(reference.session);
        }
    }
}
