use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable, IconName, Sizable, button::ButtonVariants as _, dialog::Dialog,
    h_flex, scroll::ScrollableElement, tag::Tag, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;

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
        let title = h_flex()
            .min_w_0()
            .gap_2()
            .child(div().min_w_0().truncate().child(self.path.clone()))
            .when_some(self.data.as_ref(), |title, data| {
                let (added, removed) = data.hunks.iter().fold((0, 0), |(added, removed), hunk| {
                    (added + hunk.additions, removed + hunk.deletions)
                });
                title.child(crate::conversation::surface::diff::badge(
                    added, removed, cx,
                ))
            });
        if let Some(data) = &self.data {
            for hunk in &data.hunks {
                let state = self.decisions.get(&hunk.id).copied();
                let accepted = hunk.id.clone();
                let rejected = hunk.id.clone();
                let staged = hunk.id.clone();
                let reject = Button::new("reject")
                    .small()
                    .ghost()
                    .icon(IconName::Undo2)
                    .label(tr("hunks_reject"))
                    .disabled(blocked || state.is_some())
                    .on_click(cx.listener(move |review, _, window, cx| {
                        review.apply(&rejected, hunk::Operation::Reject, window, cx)
                    }));
                let stage = Button::new("stage")
                    .small()
                    .ghost()
                    .label(tr("hunks_stage"))
                    .disabled(
                        blocked
                            || self.index.is_none()
                            || matches!(state, Some(Decision::Rejected | Decision::Staged)),
                    )
                    .on_click(cx.listener(move |review, _, window, cx| {
                        review.apply(&staged, hunk::Operation::Stage, window, cx)
                    }));
                let accept = Button::new("accept")
                    .small()
                    .primary()
                    .icon(IconName::Check)
                    .label(tr("hunks_accept"))
                    .disabled(blocked || state.is_some())
                    .on_click(cx.listener(move |review, _, window, cx| {
                        review.accept(&accepted, window, cx)
                    }));
                let stageable = self.index.is_some();
                // An accepted hunk keeps its change in the worktree and may still be staged.
                let actions = h_flex().flex_shrink_0().gap_1().map(|actions| match state {
                    None => actions
                        .child(reject)
                        .when(stageable, |actions| actions.child(stage))
                        .child(accept),
                    Some(state) => actions
                        .child(decision(state))
                        .when(state == Decision::Accepted && stageable, |actions| {
                            actions.child(stage)
                        }),
                });
                let header = hunk
                    .text
                    .lines()
                    .next()
                    .filter(|line| line.starts_with("@@"))
                    .map(|line| {
                        line.find(" @@")
                            .map_or(line, |end| &line[..end + 3])
                            .to_owned()
                    });
                // Kit exposes file headers, not a hunk-action callback. Keep the
                // revision-checked controls with this immutable hunk identity.
                let id = format!("turn-hunk-{}-{}", self.turn, hunk.id);
                body = body.child(
                    v_flex()
                        .id(id.clone())
                        .test_support()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().border)
                        .overflow_hidden()
                        .child(
                            h_flex()
                                .px_3()
                                .py_1p5()
                                .gap_2()
                                .justify_between()
                                .bg(cx.theme().muted)
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(cx.theme().muted_foreground)
                                        .children(header),
                                )
                                .child(actions),
                        )
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
            .floating_surface(cx)
            .title(title)
            .w((window.viewport_size().width - px(48.)).min(px(900.)))
            .max_h(window.viewport_size().height * 0.8)
            .child(body.overflow_y_scrollbar())
    }
}

/// Settled state shown in place of a hunk's actions.
fn decision(state: Decision) -> Tag {
    let (tag, key) = match state {
        Decision::Accepted => (Tag::success(), "hunks_state_accepted"),
        Decision::Rejected => (Tag::danger(), "hunks_state_rejected"),
        Decision::Staged => (Tag::info(), "hunks_state_staged"),
    };
    tag.small().child(tr(key))
}
