//! Composer activity combines registered badges and shared conversation state.
use super::*;
use gpui_kit::component::{button::Button, popover::Popover};
use gpui_kit::prelude::FluentBuilder as _;

impl View {
    pub(super) fn composer_activity(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let badges = self.registered_controls(
            sailry_protocol::plugin::ui::Slot::Status,
            sailry_protocol::plugin::ui::Align::Start,
            crate::plugins::contributions::Form::Toolbar,
            cx,
        );
        let active = self.active();
        let progress = self
            .history
            .calls
            .iter()
            .rev()
            .filter(|call| Some(call.turn) == active)
            .find_map(|call| call.progress.clone())
            .filter(|progress| {
                let (completed, total) = tools::progress::counts(progress);
                completed < total
            });
        let children = (!self.readonly())
            .then(|| self.child_activity(cx))
            .flatten();
        let commands = self.command_activity(cx);
        let approval = self.pending_approvals(cx);
        let question = self.pending_question(cx);
        let queue = (!self.readonly()).then(|| self.queue_bar(cx)).flatten();
        if progress.is_none()
            && children.is_none()
            && commands.is_none()
            && badges.is_empty()
            && approval.is_none()
            && question.is_none()
            && queue.is_none()
        {
            return None;
        }
        Some(
            h_flex()
                .debug_selector(|| "composer-activity".into())
                .w_full()
                .px_4()
                .gap_2()
                .children(badges)
                .when_some(progress, |bar, progress| {
                    let (completed, total) = tools::progress::counts(&progress);
                    bar.child(
                        Popover::new("live-todo-popover")
                            .anchor(Anchor::BottomLeft)
                            .bottom_2()
                            .trigger(
                                Button::new("live-todo")
                                    .outline()
                                    .small()
                                    .rounded_full()
                                    .px_3()
                                    .font_normal()
                                    .text_color(cx.theme().muted_foreground)
                                    .icon(Icon::default().path("reicon:ui/tasks"))
                                    .debug_selector(|| "composer-todo".into())
                                    .accessibility_label(tr("task_progress"))
                                    .label(
                                        rust_i18n::t!(
                                            "composer_todo_count",
                                            completed = completed,
                                            total = total
                                        )
                                        .to_string(),
                                    ),
                            )
                            .content(move |_, window, cx| {
                                div()
                                    .w(px(320.).min(window.viewport_size().width - px(48.)))
                                    .child(tools::progress::list(
                                        "composer-todo",
                                        &progress,
                                        false,
                                        cx,
                                    ))
                            }),
                    )
                })
                .children(commands)
                .children(children)
                .children(queue)
                .child(div().flex_1())
                .children(approval)
                .children(question)
                .into_any_element(),
        )
    }
}
