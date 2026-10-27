//! The Client tracker owns selected tool values and their shared budget.
use super::*;
use sailry_protocol::tool::detail::{Detail as ToolDetail, Reference};

pub(super) enum Action {
    Copy,
    Open(WorktreeId),
}

impl View {
    pub(in crate::conversation::live) fn reconcile_details(&self) {
        let current: std::collections::BTreeSet<_> = std::iter::once(&self.history)
            .chain(self.approvals.histories())
            .flat_map(|history| {
                history
                    .calls
                    .iter()
                    .filter_map(|call| call.detail_reference(&history.snapshot.as_ref()?.page))
            })
            .collect();
        self.details.set(
            self.details
                .references()
                .into_iter()
                .filter(|reference| current.contains(reference)),
        );
    }

    pub(super) fn load_detail(&self, reference: Reference, action: Action, cx: &mut Context<Self>) {
        self.load_once(reference, action, true, cx);
    }

    fn load_once(&self, reference: Reference, action: Action, retry: bool, cx: &mut Context<Self>) {
        use sailry_client::conversation::details::State;
        self.request_details([reference.clone()], true, cx);
        let details = self.details.clone();
        let stop = self.stop.clone();
        let query = reference.clone();
        let job = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                detail = details.wait(&query) => Some(detail),
            }
        });
        cx.spawn(async move |view, cx| {
            let state = match job.await {
                Ok(Some(state)) => Ok(state),
                Ok(None) => return,
                Err(_) => Err(tr("chat_history_failed").to_string()),
            };
            let _ = view.update(cx, |view, cx| {
                let message = match state {
                    Ok(State::Superseded) if retry => {
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
                            view.load_once(current, action, false, cx);
                            return;
                        }
                        tr("chat_history_changed").to_string()
                    }
                    Ok(State::Superseded) => tr("chat_history_changed").to_string(),
                    // The shared tracker notification owns these failure toasts.
                    Ok(State::TooLarge(_) | State::Failed { .. }) => return,
                    Ok(State::Ready(detail)) => {
                        match action {
                            Action::Copy => {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_text(&detail)))
                            }
                            Action::Open(worktree) => {
                                if let Some(path) = full_path(&detail) {
                                    cx.emit(Event::FileAt(worktree, path.to_owned()));
                                }
                            }
                        }
                        return;
                    }
                    Ok(State::Loading) => unreachable!("wait returns a settled detail"),
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

    pub(in crate::conversation::live) fn request_details(
        &self,
        references: impl IntoIterator<Item = Reference>,
        explicit: bool,
        _cx: &mut Context<Self>,
    ) {
        for reference in references {
            self.details.include(reference.clone());
            if explicit {
                self.details.retry(&reference);
            }
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
            self.request_details(references, true, cx);
        } else {
            for reference in references {
                self.details.remove(&reference);
            }
        }
        self.expand(turn, key, open, cx);
    }
}

fn full_path(detail: &sailry_protocol::tool::detail::Detail) -> Option<&str> {
    detail
        .resolved
        .as_ref()
        .and_then(|resolved| {
            resolved
                .output
                .as_ref()
                .filter(|output| output.paths.len() == 1)
                .map(|output| output.paths[0].as_str())
                .or_else(|| {
                    resolved
                        .input
                        .target
                        .as_ref()
                        .map(|target| target.text.as_str())
                })
        })
        .or_else(|| {
            detail
                .arguments
                .get("path")
                .and_then(serde_json::Value::as_str)
        })
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
