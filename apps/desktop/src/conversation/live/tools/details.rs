//! Visible details retain the Client's shared values until their references leave the view.
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
        self.detail_refs
            .borrow_mut()
            .retain(|reference| current.contains(reference));
        self.details.set(self.detail_refs.borrow().iter().cloned());
    }

    pub(super) fn load_detail(&self, reference: Reference, action: Action, cx: &mut Context<Self>) {
        if let Some(detail) = self.detail(&reference) {
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
        let client = self.binding.client.clone();
        let stop = self.stop.clone();
        let job = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                detail = client.detail(reference) => Some(detail),
            }
        });
        cx.spawn(async move |view, cx| {
            let result = match job.await {
                Ok(Some(result)) => result,
                Ok(None) => return,
                Err(_) => Err(sailry_protocol::Fault::new(
                    sailry_protocol::ErrorCode::Internal,
                    "tool details could not be loaded",
                )),
            };
            let _ = view.update(cx, |view, cx| match result {
                Ok(sailry_client::conversation::details::Read::Superseded) => {}
                Ok(sailry_client::conversation::details::Read::TooLarge(limit)) => {
                    let _ = cx.update_window(view.history_window, |_, window, cx| {
                        crate::feedback::error(
                            "",
                            &diagnostic::display(limit.message()),
                            window,
                            cx,
                        )
                    });
                }
                Ok(sailry_client::conversation::details::Read::Ready(detail)) => match action {
                    Action::Copy => {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_text(&detail)))
                    }
                    Action::Open(worktree) => {
                        if let Some(path) = full_path(&detail) {
                            cx.emit(Event::FileAt(worktree, path.to_owned()));
                        }
                    }
                },
                Err(error) => {
                    let _ = cx.update_window(view.history_window, |_, window, cx| {
                        crate::feedback::error(
                            "",
                            &diagnostic::display(&error.message),
                            window,
                            cx,
                        );
                    });
                }
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
            sailry_client::conversation::details::State::Ready(detail) => Some(detail),
            _ => None,
        }
    }

    pub(in crate::conversation::live) fn request_details(
        &self,
        references: impl IntoIterator<Item = Reference>,
        explicit: bool,
        _cx: &mut Context<Self>,
    ) {
        let references = references.into_iter().collect::<Vec<_>>();
        self.detail_refs
            .borrow_mut()
            .extend(references.iter().cloned());
        self.details.set(self.detail_refs.borrow().iter().cloned());
        if explicit {
            for reference in references {
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
                self.detail_refs.borrow_mut().remove(&reference);
            }
            self.details.set(self.detail_refs.borrow().iter().cloned());
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
