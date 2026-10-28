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
    capacity_task: Option<AbortHandle>,
}
struct Slot {
    state: State,
    task: AbortHandle,
    id: u64,
    revision: Option<u64>,
    reported: bool,
    protected: bool,
    accessed: u64,
    required: Option<usize>,
}

impl Tracker {
    pub fn new(client: Arc<Client>, runtime: Handle) -> Self {
        let mut capacity = client.details.capacity.subscribe();
        let tracked = Arc::new(Tracked {
            client,
            runtime,
            slots: Mutex::default(),
            changed: watch::channel(0).0,
        });
        let weak = Arc::downgrade(&tracked);
        let task = tracked.runtime.spawn(async move {
            while capacity.changed().await.is_ok() {
                let Some(tracked) = weak.upgrade() else {
                    return;
                };
                let mut slots = tracked.slots.lock().expect("tool tracker lock");
                let ready = slots
                    .values
                    .iter()
                    .filter_map(|(reference, slot)| {
                        let required = slot.required?;
                        tracked
                            .client
                            .details
                            .available(reference, required)
                            .then(|| reference.clone())
                    })
                    .collect::<Vec<_>>();
                for reference in ready {
                    // Recovery may use free capacity, never replace another visible value.
                    Self::start(&tracked, &mut slots, reference, false);
                }
            }
        });
        tracked
            .slots
            .lock()
            .expect("tool tracker lock")
            .capacity_task = Some(task.abort_handle());
        Self(tracked)
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.changed.subscribe()
    }
    pub fn changes(&self) -> Vec<Reference> {
        std::mem::take(&mut self.0.slots.lock().expect("tool tracker lock").dirty)
            .into_iter()
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
            Self::select(&self.0, &mut slots, reference);
        }
    }
    pub fn retain(&self, mut keep: impl FnMut(&Reference) -> bool) {
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        let (references, removed): (Vec<_>, Vec<_>) = slots
            .values
            .keys()
            .cloned()
            .partition(|reference| keep(reference));
        for reference in removed {
            self.0.remove(&mut slots, &reference);
        }
        for reference in references {
            Self::select(&self.0, &mut slots, reference);
        }
    }
    pub fn include(&self, reference: Reference) {
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        Self::select(&self.0, &mut slots, reference);
    }
    /// Pending approval payloads cannot be replaced by another selection.
    pub fn protect(&self, references: impl IntoIterator<Item = Reference>) {
        let references = references.into_iter().collect::<BTreeSet<_>>();
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        for (reference, slot) in &mut slots.values {
            slot.protected = references.contains(reference);
        }
        for reference in references {
            if !slots.values.contains_key(&reference) {
                Self::start(&self.0, &mut slots, reference.clone(), true);
            }
            slots.values.get_mut(&reference).unwrap().protected = true;
        }
    }
    pub fn remove(&self, reference: &Reference) {
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        if slots
            .values
            .get(reference)
            .is_some_and(|slot| slot.protected)
        {
            return;
        }
        self.0.remove(&mut slots, reference);
    }
    pub fn retry(&self, reference: &Reference) {
        let mut slots = self.0.slots.lock().expect("tool tracker lock");
        let state = self.0.state(&mut slots, reference);
        if matches!(state, State::Loading | State::Ready(_)) {
            return;
        }
        Self::start(&self.0, &mut slots, reference.clone(), true);
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
            let (state, waiting) = {
                let mut slots = self.0.slots.lock().expect("tool tracker lock");
                let state = self.0.state(&mut slots, reference);
                let waiting = slots
                    .values
                    .get(reference)
                    .is_some_and(|slot| slot.required.is_some());
                (state, waiting)
            };
            if !waiting
                && !matches!(
                    state,
                    State::Loading
                        | State::Failed {
                            retry_at: Some(_),
                            ..
                        }
                )
            {
                return state;
            }
            if changed.changed().await.is_err() {
                return State::Superseded;
            }
        }
    }
    fn touch(slots: &mut Slots, reference: &Reference) {
        slots.next += 1;
        if let Some(slot) = slots.values.get_mut(reference) {
            slot.accessed = slots.next;
        }
    }
    fn select(tracked: &Arc<Tracked>, slots: &mut Slots, reference: Reference) {
        let current = tracked.client.details.revision(reference.session);
        if let Some(slot) = slots.values.get(&reference)
            && current.is_none_or(|revision| Some(revision) == slot.revision)
        {
            if slot
                .required
                .is_some_and(|bytes| tracked.client.details.available(&reference, bytes))
            {
                Self::start(tracked, slots, reference, false);
            } else {
                Self::touch(slots, &reference);
            }
        } else {
            Self::start(tracked, slots, reference, true);
        }
    }
    fn start(tracked: &Arc<Tracked>, slots: &mut Slots, reference: Reference, replace: bool) {
        let protected = slots
            .values
            .get(&reference)
            .is_some_and(|slot| slot.protected);
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
                    if !tracked.publish(&query, id, State::Loading, replace) {
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
                if !tracked.publish(&query, id, state, replace) || !retry {
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
                protected,
                accessed: id,
                required: None,
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
    fn publish(
        &self,
        reference: &Reference,
        id: u64,
        mut state: State,
        replace_ready: bool,
    ) -> bool {
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
        let mut required = None;
        if let State::Ready(detail) = &state {
            let mut replace = slots
                .values
                .iter()
                .filter_map(|(reference, slot)| match &slot.state {
                    State::Ready(detail) if replace_ready && !slot.protected => {
                        Some((slot.accessed, reference, detail))
                    }
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
                        slot.required = match &slot.state {
                            State::Ready(detail) => Some(detail.bytes),
                            _ => unreachable!("only ready values are replaced"),
                        };
                        slot.state = contended();
                        slot.reported = true;
                        self.notify(&mut slots, reference);
                    }
                }
                None => {
                    required = Some(detail.bytes);
                    state = contended();
                    slots.values.get_mut(reference).unwrap().reported = !replace_ready;
                }
            }
        }
        let slot = slots.values.get_mut(reference).unwrap();
        slot.required = required;
        slot.state = state;
        self.notify(&mut slots, reference.clone());
        true
    }
}

fn contended() -> State {
    State::Failed {
        fault: Fault::new(ErrorCode::Busy, "tool detail display memory is in use"),
        retry_at: None,
    }
}
impl Drop for Tracked {
    fn drop(&mut self) {
        let slots = self.slots.get_mut().expect("tool tracker lock");
        if let Some(task) = &slots.capacity_task {
            task.abort();
        }
        for (reference, slot) in &slots.values {
            slot.task.abort();
            if let State::Ready(detail) = &slot.state {
                self.client.details.unpin(reference, detail);
            }
            self.client.details.untrack(reference.session);
        }
    }
}
