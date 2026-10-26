//! Explicit detail reads share one bounded cache across a controller's views.
use crate::Client;
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
    reading: tokio::sync::Mutex<()>,
}

#[derive(Default)]
struct Values {
    entries: VecDeque<Cached>,
    revisions: BTreeMap<SessionId, u64>,
    loading: Option<(Reference, bool)>,
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
            .insert(page.session, page.revision)
            .is_some_and(|previous| previous != page.revision);
        if let Some((reference, invalidated)) = &mut values.loading
            && reference.session == page.session
            && (changed || !matches_page(reference, page))
        {
            *invalidated = true;
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
    }

    fn begin(&self, reference: &Reference) -> Option<u64> {
        let mut values = self.values.lock().expect("tool detail cache lock");
        values.loading = Some((reference.clone(), false));
        values.revisions.get(&reference.session).copied()
    }

    fn finish(
        &self,
        reference: &Reference,
        revision: Option<u64>,
        detail: Option<(Arc<Detail>, usize)>,
    ) -> bool {
        let mut values = self.values.lock().expect("tool detail cache lock");
        let valid = values
            .loading
            .take()
            .is_some_and(|(loading, invalidated)| loading == *reference && !invalidated)
            && values.revisions.get(&reference.session).copied() == revision;
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
        valid
    }
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
        let _reading = self.details.reading.lock().await;
        if let Some(detail) = self.details.get(&reference) {
            return Ok(detail);
        }
        let revision = self.details.begin(&reference);
        match self.read_tool_detail(&reference).await {
            Ok((detail, bytes)) => {
                let detail = Arc::new(detail);
                if !self
                    .details
                    .finish(&reference, revision, Some((detail.clone(), bytes)))
                {
                    return Err(Fault::new(
                        ErrorCode::RevisionConflict,
                        "conversation changed while loading tool details",
                    ));
                }
                Ok(detail)
            }
            Err(error) => {
                self.details.finish(&reference, revision, None);
                Err(error)
            }
        }
    }

    async fn read_tool_detail(&self, reference: &Reference) -> Result<(Detail, usize), Fault> {
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
        Ok((detail, text.len()))
    }
}

#[cfg(test)]
mod tests;
