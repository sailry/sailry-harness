//! The Client tracker owns selected tool values and their shared budget.
use super::*;
use sailry_protocol::tool::detail::{Detail as ToolDetail, Reference};
use std::collections::BTreeSet;

impl View {
    /// Expansion state decides what the tracker keeps; layout and focus never do.
    pub(in crate::conversation::live) fn select_details(&self) {
        if !self.details_visible.get() {
            return;
        }
        let mut protected = BTreeSet::new();
        for history in std::iter::once(&self.history).chain(self.approvals.histories()) {
            let Some(snapshot) = &history.snapshot else {
                continue;
            };
            protected.extend(
                history
                    .calls
                    .iter()
                    .filter(|call| call.approval_pending(&snapshot.page))
                    .filter_map(|call| call.detail_reference(&snapshot.page)),
            );
        }
        let mut selected = protected.clone();
        if let Some(snapshot) = &self.history.snapshot {
            let calls = self.history.calls.as_slice();
            let shown = publications(calls, &snapshot.page);
            self.publication_heights
                .borrow_mut()
                .retain(|reference, _| shown.contains(reference));
            selected.extend(expanded(&self.expanded, calls, &snapshot.page));
            selected.extend(shown);
        } else {
            self.publication_heights.borrow_mut().clear();
        }
        self.details.protect(protected);
        self.details.set(selected);
        self.prune_tool_texts();
    }

    pub(super) fn copy_detail(&self, reference: Reference, cx: &mut Context<Self>) {
        self.load_once(reference, true, cx);
    }

    fn load_once(&self, reference: Reference, retry: bool, cx: &mut Context<Self>) {
        use sailry_client::conversation::details::Read;
        let client = self.binding.client.clone();
        let stop = self.stop.clone();
        let query = reference.clone();
        let job = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                detail = client.detail(query) => Some(detail),
            }
        });
        cx.spawn(async move |view, cx| {
            let state = match job.await {
                Ok(Some(result)) => result.map_err(|fault| fault.message),
                Ok(None) => return,
                Err(_) => Err(tr("chat_history_failed").to_string()),
            };
            let _ = view.update(cx, |view, cx| {
                let message = match state {
                    Ok(Read::Superseded) if retry => {
                        let current = std::iter::once(&view.history)
                            .chain(view.approvals.histories())
                            .find_map(|history| {
                                let page = &history.snapshot.as_ref()?.page;
                                history
                                    .calls
                                    .iter()
                                    .find(|call| {
                                        call.turn == reference.turn
                                            && (call.id.is_some() && call.id == reference.call
                                                || [Some(&call.source), call.response.as_ref()]
                                                    .into_iter()
                                                    .flatten()
                                                    .any(|part| {
                                                        part.entry == reference.entry
                                                            && part.index == reference.index
                                                    }))
                                    })?
                                    .detail_reference(page)
                            });
                        if let Some(current) = current {
                            view.load_once(current, false, cx);
                            return;
                        }
                        tr("chat_history_changed").to_string()
                    }
                    Ok(Read::Superseded) => tr("chat_history_changed").to_string(),
                    Ok(Read::TooLarge(limit)) => limit.message().to_string(),
                    Ok(Read::Ready(detail)) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_text(&detail)));
                        return;
                    }
                    Err(message) => message,
                };
                let _ = cx.update_window(view.history_window, |_, window, cx| {
                    crate::feedback::error(
                        "tool_failed",
                        "",
                        &diagnostic::display(&message),
                        window,
                        cx,
                    )
                });
            });
        })
        .detach();
    }

    pub(in crate::conversation::live) fn detailed(&self, call: &Call, page: &Page) -> Call {
        call.detail_reference(page)
            .and_then(|reference| self.detail(&reference))
            .map_or_else(|| call.clone(), |detail| call.with_detail(detail))
    }

    pub(in crate::conversation::live) fn detail(
        &self,
        reference: &Reference,
    ) -> Option<Arc<ToolDetail>> {
        match self.details.state(reference) {
            sailry_client::conversation::details::State::Ready(detail) => Some(detail.detail),
            _ => None,
        }
    }

    pub(super) fn expand_tools(
        &mut self,
        turn: TurnId,
        key: String,
        open: bool,
        references: Vec<Reference>,
        cx: &mut Context<Self>,
    ) {
        self.expand(turn, key, open, cx);
        self.select_details();
        if open {
            // An explicit open refreshes a failed or superseded detail.
            for reference in references {
                self.details.retry(&reference);
            }
        }
    }
}

/// References whose rows are expanded, from expansion state and the page alone.
pub(super) fn expanded(
    open: &BTreeMap<(TurnId, String), bool>,
    calls: &[Call],
    page: &Page,
) -> BTreeSet<Reference> {
    let mut references = BTreeSet::new();
    for (index, call) in calls.iter().enumerate() {
        if call.progress.is_some() || !call.details(page) {
            continue;
        }
        let is_open = |key: String| open.get(&(call.turn, key)).copied().unwrap_or(false);
        if is_open(row_key(call)) {
            references.extend(call.detail_reference(page));
        }
        // A projected group shows the whole same-name run that starts at its first call.
        if is_open(group_key(call)) && projection::grouped(call, page) {
            references.extend(
                calls[index..]
                    .iter()
                    .filter(|member| member.turn == call.turn)
                    .take_while(|member| member.name == call.name && member.progress.is_none())
                    .filter(|member| member.details(page))
                    .filter_map(|member| member.detail_reference(page)),
            );
        }
    }
    references
}

/// Answer publications are always shown, so they stay loaded while on the page.
pub(super) fn publications(calls: &[Call], page: &Page) -> BTreeSet<Reference> {
    calls
        .iter()
        .filter(|call| call.summary(page).is_some_and(|summary| summary.published))
        .filter_map(|call| call.detail_reference(page))
        .collect()
}

fn copy_text(detail: &sailry_protocol::tool::detail::Detail) -> String {
    if let (Some(content), Some(result)) = (&detail.content, &detail.result) {
        return content::copy(content, result, &rust_i18n::locale());
    }
    if let Some(body) = detail
        .resolved
        .as_ref()
        .and_then(|resolved| resolved.output.as_ref())
        .and_then(|output| output.body.as_ref())
        && !body.text.is_empty()
    {
        return body.text.clone();
    }
    detail
        .result
        .as_ref()
        .map(|value| {
            value
                .as_str()
                .map_or_else(|| output::raw(value), str::to_owned)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn copies_the_complete_result() {
        let result = "original output ".repeat(100_000);
        let detail = sailry_protocol::tool::detail::Detail {
            arguments: serde_json::Value::Null,
            result: Some(serde_json::Value::String(result.clone())),
            resolved: None,
            content: None,
            images: Vec::new(),
        };
        assert_eq!(copy_text(&detail), result);
    }

    #[core::prelude::v1::test]
    fn copies_complete_structured_text() {
        let text = "original stream ".repeat(100_000);
        let detail = sailry_protocol::tool::detail::Detail {
            arguments: serde_json::Value::Null,
            result: Some(serde_json::json!({"output":{"stream":text}})),
            resolved: None,
            content: Some(sailry_protocol::tool::Content {
                version: 1,
                blocks: vec![sailry_protocol::tool::Block::Text(
                    sailry_protocol::tool::Text {
                        path: "/stream".into(),
                        notices: Vec::new(),
                    },
                )],
            }),
            images: Vec::new(),
        };
        assert_eq!(copy_text(&detail), text);
    }
}
