//! The Client tracker owns selected tool values and their shared budget.
use super::*;
use sailry_protocol::tool::detail::{Detail as ToolDetail, Reference};

impl View {
    pub(in crate::conversation::live) fn select_details(&self, bounds: Bounds<Pixels>) {
        if !self.details_visible.get() {
            return;
        }
        let mut current = std::collections::BTreeSet::new();
        let mut protected = std::collections::BTreeSet::new();
        for history in std::iter::once(&self.history).chain(self.approvals.histories()) {
            let Some(snapshot) = &history.snapshot else {
                continue;
            };
            for call in history.calls.iter() {
                if let Some(reference) = call.detail_reference(&snapshot.page) {
                    current.insert(reference.clone());
                    if call.approval_pending(&snapshot.page) {
                        protected.insert(reference);
                    }
                }
            }
        }
        self.publication_heights
            .borrow_mut()
            .retain(|reference, _| current.contains(reference));
        let (selected, protected) =
            selection(&self.detail_rows.borrow(), bounds, &current, protected);
        self.details.protect(protected);
        self.details.set(selected);
        self.prune_tool_texts();
    }

    // Child-bounds listeners keep scroll-layer reuse from skipping selection updates.
    pub(in crate::conversation::live) fn detail_bounds(
        &self,
        references: impl IntoIterator<Item = Reference>,
        published: bool,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let references = references.into_iter().collect::<Vec<_>>();
        let rows = self.detail_rows.clone();
        move |bounds, _, _| {
            let Some(bounds) = bounds.into_iter().reduce(|row, child| row.union(&child)) else {
                return;
            };
            rows.borrow_mut().extend(
                references
                    .iter()
                    .cloned()
                    .map(|reference| (reference, bounds, published)),
            );
        }
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
                    crate::feedback::error("", &diagnostic::display(&message), window, cx)
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
        if open {
            for reference in references {
                self.details.include(reference.clone());
                self.details.retry(&reference);
            }
        } else {
            for reference in references {
                self.details.remove(&reference);
            }
        }
        self.expand(turn, key, open, cx);
    }
}

pub(super) fn visible(row: Bounds<Pixels>, viewport: Bounds<Pixels>) -> bool {
    viewport.size.height > px(0.)
        && row.size.height > px(0.)
        && row.bottom() > viewport.top()
        && row.top() < viewport.bottom()
}

pub(super) fn selection(
    rows: &[(Reference, Bounds<Pixels>, bool)],
    bounds: Bounds<Pixels>,
    current: &std::collections::BTreeSet<Reference>,
    mut protected: std::collections::BTreeSet<Reference>,
) -> (
    std::collections::BTreeSet<Reference>,
    std::collections::BTreeSet<Reference>,
) {
    let mut selected = std::collections::BTreeSet::new();
    for (reference, row, published) in rows {
        if visible(*row, bounds) && current.contains(reference) {
            selected.insert(reference.clone());
            if *published {
                protected.insert(reference.clone());
            }
        }
    }
    selected.extend(protected.iter().cloned());
    (selected, protected)
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
