//! UI owns expansion and loading; complete values stay in the shared Client cache.
use super::*;
use sailry_protocol::tool::detail::Reference;

impl View {
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
