use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::Textarea,
    popover::Popover,
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_client::conversation::{children, tools::Call};
use sailry_protocol::{
    ApprovalId,
    conversation::{Approval, ApprovalState, Decision, Page},
};

mod actions;
mod choices;
mod draft;
#[cfg(test)]
mod tests;
mod watch;
pub(super) use self::actions::Actions;

impl View {
    pub(super) fn pending_approvals(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.readonly() {
            return None;
        }
        let calls: Vec<_> = std::iter::once(&self.history)
            .chain(self.approvals.children.values().map(|child| &child.history))
            .flat_map(|history| {
                history.calls.iter().filter_map(move |call| {
                    let page = &history.snapshot.as_ref()?.page;
                    call.approval
                        .as_ref()
                        .is_some_and(|approval| {
                            approval.state == ApprovalState::Pending
                                && !self.approvals.resolved(approval.id)
                        })
                        .then_some((call, page, history.connected))
                })
            })
            .collect();
        if calls.is_empty() && self.approvals.error.is_none() {
            return None;
        }
        let busy = self.approvals.pending || self.approvals.attempt.is_some();
        let rows = calls
            .iter()
            .enumerate()
            .map(|(position, (call, page, connected))| {
                let reference = call.detail_reference(page);
                self.request_details(reference.clone(), false, cx);
                let hydrated = self.detailed(call, page);
                let call = &hydrated;
                let state = reference
                    .as_ref()
                    .map(|reference| self.details.state(reference));
                let loaded = call.detail.is_some()
                    || matches!(
                        state,
                        Some(sailry_client::conversation::details::State::TooLarge(_))
                    );
                let failed = reference.as_ref().is_some_and(|reference| {
                    matches!(
                        self.details.state(reference),
                        sailry_client::conversation::details::State::Failed { retry_at: None, .. }
                    )
                });
                let approval = call.approval.as_ref().unwrap();
                let id = approval.id;
                let details = details(call, page);
                let access = details.access.clone();
                let disabled = busy
                    || !loaded
                    || !self.connected()
                    || !connected
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
                    .when(approval.session != self.session().unwrap(), |row| {
                        row.child(
                            h_flex()
                                .gap_2()
                                .child(crate::ui::identicon::child(
                                    &approval.session.to_string(),
                                    self.child(approval.session)
                                        .is_some_and(|child| children::active(child.run.status)),
                                    cx,
                                ))
                                .child(
                                    div()
                                        .debug_selector(move || format!("live-approval-child-{id}"))
                                        .child(
                                            self.node
                                                .snapshot
                                                .as_ref()
                                                .and_then(|snapshot| {
                                                    snapshot.sessions.iter().find(|session| {
                                                        session.id == approval.session
                                                    })
                                                })
                                                .map(|session| {
                                                    SharedString::from(
                                                        session.activity.title.clone(),
                                                    )
                                                })
                                                .unwrap_or_else(|| tr("chat_child")),
                                        ),
                                ),
                        )
                    })
                    .child(
                        div()
                            .font_medium()
                            .debug_selector(move || format!("live-approval-title-{id}"))
                            .child(
                                call.row(page)
                                    .label
                                    .as_ref()
                                    .map(|label| {
                                        SharedString::from(
                                            label.label(&rust_i18n::locale()).to_owned(),
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
                    .when_some(approval.ssh.as_ref(), |row, trust| {
                        row.child(
                            div()
                                .debug_selector(move || format!("live-approval-ssh-{id}"))
                                .font_medium()
                                .whitespace_normal()
                                .child(ssh_prompt(trust)),
                        )
                    })
                    .children(
                        details
                            .notices
                            .into_iter()
                            .enumerate()
                            .map(|(position, notice)| {
                                div()
                                    .debug_selector(move || {
                                        format!("live-approval-notice-{id}-{position}")
                                    })
                                    .whitespace_normal()
                                    .line_clamp(3)
                                    .text_ellipsis()
                                    .child(notice)
                            }),
                    )
                    .when_some(details.description, |row, description| {
                        let body = div()
                            .debug_selector(move || format!("live-approval-description-{id}"))
                            .whitespace_normal()
                            .child(description);
                        row.child(if details.code {
                            v_flex()
                                .debug_selector(move || format!("live-approval-command-{id}"))
                                .rounded_md()
                                .bg(cx.theme().background)
                                .max_h_32()
                                .flex_shrink_0()
                                .child(
                                    v_flex().flex_1().overflow_hidden().child(
                                        div().flex_1().overflow_hidden().child(
                                            v_flex()
                                                .size_full()
                                                .p_3()
                                                .font_family(cx.theme().mono_font_family.clone())
                                                .child(body)
                                                .overflow_y_scrollbar()
                                                .id(format!("approval-command-{id}")),
                                        ),
                                    ),
                                )
                                .into_any_element()
                        } else {
                            body.line_clamp(2).text_ellipsis().into_any_element()
                        })
                    })
                    .when_some(
                        self.approvals.drafts.get(&id).filter(|draft| draft.editing),
                        |row, draft| {
                            row.child(
                                div()
                                    .debug_selector(move || format!("live-approval-reason-{id}"))
                                    .child(
                                        Textarea::new(&draft.input)
                                            .small()
                                            .aria_label(tr("approval_reason"))
                                            .disabled(disabled),
                                    ),
                            )
                        },
                    )
                    .when(!loaded, |row| {
                        row.child(if failed {
                            crate::feedback::retry(format!("approval-details-retry-{id}"))
                                .disabled(!self.connected() || !connected)
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.request_details(reference.clone(), true, cx);
                                    cx.notify();
                                }))
                                .into_any_element()
                        } else {
                            gpui_kit::component::spinner::Spinner::new().into_any_element()
                        })
                    })
                    .child(self.decision_controls(approval, position == 0, disabled, access, cx))
                    .debug_selector(move || format!("live-approval-request-{id}"))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let heading = if calls.is_empty() {
            tr("approval_receipt")
        } else {
            rust_i18n::t!("approval_live_count", count = calls.len())
                .to_string()
                .into()
        };
        Some(
            crate::theme::popover(Popover::new("live-approval-popover"), cx)
                .anchor(Anchor::BottomRight)
                .bottom_2()
                .overlay_closable(false)
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
    pub access: Option<SharedString>,
    pub notices: Vec<SharedString>,
    pub code: bool,
}

pub(super) fn details(call: &Call, page: &Page) -> Details {
    let view = call.approval_view(page);
    let localize = |value: sailry_protocol::tool::LocalText| {
        SharedString::from(value.label(&rust_i18n::locale()).to_owned())
    };
    Details {
        prompt: view.prompt.map(localize),
        context: view.context,
        description: view.description.map(localize),
        access: view.unrestricted.then(|| tr("approval_unrestricted")),
        notices: view.notices.into_iter().map(localize).collect(),
        code: view.code,
    }
}

fn ssh_prompt(trust: &sailry_protocol::ssh::Trust) -> String {
    let key = if trust.changed {
        "approval_ssh_replace"
    } else {
        "approval_ssh_trust"
    };
    rust_i18n::t!(
        key,
        host = trust.host,
        port = trust.port,
        algorithm = trust.key.algorithm,
        fingerprint = trust.key.fingerprint
    )
    .to_string()
}
