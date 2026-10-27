//! UI owns expansion and loading; complete values stay in the shared Client cache.
use super::*;
use sailry_protocol::tool::detail::Reference;

pub(super) enum Action {
    Copy,
    Open(WorktreeId),
}

impl View {
    pub(super) fn load_detail(&self, reference: Reference, action: Action, cx: &mut Context<Self>) {
        let client = self.binding.client.clone();
        let stop = self.stop.clone();
        let job = self.binding.runtime.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => None,
                detail = client.tool_detail(reference) => Some(detail),
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
                Ok(detail) => match action {
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
            .and_then(|reference| self.binding.client.cached_tool_detail(&reference))
            .map_or_else(|| call.clone(), |detail| call.with_detail(detail))
    }

    pub(in crate::conversation::live) fn request_details(
        &self,
        references: impl IntoIterator<Item = Reference>,
        explicit: bool,
        cx: &mut Context<Self>,
    ) {
        for reference in references {
            if self.binding.client.cached_tool_detail(&reference).is_some()
                || self.detail_loading.borrow().contains(&reference)
                || !explicit && self.detail_failed.borrow().contains(&reference)
            {
                continue;
            }
            self.detail_failed.borrow_mut().remove(&reference);
            self.detail_loading.borrow_mut().insert(reference.clone());
            let client = self.binding.client.clone();
            let stop = self.stop.clone();
            let query = reference.clone();
            let job = self.binding.runtime.spawn(async move {
                tokio::select! {
                    _ = stop.cancelled() => None,
                    result = client.tool_detail(query) => Some(result),
                }
            });
            cx.spawn(async move |view, cx| {
                let result = job.await;
                let _ = view.update(cx, |view, cx| {
                    view.detail_loading.borrow_mut().remove(&reference);
                    let current = std::iter::once(&view.history)
                        .chain(view.approvals.histories())
                        .any(|history| {
                            history.snapshot.as_ref().is_some_and(|snapshot| {
                                history.calls.iter().any(|call| {
                                    call.detail_reference(&snapshot.page).as_ref()
                                        == Some(&reference)
                                })
                            })
                        });
                    if !current {
                        return;
                    }
                    let error = match result {
                        Ok(Some(Ok(_))) => None,
                        Ok(Some(Err(error))) => Some(error),
                        Ok(None) => return,
                        Err(_) => Some(sailry_protocol::Fault::new(
                            sailry_protocol::ErrorCode::Internal,
                            "tool details could not be loaded",
                        )),
                    };
                    if let Some(error) = error {
                        view.detail_failed.borrow_mut().insert(reference.clone());
                        let _ = cx.update_window(view.history_window, |_, window, cx| {
                            crate::feedback::error(
                                "",
                                &diagnostic::display(&error.message),
                                window,
                                cx,
                            );
                        });
                    }
                    if let Some(index) = view.rows.iter().position(|turn| *turn == reference.turn) {
                        view.scroller.update(cx, |scroller, cx| {
                            scroller.remeasure_items(index..index + 1, cx);
                        });
                    }
                    cx.notify();
                });
            })
            .detach();
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
