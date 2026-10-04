pub(crate) use super::turn::Target;
use super::turn::{Block, Status};
use crate::{shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub(crate) mod presentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Pending,
    Approved,
    Rejected,
    Cancelled,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "approval_pending",
            Self::Approved => "approval_approved",
            Self::Rejected => "approval_rejected",
            Self::Cancelled => "approval_cancelled",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Pending => IconName::CircleUser,
            Self::Approved => IconName::CircleCheck,
            Self::Rejected | Self::Cancelled => IconName::CircleX,
        }
    }
}

/// In-memory request presentation, not an execution authorization.
#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub id: usize,
    pub generation: usize,
    pub prompt: SharedString,
    pub target: SharedString,
    pub state: State,
}

impl Shell {
    pub(super) fn pending_approvals(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let key = (self.host, self.session);
        let thread = &self.conversations[&key];
        let turn = thread.turns.last()?;
        if turn.status != Status::Waiting {
            return None;
        }
        let index = thread.turns.len() - 1;
        let requests: Vec<_> = turn
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Approval(request) if request.state == State::Pending => Some(request),
                _ => None,
            })
            .collect();
        if requests.is_empty() {
            return None;
        }
        let rows = requests
            .into_iter()
            .enumerate()
            .map(|(position, request)| {
                let target = Target {
                    session: key,
                    turn: index,
                    request: request.id,
                    generation: request.generation,
                };
                let id = request.id;
                presentation::row(
                    request.prompt.clone(),
                    h_flex()
                        .gap_2()
                        .flex_shrink_0()
                        .child(
                            Button::new(("approval-reject", id))
                                .small()
                                .min_w_12()
                                .label(tr("approval_reject"))
                                .debug_selector(move || format!("approval-reject-{id}"))
                                .on_click(cx.listener(move |shell, _, window, cx| {
                                    shell.decide_preview(target, State::Rejected, cx);
                                    shell.resume_queued_preview(target.session, window, cx);
                                })),
                        )
                        .child(
                            Button::new(("approval-approve", id))
                                .when(position == 0, |button| button.primary())
                                .small()
                                .min_w_12()
                                .label(tr("approval_approve"))
                                .debug_selector(move || format!("approval-approve-{id}"))
                                .on_click(cx.listener(move |shell, _, window, cx| {
                                    shell.decide_preview(target, State::Approved, cx);
                                    shell.resume_queued_preview(target.session, window, cx);
                                })),
                        ),
                )
                .debug_selector(move || format!("approval-request-{id}"))
                .into_any_element()
            })
            .collect::<Vec<_>>();
        Some(presentation::panel(
            format!("approval-scroll-{}-{}-{index}", key.0, key.1),
            rust_i18n::t!("approval_count", count = rows.len())
                .to_string()
                .into(),
            rows,
            cx,
        ))
    }

    pub(super) fn approval_summary(
        &self,
        id: String,
        request: &Request,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let link_id = format!("{id}-target");
        let request_id = request.id;
        h_flex()
            .id(id.clone())
            .debug_selector(move || format!("approval-history-{id}"))
            .w_full()
            .gap_2()
            .items_start()
            .child(Icon::new(request.state.icon()).size_4())
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div().text_sm().whitespace_normal().child(
                            rust_i18n::t!(
                                "approval_summary",
                                state = tr(request.state.label()),
                                prompt = request.prompt
                            )
                            .to_string(),
                        ),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("approval-link-{request_id}"))
                            .child(self.linked_message(
                                link_id,
                                format!("[{}]({})", request.target, request.target).into(),
                                cx,
                            )),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn decide_preview(
        &mut self,
        target: Target,
        decision: State,
        cx: &mut Context<Self>,
    ) {
        if !matches!(decision, State::Approved | State::Rejected) {
            return;
        }
        let Some(thread) = self.conversations.get_mut(&target.session) else {
            return;
        };
        let Some(turn) = thread.turns.get_mut(target.turn) else {
            return;
        };
        if turn.status != Status::Waiting {
            return;
        }
        let Some(request) = turn.blocks.iter_mut().find_map(|block| match block {
            Block::Approval(request)
                if request.id == target.request
                    && request.generation == target.generation
                    && request.state == State::Pending =>
            {
                Some(request)
            }
            _ => None,
        }) else {
            return;
        };
        request.state = decision;
        if !turn.has_pending_requests() {
            turn.status = Status::Completed;
            turn.finished_at = Some(tr("turn_preview_time"));
            turn.blocks.push(Block::Text(tr("approval_preview_result")));
        }
        thread.scroller.update(cx, |state, cx| {
            state.remeasure_items(target.turn..target.turn + 1, cx)
        });
        cx.notify();
    }
}
