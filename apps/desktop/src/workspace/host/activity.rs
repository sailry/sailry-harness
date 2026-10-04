use crate::{
    conversation::{
        approval,
        turn::{Block, Status},
    },
    shell::Shell,
    tr,
};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub(super) struct Activity {
    key: (usize, usize),
    title: SharedString,
    location: SharedString,
    model: Option<SharedString>,
    status: &'static str,
    waiting: bool,
}

impl Shell {
    pub(super) fn host_activity(&self, host: usize) -> Vec<Activity> {
        self.workspace
            .sessions
            .iter()
            .filter_map(|(&key, session)| {
                if session.owner.host != host || !self.workspace.contains(session.owner) {
                    return None;
                }
                let turn = self.conversations.get(&key)?.turns.last()?;
                if !turn.status.active() {
                    return None;
                }
                let project = &self.workspace.projects[&session.owner.project];
                let branch = self.workspace.branch_label(session.owner);
                let status = if turn.status == Status::Waiting
                    && turn.blocks.iter().any(|block| {
                        matches!(block, Block::Approval(request)
                            if request.state == approval::State::Pending)
                    }) {
                    "approval_pending"
                } else {
                    turn.status.phase_label()
                };
                Some(Activity {
                    key,
                    title: session.title.clone(),
                    location: format!("{} / {}", project.name, branch).into(),
                    model: turn
                        .options
                        .as_ref()
                        .and_then(|options| options.model.as_ref())
                        .map(|model| model.model.clone().into()),
                    status,
                    waiting: turn.status == Status::Waiting,
                })
            })
            .collect()
    }

    pub(super) fn host_activity_section(
        &self,
        records: Vec<Activity>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .gap_2()
            .debug_selector(|| "host-activity".into())
            .child(
                h_flex()
                    .gap_2()
                    .child(div().font_semibold().child(tr("workspace_running")))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .debug_selector(|| "host-activity-count".into())
                            .child(records.len().to_string()),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("host_activity_hint")),
            )
            .when(records.is_empty(), |this| {
                this.child(
                    crate::empty_state::card(IconName::LayoutDashboard, "workspace_no_running", cx)
                        .debug_selector(|| "host-activity-empty".into()),
                )
            })
            .children(records.into_iter().map(|record| {
                let key = record.key;
                Button::new(format!("host-activity-{}-{}", key.0, key.1))
                    .custom(crate::theme::subtle_button(cx))
                    .w_full()
                    .h_auto()
                    .py_2()
                    .accessibility_label(record.title.clone())
                    .debug_selector(move || format!("host-activity-{}-{}", key.0, key.1))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_3()
                            .child(
                                Icon::new(if record.waiting {
                                    IconName::CircleUser
                                } else {
                                    IconName::Bot
                                })
                                .size_5()
                                .flex_shrink_0(),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_0p5()
                                    .child(div().truncate().text_sm().child(record.title))
                                    .child(
                                        div()
                                            .truncate()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(record.location),
                                    ),
                            )
                            .when_some(record.model, |this, model| {
                                this.child(div().max_w_32().truncate().text_xs().child(model))
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .flex_shrink_0()
                                    .text_color(cx.theme().muted_foreground)
                                    .debug_selector(move || {
                                        format!("host-activity-status-{}-{}", key.0, key.1)
                                    })
                                    .child(tr(record.status)),
                            ),
                    )
                    .on_click(cx.listener(move |shell, _, window, cx| {
                        shell.select_session(key, window, cx)
                    }))
            }))
            .into_any_element()
    }
}
