use super::*;
use gpui_kit::component::{
    Disableable, Sizable, dialog::Dialog, h_flex, scroll::ScrollableElement, v_flex,
};

impl Review {
    pub(super) fn dialog(
        &self,
        dialog: Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let blocked = self.pending || self.request.is_some() || !self.connected(cx);
        let mut body = v_flex().id("turn-hunks").test_support().gap_3();
        if self.pending {
            body = body.child(gpui_kit::component::spinner::Spinner::new());
        }
        if self.failed || self.request.is_some() {
            body = body.child(
                Button::new("turn-hunks-retry")
                    .label(tr("chat_retry"))
                    .disabled(self.pending)
                    .on_click(cx.listener(|review, _, window, cx| review.retry(window, cx))),
            );
        }
        if let Some(data) = &self.data {
            for hunk in &data.hunks {
                let state = self.decisions.get(&hunk.id).copied();
                let accepted = hunk.id.clone();
                let rejected = hunk.id.clone();
                let staged = hunk.id.clone();
                let actions = h_flex()
                    .gap_2()
                    .child(
                        Button::new("accept")
                            .small()
                            .outline()
                            .label(tr("hunks_accept"))
                            .disabled(blocked || state.is_some())
                            .on_click(cx.listener(move |review, _, window, cx| {
                                review.accept(&accepted, window, cx)
                            })),
                    )
                    .child(
                        Button::new("reject")
                            .small()
                            .outline()
                            .label(tr("hunks_reject"))
                            .disabled(blocked || state.is_some())
                            .on_click(cx.listener(move |review, _, window, cx| {
                                review.apply(&rejected, hunk::Operation::Reject, window, cx)
                            })),
                    )
                    .child(
                        Button::new("stage")
                            .small()
                            .outline()
                            .label(tr("hunks_stage"))
                            .disabled(
                                blocked
                                    || self.index.is_none()
                                    || matches!(state, Some(Decision::Rejected | Decision::Staged)),
                            )
                            .on_click(cx.listener(move |review, _, window, cx| {
                                review.apply(&staged, hunk::Operation::Stage, window, cx)
                            })),
                    );
                // Kit exposes file headers, not a hunk-action callback. Keep the
                // revision-checked controls with this immutable hunk identity.
                let id = format!("turn-hunk-{}-{}", self.turn, hunk.id);
                body = body.child(
                    v_flex()
                        .id(id.clone())
                        .test_support()
                        .gap_2()
                        .child(actions)
                        .child(crate::content::diff::rows(
                            &id, &hunk.text, &self.path, false,
                        )),
                );
            }
            if data.partial {
                body = body.child(crate::conversation::surface::notice(tr("tool_partial"), cx));
            }
        }
        dialog
            .title(self.path.clone())
            .w((window.viewport_size().width - px(48.)).min(px(900.)))
            .max_h(window.viewport_size().height * 0.8)
            .child(body.overflow_y_scrollbar())
    }
}
