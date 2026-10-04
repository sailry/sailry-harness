use crate::{
    preview::{FILE_DIFF_KEYS, FILES, Page},
    shell::Shell,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    popover::{Popover, PopoverState},
    progress::ProgressCircle,
    scroll::ScrollableElement,
    separator::Separator,
    spinner::Spinner,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

#[derive(Clone, Copy, PartialEq)]
enum TaskStatus {
    Done,
    Active,
    Pending,
}

const TASKS: [(&str, TaskStatus); 5] = [
    ("composer_task_done", TaskStatus::Done),
    ("composer_task_active", TaskStatus::Active),
    ("composer_task_theme", TaskStatus::Pending),
    ("composer_task_pending", TaskStatus::Pending),
    ("composer_task_verify", TaskStatus::Pending),
];

impl Shell {
    pub(super) fn composer_activity(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let sample = self.session < 2;
        let tasks = if sample { TASKS.as_slice() } else { &[] };
        let completed = tasks
            .iter()
            .filter(|(_, state)| *state == TaskStatus::Done)
            .count();
        let changes = sample && self.repository() == crate::workspace::Repository::Ready;
        let (added, removed) = if changes { change_counts() } else { (0, 0) };
        let changes_label = rust_i18n::t!(
            "composer_changes_hint",
            count = if changes { FILES.len() } else { 0 },
            added = added,
            removed = removed,
        )
        .to_string();
        h_flex()
            .debug_selector(|| "composer-activity".into())
            .mx_2()
            .gap_2()
            .child(
                Popover::new("composer-todo-popover")
                    .anchor(Anchor::BottomLeft)
                    .p_2()
                    .trigger(
                        Button::new("composer-todo")
                            .custom(crate::theme::subtle_button(cx))
                            .rounded_full()
                            .icon(IconName::Menu)
                            .debug_selector(|| "composer-todo".into())
                            .tooltip(tr("context_description"))
                            .label(
                                rust_i18n::t!(
                                    "composer_todo_count",
                                    completed = completed,
                                    total = tasks.len()
                                )
                                .to_string(),
                            ),
                    )
                    .content(move |_, window, cx| todo_content(tasks, window, cx)),
            )
            .child(Separator::vertical().h_4())
            .child(
                Button::new("composer-changes")
                    .custom(crate::theme::subtle_button(cx))
                    .rounded_full()
                    .icon(IconName::Network)
                    .debug_selector(|| "composer-changes".into())
                    .accessibility_label(changes_label.clone())
                    .tooltip(changes_label)
                    .child(
                        h_flex()
                            .gap_2()
                            .text_sm()
                            .child(
                                div()
                                    .text_color(cx.theme().success)
                                    .child(format!("+{added}")),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().danger)
                                    .child(format!("-{removed}")),
                            ),
                    )
                    .disabled(!changes)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_resource_panel(Page::Git, window, cx)
                    })),
            )
            .when_some(self.subagent_activity(cx), |bar, agents| {
                bar.child(Separator::vertical().h_4()).child(agents)
            })
    }
}

fn todo_content(
    tasks: &'static [(&str, TaskStatus)],
    window: &mut Window,
    cx: &mut Context<PopoverState>,
) -> AnyElement {
    let completed = tasks
        .iter()
        .filter(|(_, state)| *state == TaskStatus::Done)
        .count();
    let remaining = tasks.len() - completed;
    v_flex()
        .debug_selector(|| "composer-todo-content".into())
        .w(px(320.).min(window.viewport_size().width - px(48.)))
        .gap_2()
        .child(
            h_flex()
                .px_2()
                .gap_2()
                .child(
                    ProgressCircle::new("todo-progress")
                        .value(if tasks.is_empty() {
                            0.
                        } else {
                            completed as f32 / tasks.len() as f32 * 100.
                        })
                        .with_size(px(24.))
                        .color(cx.theme().muted_foreground)
                        .accessibility_label(tr("composer_todo_title")),
                )
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            rust_i18n::t!("composer_todo_remaining", count = remaining).to_string(),
                        ),
                )
                .child(
                    Button::new("composer-todo-collapse")
                        .custom(crate::theme::subtle_button(cx))
                        .rounded_full()
                        .icon(IconName::Minus)
                        .tooltip(tr("composer_todo_collapse"))
                        .accessibility_label(tr("composer_todo_collapse"))
                        .debug_selector(|| "composer-todo-collapse".into())
                        .on_click(cx.listener(|state, _, window, cx| state.dismiss(window, cx))),
                ),
        )
        .child(
            v_flex()
                .id("composer-todo-list")
                .gap_1()
                .max_h(px(320.).min(window.viewport_size().height * 0.5))
                .overflow_y_scrollbar()
                .when(tasks.is_empty(), |body| {
                    body.child(
                        div()
                            .p_2()
                            .text_sm()
                            .debug_selector(|| "composer-todo-empty".into())
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("composer_todo_empty")),
                    )
                })
                .children(tasks.iter().enumerate().map(|(index, (key, status))| {
                    h_flex()
                        .id(("todo-task", index))
                        .debug_selector(move || format!("composer-todo-task-{index}"))
                        .aria_label(format!(
                            "{}: {}",
                            tr(match status {
                                TaskStatus::Done => "composer_task_status_done",
                                TaskStatus::Active => "composer_task_status_active",
                                TaskStatus::Pending => "composer_task_status_pending",
                            }),
                            tr(key)
                        ))
                        .px_2()
                        .py_1p5()
                        .gap_2()
                        .text_sm()
                        .border_1()
                        .border_color(if *status == TaskStatus::Active {
                            cx.theme().border
                        } else {
                            cx.theme().transparent
                        })
                        .rounded_full()
                        .text_color(if *status == TaskStatus::Active {
                            cx.theme().foreground
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(match status {
                            TaskStatus::Done => {
                                Icon::new(IconName::CircleCheck).size_4().into_any_element()
                            }
                            TaskStatus::Active => Spinner::new()
                                .color(cx.theme().muted_foreground)
                                .icon(IconName::LoaderCircle)
                                .with_size(px(16.))
                                .into_any_element(),
                            TaskStatus::Pending => ProgressCircle::new(("todo-pending", index))
                                .value(0.)
                                .with_size(px(20.))
                                .into_any_element(),
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .when(*status == TaskStatus::Done, |label| label.line_through())
                                .child(tr(key)),
                        )
                })),
        )
        .into_any_element()
}

fn change_counts() -> (usize, usize) {
    FILE_DIFF_KEYS.iter().fold((0, 0), |(added, removed), key| {
        let diff = tr(key);
        (
            added
                + diff
                    .lines()
                    .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
                    .count(),
            removed
                + diff
                    .lines()
                    .filter(|line| line.starts_with('-') && !line.starts_with("---"))
                    .count(),
        )
    })
}
