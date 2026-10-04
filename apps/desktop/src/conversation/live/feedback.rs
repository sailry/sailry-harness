//! Conversation loading and connection failures stay separate from durable message history.
use super::*;
use gpui_kit::component::button::Button;

impl View {
    pub(super) fn observe_errors(window: &Window, cx: &mut Context<Self>) {
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, _| {
                view.connection_error()
                    .map(|error| {
                        if retrying(error) {
                            "chat_reconnecting"
                        } else {
                            "chat_sync_failed"
                        }
                    })
                    .into_iter()
                    .collect()
            },
            |view, key, cx| {
                let error = view
                    .connection_error()
                    .expect("connection error is present");
                let notice =
                    crate::feedback::diagnostic(error.message.clone().into()).title(tr(key));
                if retrying(error) {
                    return notice;
                }
                let owner = cx.weak_entity();
                notice.autohide(false).action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("chat-sync-retry")
                        .label(tr("chat_sync_retry"))
                        .debug_selector(|| "chat-sync-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |view, cx| view.reconnect(cx));
                        }))
                })
            },
        );
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, _| view.error.into_iter().collect(),
            |view, key, cx| {
                let notice = gpui_kit::component::notification::Notification::error(tr(key));
                if view.retry.is_none() {
                    return notice;
                }
                let owner = cx.weak_entity();
                notice.action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("chat-retry")
                        .label(tr("chat_retry"))
                        .debug_selector(|| "chat-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |view, cx| view.retry(window, cx));
                        }))
                })
            },
        );
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, _| view.approvals.error.into_iter().collect(),
            |view, key, cx| {
                let notice = gpui_kit::component::notification::Notification::error(tr(key));
                if view.approvals.attempt.is_none() {
                    return notice;
                }
                let owner = cx.weak_entity();
                notice.action(move |_, _, cx| {
                    let owner = owner.clone();
                    Button::new("approval-retry")
                        .label(tr("chat_retry"))
                        .debug_selector(|| "live-approval-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            let owner = owner.clone();
                            window.defer(cx, move |window, cx| {
                                _ = owner.update(cx, |view, cx| {
                                    view.retry_approval(window, cx);
                                });
                            });
                        }))
                })
            },
        );
        crate::feedback::observe_with(
            window,
            cx,
            |view: &Self, _| {
                view.history
                    .older_error
                    .as_ref()
                    .map(|_| "chat_history_failed")
                    .into_iter()
                    .collect()
            },
            |_, key, cx| {
                let owner = cx.weak_entity();
                gpui_kit::component::notification::Notification::error(tr(key)).action(
                    move |_, _, cx| {
                        let owner = owner.clone();
                        Button::new("live-load-older")
                            .label(tr("chat_retry"))
                            .debug_selector(|| "live-load-older".into())
                            .on_click(cx.listener(move |toast, _, window, cx| {
                                toast.dismiss(window, cx);
                                _ = owner.update(cx, |view, cx| view.load_older(cx));
                            }))
                    },
                )
            },
        );
        crate::feedback::observe(window, cx, |view: &Self, _| {
            [
                view.attachments.error,
                view.questions.error,
                view.references.failure(),
            ]
            .into_iter()
            .flatten()
            .collect()
        });
    }

    fn connection_error(&self) -> Option<&sailry_protocol::Fault> {
        (!self.connected())
            .then(|| {
                let errors = [
                    &self.history.error,
                    &self.node.error,
                    &self.configuration().error,
                ]
                .into_iter()
                .flatten();
                // A retrying history stream must not hide a stopped Node observer.
                errors
                    .clone()
                    .find(|error| {
                        !matches!(
                            error.code,
                            sailry_protocol::ErrorCode::Unavailable
                                | sailry_protocol::ErrorCode::Busy
                        )
                    })
                    .or_else(|| errors.into_iter().next())
            })
            .flatten()
    }

    /// Missing history is a loading state, never a new conversation.
    pub(super) fn unavailable(&self, cx: &mut Context<Self>) -> Option<Div> {
        let loading = self.session.is_some() && self.history.snapshot.is_none();
        if !loading || self.outgoing.is_some() {
            return None;
        }
        Some(
            crate::empty_state::panel(IconName::LoaderCircle, "chat_history_loading", cx)
                .debug_selector(|| "chat-unavailable".into())
                .key_context("LiveConversation"),
        )
    }

    fn reconnect(&mut self, cx: &mut Context<Self>) {
        if self.node.error.is_some() {
            self._node = watch::node(&self.binding, false, &self.stop, cx);
            self.node.error = None;
        }
        if self.defaults.error.is_some() {
            self._defaults = Some(watch::node(&self.binding, true, &self.defaults_stop, cx));
            self.defaults.error = None;
        }
        if let Some(session) = self.session() {
            let (subscription, older) = watch::history(&self.binding, session, &self.stop, cx);
            self.subscription = Some(subscription);
            self.older = Some(older);
            self.history.error = None;
        }
        cx.notify();
    }
}

fn retrying(error: &sailry_protocol::Fault) -> bool {
    matches!(
        error.code,
        sailry_protocol::ErrorCode::Unavailable | sailry_protocol::ErrorCode::Busy
    )
}
