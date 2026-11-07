//! Composer activity combines registered badges and shared conversation state.
use super::*;
use gpui_kit::component::{button::Button, popover::Popover, spinner::Spinner};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::conversation::progress::Progress;

impl View {
    pub(super) fn turn_progress(&self, turn: TurnId) -> Option<Progress> {
        self.history
            .calls
            .iter()
            .rev()
            .filter(|call| call.turn == turn)
            .find_map(|call| call.progress.clone())
    }

    pub(super) fn composer_activity(&self, cx: &mut Context<Self>) -> AnyElement {
        let badges = self.registered_controls(
            sailry_protocol::plugin::ui::Slot::Status,
            sailry_protocol::plugin::ui::Align::Start,
            crate::plugins::contributions::Form::Toolbar,
            cx,
        );
        let progress = self
            .active()
            .and_then(|turn| self.turn_progress(turn))
            .filter(|progress| {
                let (completed, total) = tools::progress::counts(progress);
                completed < total
            });
        let animated = self.history.connected && self.active().is_some();
        let children = (!self.readonly())
            .then(|| self.child_activity(cx))
            .flatten();
        let commands = self.command_activity(cx);
        let approval = self.pending_approvals(cx);
        let question = self.pending_question(cx);
        // An empty strip still reserves one chip row so the composer never shifts.
        let selector = if progress.is_none()
            && children.is_none()
            && commands.is_none()
            && badges.is_empty()
            && approval.is_none()
            && question.is_none()
        {
            "composer-activity-reserve"
        } else {
            "composer-activity"
        };
        h_flex()
            .debug_selector(move || selector.into())
            .w_full()
            .min_h_7()
            .items_center()
            .px_4()
            .gap_2()
            .children(badges)
            .when_some(progress, |bar, progress| {
                let (completed, total) = tools::progress::counts(&progress);
                bar.child(
                    crate::theme::popover(Popover::new("live-todo-popover"), cx)
                        .anchor(Anchor::BottomLeft)
                        .bottom_2()
                        .trigger(
                            crate::theme::capsule(Button::new("live-todo"), cx)
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
                                .debug_selector(|| "composer-todo-content".into())
                                .w(px(320.).min(window.viewport_size().width - px(48.)))
                                .child(tools::progress::list(
                                    "composer-todo",
                                    &progress,
                                    false,
                                    animated,
                                    cx,
                                ))
                        }),
                )
            })
            .children(commands)
            .children(children)
            .child(div().flex_1())
            .children(approval)
            .children(question)
            .into_any_element()
    }
}

/// A Kit spinner keeps the status popover interactive, unlike Button::loading.
pub(super) fn working(
    button: Button,
    icon: IconName,
    label: String,
    running: bool,
    cx: &App,
) -> Button {
    let label = SharedString::from(label);
    let accessible = if running {
        format!("{label}: {}", tr("chat_running")).into()
    } else {
        label.clone()
    };
    crate::theme::capsule(button, cx)
        .accessibility_label(accessible)
        .icon(icon)
        .label(label)
        .when(running, |button| button.icon(Spinner::new()))
}

/// One item uses its own resource, since languages differ beyond appending a plural suffix.
pub(super) fn count(one: &str, many: &str, count: usize, locale: &str) -> String {
    if count == 1 {
        rust_i18n::t!(one, locale = locale).to_string()
    } else {
        rust_i18n::t!(many, locale = locale, count = count).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn counts() {
        for (single, plural, one, many) in [
            (
                "composer_subagent_one",
                "composer_subagent_count",
                "1 subagent",
                "3 subagents",
            ),
            (
                "background_one",
                "background_count",
                "1 terminal",
                "3 terminals",
            ),
        ] {
            assert_eq!(count(single, plural, 1, "en"), one);
            assert_eq!(count(single, plural, 3, "en"), many);
        }
    }
}
