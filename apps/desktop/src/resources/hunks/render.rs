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
        let mut body = v_flex().gap_3().debug_selector(|| "turn-hunks".into());
        if self.pending {
            body = body.child(gpui_kit::component::spinner::Spinner::new());
        }
        if self.failed || self.request.is_some() {
            body = body.child(
                Button::new("retry-hunks")
                    .label(tr("chat_retry"))
                    .disabled(self.pending)
                    .debug_selector(|| "turn-hunks-retry".into())
                    .on_click(cx.listener(|review, _, window, cx| review.retry(window, cx))),
            );
        }
        if let Some(data) = &self.data {
            for (index, hunk) in data.hunks.iter().enumerate() {
                let state = self.decisions.get(&hunk.id).copied();
                let accepted = hunk.id.clone();
                let rejected = hunk.id.clone();
                let staged = hunk.id.clone();
                let actions = h_flex()
                    .gap_2()
                    .child(
                        Button::new(format!("accept-{index}"))
                            .small()
                            .outline()
                            .label(tr("hunks_accept"))
                            .disabled(blocked || state.is_some())
                            .debug_selector(move || format!("turn-hunks-accept-{index}"))
                            .on_click(cx.listener(move |review, _, window, cx| {
                                review.accept(&accepted, window, cx)
                            })),
                    )
                    .child(
                        Button::new(format!("reject-{index}"))
                            .small()
                            .outline()
                            .label(tr("hunks_reject"))
                            .disabled(blocked || state.is_some())
                            .debug_selector(move || format!("turn-hunks-reject-{index}"))
                            .on_click(cx.listener(move |review, _, window, cx| {
                                review.apply(&rejected, hunk::Operation::Reject, window, cx)
                            })),
                    )
                    .child(
                        Button::new(format!("stage-{index}"))
                            .small()
                            .outline()
                            .label(tr("hunks_stage"))
                            .disabled(
                                blocked
                                    || self.index.is_none()
                                    || matches!(state, Some(Decision::Rejected | Decision::Staged)),
                            )
                            .debug_selector(move || format!("turn-hunks-stage-{index}"))
                            .on_click(cx.listener(move |review, _, window, cx| {
                                review.apply(&staged, hunk::Operation::Stage, window, cx)
                            })),
                    );
                let id = format!("turn-hunk-{}-{index}", self.turn);
                let lines = crate::content::diff::unified(&hunk.text);
                body = body.child(v_flex().gap_2().child(actions).child(
                    crate::content::diff::rows(
                        &id,
                        &lines,
                        crate::content::diff::language(&self.path),
                        cx,
                    ),
                ));
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
