//! Explicit detail reads share one bounded cache across a controller's views.
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

pub(crate) struct Cache {
    values: Mutex<Values>,
    changes: tokio::sync::watch::Sender<u64>,
    reads: Arc<tokio::sync::Semaphore>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            values: Mutex::default(),
            changes: tokio::sync::watch::channel(0).0,
            reads: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
}

#[derive(Default)]
struct Values {
    entries: VecDeque<Cached>,
    revisions: BTreeMap<SessionId, u64>,
    loading: BTreeMap<Reference, Reading>,
    next: u64,
    pins: BTreeMap<Reference, usize>,
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
    fn finish(&mut self, detail: Option<(Arc<Detail>, usize)>) -> bool {
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
    detail: Arc<Detail>,
    bytes: usize,
}

impl Cache {
    fn track(&self, session: SessionId) {
        let mut values = self.values.lock().expect("tool detail cache lock");
        *values.tracked.entry(session).or_default() += 1;
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

    fn get(&self, reference: &Reference) -> Option<Arc<Detail>> {
        let mut values = self.values.lock().expect("tool detail cache lock");
        let index = values
            .entries
            .iter()
            .position(|entry| entry.reference == *reference)?;
        let cached = values.entries.remove(index)?;
        let detail = cached.detail.clone();
        values.entries.push_back(cached);
        Some(detail)
    }

    fn pin(&self, reference: &Reference, detail: &Arc<Detail>) -> bool {
        let bytes = allocation(detail, reference);
        let mut values = self.values.lock().expect("tool detail cache lock");
        let pinned = values
            .entries
            .iter()
            .filter(|entry| {
                values.pins.contains_key(&entry.reference) && entry.reference != *reference
            })
            .map(|entry| entry.bytes)
            .sum::<usize>();
        if pinned + bytes > MAX_BYTES {
            return false;
        }
        values.entries.retain(|entry| entry.reference != *reference);
        while values.entries.len() >= MAX_ENTRIES
            || values
                .entries
                .iter()
                .map(|entry| entry.bytes)
                .sum::<usize>()
                + bytes
                > MAX_BYTES
        {
            let Some(index) = values
                .entries
                .iter()
                .position(|entry| !values.pins.contains_key(&entry.reference))
            else {
                // The item count is an LRU target. Pinned entries may exceed it;
                // their combined retained allocation still fits the hard byte budget.
                break;
            };
            values.entries.remove(index);
        }
        values.entries.push_back(Cached {
            reference: reference.clone(),
            detail: detail.clone(),
            bytes,
        });
        *values.pins.entry(reference.clone()).or_default() += 1;
        true
    }

    fn unpin(&self, reference: &Reference) {
        let mut values = self.values.lock().expect("tool detail cache lock");
        if let Some(count) = values.pins.get_mut(reference) {
            *count -= 1;
            if *count == 0 {
                values.pins.remove(reference);
            }
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
        values.entries.retain(|cached| {
            if cached.reference.session != page.session {
                return true;
            }
            if changed {
                return false;
            }
            matches_page(&cached.reference, page)
        });
        values.revisions.insert(page.session, page.revision);
        values.prune(Some(page.session));
        drop(values);
        self.changes.send_modify(|sequence| *sequence += 1);
    }

    fn finish(&self, reference: &Reference, id: u64, detail: Option<(Arc<Detail>, usize)>) -> bool {
        let mut values = self.values.lock().expect("tool detail cache lock");
        if !values
            .loading
            .get(reference)
            .is_some_and(|reading| reading.id == id)
        {
            return false;
        }
        let valid = !values.loading.remove(reference).unwrap().invalidated;
        if valid && let Some((detail, bytes)) = detail {
            values
                .entries
                .retain(|entry| !same_part(&entry.reference, reference));
            while values.entries.len() >= MAX_ENTRIES
                || values
                    .entries
                    .iter()
                    .map(|entry| entry.bytes)
                    .sum::<usize>()
                    + bytes
                    > MAX_BYTES
            {
                let Some(index) = values
                    .entries
                    .iter()
                    .position(|entry| !values.pins.contains_key(&entry.reference))
                else {
                    values.prune(None);
                    return valid;
                };
                values.entries.remove(index);
            }
            values.entries.push_back(Cached {
                reference: reference.clone(),
                detail,
                bytes,
            });
        }
        values.prune(None);
        valid
    }
}

impl Values {
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
pub fn retained_bytes(value: &serde_json::Value) -> usize {
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
    pub fn cached_tool_detail(&self, reference: &Reference) -> Option<Arc<Detail>> {
        self.details.get(reference)
    }

    /// Explicit readers can request complete values without a presentation tracker.
    pub async fn tool_detail(&self, reference: Reference) -> Result<Arc<Detail>, Fault> {
        match self.detail(reference).await? {
            Read::Ready(detail) => Ok(detail),
            Read::TooLarge(limit) => Err(Fault::new(ErrorCode::InvalidRequest, limit.message())),
            Read::Superseded => Err(Fault::new(
                ErrorCode::Cancelled,
                "tool detail read was superseded",
            )),
        }
    }

    /// Changed references and display limits are outcomes, not transport failures.
    pub async fn detail(&self, reference: Reference) -> Result<Read, Fault> {
        if let Some(detail) = self.details.get(&reference) {
            return Ok(Read::Ready(detail));
        }
        let future = {
            let mut values = self.details.values.lock().expect("tool detail cache lock");
            if let Some(cached) = values
                .entries
                .iter()
                .find(|entry| entry.reference == reference)
            {
                return Ok(Read::Ready(cached.detail.clone()));
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
                        Ok(Read::Ready(detail))
                            if allocation(&detail, &read.reference) > MAX_BYTES =>
                        {
                            Ok(Read::TooLarge(Limit::Memory))
                        }
                        Err(error)
                            if matches!(
                                error.code,
                                ErrorCode::RevisionConflict | ErrorCode::NotFound
                            ) =>
                        {
                            Ok(Read::Superseded)
                        }
                        result => result,
                    };
                    let detail = match &result {
                        Ok(Read::Ready(detail)) => {
                            Some((detail.clone(), allocation(detail, &read.reference)))
                        }
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
                    reference.clone(),
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
        Ok(Read::Ready(Arc::new(detail)))
    }
}

#[derive(Clone, Debug)]
pub enum Read {
    Ready(Arc<Detail>),
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
