//! Explicit detail reads share one bounded cache across a controller's views.
use crate::Client;
use futures::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, SessionId,
    conversation::{Page, Part},
    tool::detail::{DETAIL_PAGE_BYTES, Detail, Reference},
};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex},
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;

#[derive(Default)]
pub(crate) struct Cache {
    values: Mutex<Values>,
}

#[derive(Default)]
struct Values {
    entries: VecDeque<Cached>,
    revisions: BTreeMap<SessionId, u64>,
    loading: BTreeMap<Reference, Reading>,
    next: u64,
}

struct Reading {
    id: u64,
    invalidated: bool,
    future: Shared<BoxFuture<'static, Result<Arc<Detail>, Fault>>>,
}

struct Cached {
    reference: Reference,
    detail: Arc<Detail>,
    bytes: usize,
}

impl Cache {
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
    }

    fn finish(&self, reference: &Reference, id: u64, detail: Option<(Arc<Detail>, usize)>) -> bool {
        let mut values = self.values.lock().expect("tool detail cache lock");
        let valid = values
            .loading
            .remove(reference)
            .is_some_and(|reading| reading.id == id && !reading.invalidated);
        if valid && let Some((detail, bytes)) = detail.filter(|(_, bytes)| *bytes <= MAX_BYTES) {
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
                if values.entries.pop_front().is_none() {
                    break;
                }
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
    std::mem::size_of::<Cached>()
        + reference.entry.capacity()
        + reference.revision.capacity()
        + reference.call.as_ref().map_or(0, String::capacity)
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

    /// Publish only complete, identity-checked JSON; a failed page never enters the cache.
    pub async fn tool_detail(&self, reference: Reference) -> Result<Arc<Detail>, Fault> {
        if let Some(detail) = self.details.get(&reference) {
            return Ok(detail);
        }
        let future = {
            let mut values = self.details.values.lock().expect("tool detail cache lock");
            if let Some(cached) = values
                .entries
                .iter()
                .find(|entry| entry.reference == reference)
            {
                return Ok(cached.detail.clone());
            }
            if let Some(reading) = values.loading.get(&reference) {
                reading.future.clone()
            } else {
                values.next += 1;
                let id = values.next;
                let client = Client::new(self.transport.clone());
                let cache = Arc::downgrade(&self.details);
                let reading = reference.clone();
                let task = tokio::spawn(async move {
                    let result = client.read_tool_detail(&reading).await.map(Arc::new);
                    if let Some(cache) = cache.upgrade() {
                        let detail = result
                            .as_ref()
                            .ok()
                            .map(|detail| (detail.clone(), allocation(detail, &reading)));
                        if !cache.finish(&reading, id, detail) {
                            return Err(Fault::new(
                                ErrorCode::RevisionConflict,
                                "conversation changed while loading tool details",
                            ));
                        }
                    }
                    result
                });
                let future = async move {
                    task.await
                        .map_err(|_| Fault::new(ErrorCode::Internal, "tool detail read stopped"))?
                }
                .boxed()
                .shared();
                values.loading.insert(
                    reference.clone(),
                    Reading {
                        id,
                        invalidated: false,
                        future: future.clone(),
                    },
                );
                future
            }
        };
        future.await
    }

    async fn read_tool_detail(&self, reference: &Reference) -> Result<Detail, Fault> {
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
            if page.reference != *reference
                || page.offset != text.len()
                || page.total_bytes > MAX_BYTES
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
        Ok(detail)
    }
}

#[cfg(test)]
mod tests;
