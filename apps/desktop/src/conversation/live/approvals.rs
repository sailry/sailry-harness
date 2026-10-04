use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    popover::Popover,
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::conversation::tools::Call;
use sailry_protocol::{
    ApprovalId,
    conversation::{ApprovalState, Decision, Page},
};

mod actions;
#[cfg(test)]
mod tests;
pub(super) use actions::Actions;

impl View {
    pub(super) fn pending_approvals(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let page = &self.history.snapshot.as_ref()?.page;
        let calls: Vec<_> = self
            .history
            .calls
            .iter()
            .filter(|call| {
                call.approval
                    .as_ref()
                    .is_some_and(|approval| approval.state == ApprovalState::Pending)
            })
            .collect();
        if calls.is_empty() && self.approvals.error.is_none() {
            return None;
        }
        let busy = self.approvals.pending || self.approvals.attempt.is_some();
        let rows = calls.iter().enumerate().map(|(position, call)| {
            let approval = call.approval.as_ref().unwrap();
            let id = approval.id;
            let details = details(call, page);
            let disabled = busy
                || !self.connected()
                || !page
                    .runs
                    .iter()
                    .any(|run| run.turn == approval.turn && run.status == Status::Running);
            v_flex()
                .id(format!("approval-request-{id}"))
                .aria_label(details.prompt.unwrap_or_else(|| call.name.clone().into()))
                .w_full()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .font_medium()
                        .debug_selector(move || format!("live-approval-title-{id}"))
                        .child(
                            call.display(page)
                                .map(|display| {
                                    SharedString::from(
                                        display.label(&rust_i18n::locale()).to_owned(),
                                    )
                                })
                                .unwrap_or_else(|| {
                                    crate::conversation::surface::tool_label(&call.name)
                                }),
                        ),
                )
                .when(position > 0, |row| {
                    row.pt_3().border_t_1().border_color(cx.theme().border)
                })
                .when_some(details.context, |row, path| {
                    row.child(div().text_color(cx.theme().muted_foreground).child(path))
                })
                .when_some(details.description, |row, description| {
                    row.child(
                        div()
                            .debug_selector(move || format!("live-approval-description-{id}"))
                            .whitespace_normal()
                            .child(description),
                    )
                })
                .child(
                    h_flex()
                        .gap_2()
                        .pt_2()
                        .justify_end()
                        .flex_shrink_0()
                        .child(
                            Button::new(format!("approval-reject-{id}"))
                                .ghost()
                                .small()
                                .rounded_full()
                                .min_w_12()
                                .label(tr("approval_reject"))
                                .disabled(disabled)
                                .debug_selector(move || format!("live-approval-reject-{id}"))
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.resolve_approval(id, Decision::Deny, window, cx)
                                })),
                        )
                        .child(
                            Button::new(format!("approval-approve-{id}"))
                                .when(position == 0, |button| button.primary())
                                .small()
                                .rounded_full()
                                .min_w_12()
                                .label(tr("approval_approve"))
                                .disabled(disabled)
                                .debug_selector(move || format!("live-approval-approve-{id}"))
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.resolve_approval(id, Decision::Approve, window, cx)
                                })),
                        ),
                )
                .debug_selector(move || format!("live-approval-request-{id}"))
                .into_any_element()
        });
        let heading = if calls.is_empty() {
            tr("approval_receipt")
        } else {
            rust_i18n::t!("approval_live_count", count = calls.len())
                .to_string()
                .into()
        };
        Some(
            Popover::new("live-approval-popover")
                .anchor(Anchor::BottomRight)
                .bottom_2()
                .open(self.approvals.open)
                .on_open_change(cx.listener(|view, open: &bool, _, cx| {
                    view.approvals.open = *open;
                    cx.notify();
                }))
                .trigger(
                    Button::new("live-approval-trigger")
                        .outline()
                        .small()
                        .rounded_full()
                        .icon(IconName::CircleUser)
                        .font_normal()
                        .text_color(cx.theme().muted_foreground)
                        .label(heading)
                        .debug_selector(|| "live-approval-trigger".into()),
                )
                .child(
                    v_flex()
                        .id("pending-approvals")
                        .debug_selector(|| "pending-approvals".into())
                        .w_80()
                        .max_w_full()
                        .gap_3()
                        .max_h_64()
                        .overflow_y_scrollbar()
                        .children(rows),
                )
                .into_any_element(),
        )
    }
}

#[derive(Default)]
pub(super) struct Details {
    pub prompt: Option<SharedString>,
    pub context: Option<String>,
    pub description: Option<SharedString>,
}

pub(super) fn details(call: &Call, page: &Page) -> Details {
    let arguments = call.arguments(page).unwrap_or(&serde_json::Value::Null);
    let Some(display) = call.display(page) else {
        return Details::default();
    };
    let read = |field: &Option<sailry_protocol::tool::Field>| {
        field
            .as_ref()
            .and_then(|field| field.read(arguments))
            .map(str::to_owned)
    };
    let description: Option<SharedString> = display
        .approval
        .iter()
        .find_map(|prompt| prompt.render(arguments, &rust_i18n::locale()))
        .or_else(|| {
            let input = display.input.as_ref()?;
            read(&input.target).or_else(|| read(&input.summary))
        })
        .map(Into::into);
    Details {
        prompt: description
            .clone()
            .or_else(|| Some(display.label(&rust_i18n::locale()).to_owned().into())),
        context: display
            .input
            .as_ref()
            .and_then(|input| read(&input.context)),
        description,
    }
}

pub(super) fn label(state: ApprovalState) -> &'static str {
    match state {
        ApprovalState::Pending => "approval_pending",
        ApprovalState::Approved => "approval_approved",
        ApprovalState::Denied => "approval_rejected",
        ApprovalState::Cancelled => "approval_cancelled",
        ApprovalState::Interrupted => "approval_interrupted",
    }
}
