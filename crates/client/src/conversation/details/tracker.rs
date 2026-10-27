//! One presentation owns selected values; their allocations share the Client budget.
use super::*;
use std::collections::BTreeSet;
use std::time::Duration;
use tokio::{runtime::Handle, sync::watch, task::AbortHandle, time::Instant};

#[derive(Clone, Debug)]
pub enum State {
    Loading,
    Ready(Loaded),
    Failed {
        fault: Fault,
        retry_at: Option<Instant>,
    },
    TooLarge(Limit),
    Superseded,
}

#[derive(Clone)]
pub struct Tracker(Arc<Tracked>);
struct Tracked {
    client: Arc<Client>,
    runtime: Handle,
    slots: Mutex<Slots>,
    changed: watch::Sender<u64>,
}
#[derive(Default)]
struct Slots {
    values: BTreeMap<Reference, Slot>,
    next: u64,
    dirty: BTreeSet<Reference>,
}
struct Slot {
    state: State,
    task: AbortHandle,
    id: u64,
    revision: Option<u64>,
    reported: bool,
}

impl Tracker {
    pub fn new(client: Arc<Client>, runtime: Handle) -> Self {
        Self(Arc::new(Tracked {
            client,
            runtime,
            slots: Mutex::default(),
            changed: watch::channel(0).0,
        }))
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.changed.subscribe()
    }
    pub fn changes(&self) -> Vec<Reference> {
        std::mem::take(&mut self.0.slots.lock().expect("tool tracker lock").dirty)
            .into_iter()
            .collect()
    }
    pub fn references(&self) -> Vec<Reference> {
        self.0
            .slots
            .lock()
            .expect("tool tracker lock")
            .values
            .keys()
            .cloned()
            .collect()
    }
    pub fn set(&self, references: impl IntoIterator<Item = Reference>) {
        let references = references.into_iter().collect::<BTreeSet<_>>();
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        let removed = slots
            .values
            .keys()
            .filter(|reference| !references.contains(*reference))
            .cloned()
            .collect::<Vec<_>>();
        for reference in removed {
            self.0.remove(&mut slots, &reference);
        }
        for reference in references {
            let current = self.0.client.details.revision(reference.session);
            if slots
                .values
                .get(&reference)
                .is_some_and(|slot| current.is_none_or(|revision| Some(revision) == slot.revision))
            {
                continue;
            }
            Self::start(&self.0, &mut slots, reference);
        }
    }
    pub fn include(&self, reference: Reference) {
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        if !slots.values.contains_key(&reference) {
            Self::start(&self.0, &mut slots, reference);
        }
    }
    pub fn remove(&self, reference: &Reference) {
        self.0.remove(
            &mut self.0.slots.lock().expect("tool tracker lock"),
            reference,
        );
    }
    pub fn retry(&self, reference: &Reference) {
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        let state = self.0.state(&mut slots, reference);
        if matches!(state, State::Loading) {
            return;
        }
        if matches!(state, State::Ready(_)) {
            slots.next += 1;
            let id = slots.next;
            if let Some(slot) = slots.values.get_mut(reference) {
                slot.id = id;
            }
            return;
        }
        Self::start(&self.0, &mut slots, reference.clone());
    }
    pub fn state(&self, reference: &Reference) -> State {
        self.0.state(
            &mut self.0.slots.lock().expect("tool tracker lock"),
            reference,
        )
    }
    pub fn failures(&self) -> Vec<(Reference, Fault)> {
        self.0
            .slots
            .lock()
            .expect("tool tracker lock")
            .values
            .iter_mut()
            .filter_map(|(reference, slot)| {
                if slot.reported {
                    return None;
                }
                let fault = match &slot.state {
                    State::Failed {
                        fault,
                        retry_at: None,
                    } => fault.clone(),
                    State::TooLarge(limit) => {
                        Fault::new(ErrorCode::InvalidRequest, limit.message())
                    }
                    _ => return None,
                };
                slot.reported = true;
                Some((reference.clone(), fault))
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
    fn start(tracked: &Arc<Tracked>, slots: &mut Slots, reference: Reference) {
        if let Some(previous) = slots.values.remove(&reference) {
            previous.task.abort();
            if let State::Ready(detail) = previous.state {
                tracked.client.details.unpin(&reference, &detail);
            }
        } else {
            tracked.client.details.track(reference.session);
        }
        slots.next += 1;
        let id = slots.next;
        let revision = tracked.client.details.revision(reference.session);
        let weak = Arc::downgrade(tracked);
        let client = tracked.client.clone();
        let query = reference.clone();
        let task = tracked.runtime.spawn(async move {
            let delays = [
                Duration::ZERO,
                Duration::from_millis(200),
                Duration::from_secs(1),
                Duration::from_secs(3),
            ];
            for (index, delay) in delays.into_iter().enumerate() {
                if index != 0 {
                    tokio::time::sleep(delay).await;
                    let Some(tracked) = weak.upgrade() else {
                        return;
                    };
                    if !tracked.publish(&query, id, State::Loading) {
                        return;
                    }
                }
                let result = client.detail(query.clone()).await;
                let Some(tracked) = weak.upgrade() else {
                    return;
                };
                let state = match result {
                    Ok(Read::Ready(detail)) => State::Ready(detail),
                    Ok(Read::TooLarge(limit)) => State::TooLarge(limit),
                    Ok(Read::Superseded) => State::Superseded,
                    Err(fault) => State::Failed {
                        fault,
                        retry_at: delays.get(index + 1).map(|delay| Instant::now() + *delay),
                    },
                };
                let retry = matches!(
                    state,
                    State::Failed {
                        retry_at: Some(_),
                        ..
                    }
                );
                if !tracked.publish(&query, id, state) || !retry {
                    return;
                }
            }
        });
        slots.values.insert(
            reference.clone(),
            Slot {
                state: State::Loading,
                task: task.abort_handle(),
                id,
                revision,
                reported: false,
            },
        );
        tracked.notify(slots, reference);
    }
}
impl Tracked {
    fn notify(&self, slots: &mut Slots, reference: Reference) {
        if slots.values.contains_key(&reference) {
            slots.dirty.insert(reference);
        } else {
            slots.dirty.remove(&reference);
        }
        self.changed.send_modify(|sequence| *sequence += 1);
    }
    fn remove(&self, slots: &mut Slots, reference: &Reference) {
        if let Some(slot) = slots.values.remove(reference) {
            slot.task.abort();
            if let State::Ready(detail) = slot.state {
                self.client.details.unpin(reference, &detail);
            }
            self.client.details.untrack(reference.session);
            self.notify(slots, reference.clone());
        }
    }
    fn state(&self, slots: &mut Slots, reference: &Reference) -> State {
        let Some(slot) = slots.values.get_mut(reference) else {
            return State::Superseded;
        };
        if let State::Ready(detail) = &slot.state
            && !self.client.details.valid(reference, detail)
        {
            self.client.details.unpin(reference, detail);
            slot.state = State::Superseded;
            self.notify(slots, reference.clone());
        }
        slots.values[reference].state.clone()
    }
    fn publish(&self, reference: &Reference, id: u64, mut state: State) -> bool {
        let mut slots = self.slots.lock().expect("tool tracker lock");
        let Some(slot) = slots.values.get(reference) else {
            return false;
        };
        if slot.id != id {
            return false;
        }
        if self
            .client
            .details
            .revision(reference.session)
            .is_some_and(|revision| Some(revision) != slot.revision)
        {
            state = State::Superseded;
        }
        if let State::Ready(detail) = &state {
            let mut replace = slots
                .values
                .iter()
                .filter_map(|(reference, slot)| match &slot.state {
                    State::Ready(detail) => Some((slot.id, reference, detail)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            replace.sort_unstable_by_key(|(id, _, _)| *id);
            match self.client.details.pin(
                reference,
                detail,
                replace
                    .into_iter()
                    .map(|(_, reference, detail)| (reference, detail)),
            ) {
                Some(evicted) => {
                    for reference in evicted {
                        let slot = slots.values.get_mut(&reference).unwrap();
                        slot.state = State::TooLarge(Limit::Memory);
                        slot.reported = false;
                        self.notify(&mut slots, reference);
                    }
                }
                None => state = State::TooLarge(Limit::Memory),
            }
        }
        slots.values.get_mut(reference).unwrap().state = state;
        self.notify(&mut slots, reference.clone());
        true
    }
}
impl Drop for Tracked {
    fn drop(&mut self) {
        for (reference, slot) in &self.slots.get_mut().expect("tool tracker lock").values {
            slot.task.abort();
            if let State::Ready(detail) = &slot.state {
                self.client.details.unpin(reference, detail);
            }
            self.client.details.untrack(reference.session);
        }
    }
}
