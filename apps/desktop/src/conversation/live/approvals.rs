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
                let approval = call.approval.as_ref().unwrap();
                let id = approval.id;
                let details = details(call, page);
                let access = details.access.clone();
                let disabled = busy
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
    let arguments = call.arguments(page).unwrap_or(&serde_json::Value::Null);
    let access = call.approval.as_ref().and_then(|approval| {
        (approval.access == Some(sailry_protocol::process::Access::Unrestricted))
            .then(|| tr("approval_unrestricted"))
    });
    let Some(display) = call.display(page) else {
        return Details {
            access,
            ..Default::default()
        };
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
    let code = display
        .input
        .as_ref()
        .and_then(|input| input.summary.as_ref())
        .is_some_and(|field| field.code && field.read(arguments) == description.as_deref());
    Details {
        prompt: description
            .clone()
            .or_else(|| Some(display.label(&rust_i18n::locale()).to_owned().into())),
        context: display
            .input
            .as_ref()
            .and_then(|input| read(&input.context)),
        notices: display
            .approval
            .iter()
            .filter_map(|prompt| prompt.render(arguments, &rust_i18n::locale()))
            .filter(|notice| Some(notice.as_str()) != description.as_deref())
            .map(Into::into)
            .collect(),
        description,
        access,
        code,
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
