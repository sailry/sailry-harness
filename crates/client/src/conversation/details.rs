//! Shared bounded values and presentation lifetimes for canonical tool details.
use crate::Client;
use futures::{
    FutureExt,
    future::{BoxFuture, WeakShared},
};
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, SessionId,
    conversation::{Page, Part},
    tool::detail::{DETAIL_PAGE_BYTES, Detail, Reference},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, Weak},
};

pub const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;

#[derive(Clone, Debug)]
pub struct Loaded {
    pub detail: Arc<Detail>,
    pub bytes: usize,
}

impl std::ops::Deref for Loaded {
    type Target = Detail;
    fn deref(&self) -> &Self::Target {
        &self.detail
    }
}

pub(crate) struct Cache {
    values: Mutex<Values>,
    reads: Arc<tokio::sync::Semaphore>,
    capacity: tokio::sync::watch::Sender<()>,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            values: Mutex::default(),
            reads: Arc::new(tokio::sync::Semaphore::new(4)),
            capacity: tokio::sync::watch::channel(()).0,
        }
    }
}
#[derive(Default)]
struct Values {
    entries: VecDeque<Cached>,
    revisions: BTreeMap<SessionId, u64>,
    loading: BTreeMap<Reference, Reading>,
    next: u64,
    tracked: BTreeMap<SessionId, usize>,
}
struct Reading {
    id: u64,
    invalidated: bool,
    future: WeakShared<BoxFuture<'static, Result<Read, Fault>>>,
}
struct ReadGuard {
    cache: Weak<Cache>,
    reference: Reference,
    id: u64,
    complete: bool,
}
impl ReadGuard {
    fn finish(&mut self, detail: Option<Loaded>) -> bool {
        self.complete = true;
        self.cache
            .upgrade()
            .is_some_and(|cache| cache.finish(&self.reference, self.id, detail))
    }
}
impl Drop for ReadGuard {
    fn drop(&mut self) {
        if !self.complete
            && let Some(cache) = self.cache.upgrade()
        {
            cache.finish(&self.reference, self.id, None);
        }
    }
}
struct Cached {
    reference: Reference,
    value: Loaded,
    pins: usize,
    valid: bool,
}
impl Cache {
    fn track(&self, session: SessionId) {
        *self
            .values
            .lock()
            .expect("tool detail cache lock")
            .tracked
            .entry(session)
            .or_default() += 1;
    }
    fn untrack(&self, session: SessionId) {
        let mut values = self.values.lock().expect("tool detail cache lock");
        if let Some(count) = values.tracked.get_mut(&session) {
            *count -= 1;
            if *count == 0 {
                values.tracked.remove(&session);
            }
        }
        values.prune(None);
    }
    fn revision(&self, session: SessionId) -> Option<u64> {
        self.values
            .lock()
            .expect("tool detail cache lock")
            .revisions
            .get(&session)
            .copied()
    }
    fn valid(&self, reference: &Reference, detail: &Loaded) -> bool {
        self.values
            .lock()
            .expect("tool detail cache lock")
            .entries
            .iter()
            .any(|entry| {
                entry.valid
                    && entry.reference == *reference
                    && Arc::ptr_eq(&entry.value.detail, &detail.detail)
            })
    }
    fn available(&self, reference: &Reference, bytes: usize) -> bool {
        let values = self.values.lock().expect("tool detail cache lock");
        values
            .entries
            .iter()
            .any(|entry| entry.valid && entry.reference == *reference && entry.pins != 0)
            || values
                .entries
                .iter()
                .filter(|entry| entry.pins != 0)
                .map(|entry| entry.value.bytes)
                .sum::<usize>()
                + bytes
                <= MAX_BYTES
    }
    fn pin<'a>(
        &self,
        reference: &Reference,
        detail: &Loaded,
        replace: impl IntoIterator<Item = (&'a Reference, &'a Loaded)>,
    ) -> Option<Vec<Reference>> {
        let mut values = self.values.lock().expect("tool detail cache lock");
        if let Some(entry) = values.entries.iter_mut().find(|entry| {
            entry.valid
                && entry.reference == *reference
                && Arc::ptr_eq(&entry.value.detail, &detail.detail)
        }) {
            entry.pins += 1;
            return Some(Vec::new());
        }
        let replace = replace
            .into_iter()
            .filter_map(|(reference, detail)| {
                values.entries.iter().position(|entry| {
                    entry.pins == 1
                        && entry.reference == *reference
                        && Arc::ptr_eq(&entry.value.detail, &detail.detail)
                })
            })
            .collect::<Vec<_>>();
        let pinned = values
            .entries
            .iter()
            .filter(|entry| entry.pins != 0)
            .map(|entry| entry.value.bytes)
            .sum::<usize>();
        let reclaimable = replace
            .iter()
            .map(|index| values.entries[*index].value.bytes)
            .sum::<usize>();
        if pinned - reclaimable + detail.bytes > MAX_BYTES {
            return None;
        }
        // Admission and releasing sole-owned pins share this lock. A peer pin
        // cannot change the budget after we decide to replace visible values.
        let mut required = pinned + detail.bytes;
        let mut evicted = Vec::new();
        for index in replace {
            if required <= MAX_BYTES {
                break;
            }
            let entry = &mut values.entries[index];
            required -= entry.value.bytes;
            entry.pins = 0;
            evicted.push(entry.reference.clone());
        }
        values
            .entries
            .retain(|entry| entry.valid || entry.pins != 0);
        assert!(
            values.insert(reference.clone(), detail.clone(), 1),
            "tool detail admission must fit its reserved budget"
        );
        if required < pinned {
            self.capacity.send_modify(|_| {});
        }
        Some(evicted)
    }
    fn unpin(&self, reference: &Reference, detail: &Loaded) {
        let mut values = self.values.lock().expect("tool detail cache lock");
        let mut released = false;
        if let Some(entry) = values.entries.iter_mut().find(|entry| {
            entry.reference == *reference && Arc::ptr_eq(&entry.value.detail, &detail.detail)
        }) {
            entry.pins -= 1;
            released = entry.pins == 0;
        }
        values
            .entries
            .retain(|entry| entry.valid || entry.pins != 0);
        values.prune(None);
        if released {
            self.capacity.send_modify(|_| {});
        }
    }
    pub(crate) fn reconcile(&self, page: &Page) {
        let mut values = self.values.lock().expect("tool detail cache lock");
        let changed = values
            .revisions
            .get(&page.session)
            .is_some_and(|previous| *previous != page.revision);
        for (reference, reading) in &mut values.loading {
            if reference.session == page.session && (changed || !matches_page(reference, page)) {
                reading.invalidated = true;
            }
        }
        for entry in &mut values.entries {
            if entry.reference.session == page.session
                && (changed || !matches_page(&entry.reference, page))
            {
                entry.valid = false;
            }
        }
        // Invalidated allocations stay charged until their presentation releases them.
        values
            .entries
            .retain(|entry| entry.valid || entry.pins != 0);
        values.revisions.insert(page.session, page.revision);
        values.prune(Some(page.session));
    }
    fn finish(&self, reference: &Reference, id: u64, detail: Option<Loaded>) -> bool {
        let mut values = self.values.lock().expect("tool detail cache lock");
        if !values
            .loading
            .get(reference)
            .is_some_and(|reading| reading.id == id)
        {
            return false;
        }
        let valid = !values.loading.remove(reference).unwrap().invalidated;
        if valid && let Some(detail) = detail {
            values.insert(reference.clone(), detail, 0);
        }
        values.prune(None);
        valid
    }
}
impl Values {
    fn get(&mut self, reference: &Reference) -> Option<Loaded> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.valid && entry.reference == *reference)?;
        let entry = self.entries.remove(index)?;
        let detail = entry.value.clone();
        self.entries.push_back(entry);
        Some(detail)
    }
    fn insert(&mut self, reference: Reference, value: Loaded, pins: usize) -> bool {
        if value.bytes > MAX_BYTES {
            return false;
        }
        self.entries
            .retain(|entry| entry.pins != 0 || !same_part(&entry.reference, &reference));
        while self.entries.len() >= MAX_ENTRIES
            || self
                .entries
                .iter()
                .map(|entry| entry.value.bytes)
                .sum::<usize>()
                + value.bytes
                > MAX_BYTES
        {
            let Some(index) = self.entries.iter().position(|entry| entry.pins == 0) else {
                // Pinned item count is a soft target; retained bytes remain a hard limit.
                if self
                    .entries
                    .iter()
                    .map(|entry| entry.value.bytes)
                    .sum::<usize>()
                    + value.bytes
                    <= MAX_BYTES
                {
                    break;
                } else {
                    return false;
                }
            };
            self.entries.remove(index);
        }
        self.entries.push_back(Cached {
            reference,
            value,
            pins,
            valid: true,
        });
        true
    }
    fn prune(&mut self, observed: Option<SessionId>) {
        self.revisions.retain(|session, _| {
            observed == Some(*session)
                || self.tracked.contains_key(session)
                || self
                    .entries
                    .iter()
                    .any(|entry| entry.reference.session == *session)
                || self
                    .loading
                    .keys()
                    .any(|reference| reference.session == *session)
        });
    }
}
/// Conservative retained allocation size, including parsed containers, not JSON bytes.
fn retained_bytes(value: &serde_json::Value) -> usize {
    use serde_json::Value;
    let heap = match value {
        Value::String(text) => text.capacity(),
        Value::Array(values) => {
            values.capacity() * std::mem::size_of::<Value>()
                + values
                    .iter()
                    .map(|value| retained_bytes(value) - std::mem::size_of::<Value>())
                    .sum::<usize>()
        }
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                // BTreeMap nodes include spare entries and links; charge those per entry.
                key.capacity()
                    + 2 * std::mem::size_of::<(String, Value)>()
                    + 3 * std::mem::size_of::<usize>()
                    + retained_bytes(value)
            })
            .sum(),
        _ => 0,
    };
    std::mem::size_of::<Value>() + heap
}

fn allocation(detail: &Detail, reference: &Reference) -> usize {
    let metadata = serde_json::json!([detail.resolved, detail.content, detail.images]);
    // Include both cache storage and the pin map's reference/node allocation.
    std::mem::size_of::<Cached>()
        + 2 * std::mem::size_of::<Reference>()
        + 4 * std::mem::size_of::<usize>()
        + 2 * reference.entry.capacity()
        + 2 * reference.revision.capacity()
        + 2 * reference.call.as_ref().map_or(0, String::capacity)
        + retained_bytes(&detail.arguments)
        + detail.result.as_ref().map_or(0, retained_bytes)
        + 2 * retained_bytes(&metadata)
}

fn matches_page(reference: &Reference, page: &Page) -> bool {
    let Some(entry) = page
        .entries
        .iter()
        .find(|entry| entry.id == reference.entry && entry.turn == reference.turn)
    else {
        return true;
    };
    matches!(entry.parts.get(reference.index),
        Some(Part::ToolCall { id, summary, .. } | Part::ToolResult { id, summary, .. })
        if *id == reference.call && summary.revision == reference.revision)
}

fn same_part(left: &Reference, right: &Reference) -> bool {
    left.session == right.session
        && left.turn == right.turn
        && left.entry == right.entry
        && left.index == right.index
        && left.call == right.call
}

impl Client {
    pub async fn detail(&self, reference: Reference) -> Result<Read, Fault> {
        let future = {
            let mut values = self.details.values.lock().expect("tool detail cache lock");
            if let Some(detail) = values.get(&reference) {
                return Ok(Read::Ready(detail));
            }
            if let Some(future) = values
                .loading
                .get(&reference)
                .filter(|reading| !reading.invalidated)
                .and_then(|reading| reading.future.upgrade())
            {
                future
            } else {
                values.next += 1;
                let id = values.next;
                let client = Client::new(self.transport.clone());
                let read = ReadGuard {
                    cache: Arc::downgrade(&self.details),
                    reference: reference.clone(),
                    id,
                    complete: false,
                };
                let reads = self.details.reads.clone();
                let future = async move {
                    let mut read = read;
                    let _permit = reads
                        .acquire_owned()
                        .await
                        .map_err(|_| Fault::new(ErrorCode::Cancelled, "tool details are closed"))?;
                    let result = match client.read_tool_detail(&read.reference).await {
                        Err(error) if error.code == ErrorCode::RevisionConflict => {
                            Ok(Read::Superseded)
                        }
                        result => result,
                    };
                    let detail = match &result {
                        Ok(Read::Ready(detail)) => Some(detail.clone()),
                        _ => None,
                    };
                    if !read.finish(detail) {
                        return Ok(Read::Superseded);
                    }
                    result
                }
                .boxed()
                .shared();
                values.loading.insert(
                    reference,
                    Reading {
                        id,
                        invalidated: false,
                        future: future.downgrade().expect("unpolled detail future"),
                    },
                );
                future
            }
        };
        future.await
    }
    async fn read_tool_detail(&self, reference: &Reference) -> Result<Read, Fault> {
        let mut text = String::new();
        let mut total = None;
        loop {
            let request = self.prepare(Command::ReadToolDetail {
                session: reference.session,
                turn: reference.turn,
                entry: reference.entry.clone(),
                index: reference.index,
                call: reference.call.clone(),
                expected_revision: reference.revision.clone(),
                cursor: (!text.is_empty()).then_some(text.len()),
                limit: DETAIL_PAGE_BYTES,
            });
            let Output::ToolDetail(page) = self.execute(request).await? else {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "tool detail response expected",
                ));
            };
            if page.total_bytes > MAX_BYTES {
                return Ok(Read::TooLarge(Limit::Transfer));
            }
            if page.reference != *reference
                || page.offset != text.len()
                || page.text.len() > DETAIL_PAGE_BYTES
                || total.is_some_and(|total| total != page.total_bytes)
                || page.offset.saturating_add(page.text.len()) > page.total_bytes
                || page.next_offset
                    != (page.offset + page.text.len() < page.total_bytes)
                        .then_some(page.offset + page.text.len())
                || page.next_offset.is_some() && page.text.is_empty()
            {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "tool detail page is inconsistent",
                ));
            }
            total = Some(page.total_bytes);
            text.push_str(&page.text);
            if page.next_offset.is_none() {
                break;
            }
        }
        let detail: Detail = serde_json::from_str(&text)
            .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "tool detail JSON is invalid"))?;
        if detail.content.as_ref().is_some_and(|content| {
            content.blocks.iter().any(|block| {
                matches!(block, sailry_protocol::tool::Block::Card(card)
                if card.receipt.session != reference.session || card.receipt.turn != reference.turn)
            })
        }) {
            return Err(Fault::invalid(
                "tool detail card belongs to another conversation",
            ));
        }
        let bytes = allocation(&detail, reference);
        if bytes > MAX_BYTES {
            return Ok(Read::TooLarge(Limit::Memory));
        }
        Ok(Read::Ready(Loaded {
            detail: Arc::new(detail),
            bytes,
        }))
    }
}

#[derive(Clone, Debug)]
pub enum Read {
    Ready(Loaded),
    TooLarge(Limit),
    Superseded,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Limit {
    Transfer,
    Memory,
}
impl Limit {
    pub fn message(self) -> &'static str {
        match self {
            Self::Transfer => "tool details exceed the transfer size limit",
            Self::Memory => "tool details exceed the display memory budget",
        }
    }
}
mod tracker;
pub use tracker::{State, Tracker};
#[cfg(test)]
mod tests;
