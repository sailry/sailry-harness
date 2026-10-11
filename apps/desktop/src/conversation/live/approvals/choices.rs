use super::*;
use gpui_kit::component::{button::DropdownButton, menu::PopupMenuItem};

impl View {
    pub(super) fn decision_controls(
        &self,
        approval: &Approval,
        primary: bool,
        disabled: bool,
        access: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = approval.id;
        let row = h_flex().gap_2().pt_2().justify_end().flex_shrink_0();
        if let Some(draft) = self.approvals.drafts.get(&id).filter(|draft| draft.editing) {
            return row
                .child(
                    Button::new(format!("approval-reason-cancel-{id}"))
                        .ghost()
                        .small()
                        .rounded_full()
                        .label(tr("approval_reason_cancel"))
                        .disabled(disabled)
                        .debug_selector(move || format!("live-approval-reason-cancel-{id}"))
                        .on_click(cx.listener(move |view, _, _, cx| view.cancel_reason(id, cx))),
                )
                .child(
                    Button::new(format!("approval-reason-submit-{id}"))
                        .primary()
                        .small()
                        .rounded_full()
                        .label(tr("approval_reject"))
                        .disabled(disabled || draft.input.read(cx).value().trim().is_empty())
                        .debug_selector(move || format!("live-approval-reason-submit-{id}"))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.submit_reason(id, window, cx)
                        })),
                )
                .into_any_element();
        }
        let reject = Button::new(format!("approval-reject-{id}"))
            .ghost()
            .small()
            .min_w_12()
            .label(tr("approval_reject"))
            .disabled(disabled)
            .debug_selector(move || format!("live-approval-reject-{id}"))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.resolve_approval(id, Decision::Deny, window, cx)
            }));
        let owner = cx.weak_entity();
        let reject = div()
            .debug_selector(move || format!("live-approval-reject-options-{id}"))
            .child(
                DropdownButton::new(format!("approval-reject-split-{id}"))
                    .button(reject)
                    .small()
                    .disabled(disabled)
                    .dropdown_menu(move |menu, _, _| {
                        let owner = owner.clone();
                        menu.item(PopupMenuItem::new(tr("approval_deny_reason")).on_click(
                            move |_, window, cx| {
                                let _ =
                                    owner.update(cx, |view, cx| view.begin_reason(id, window, cx));
                            },
                        ))
                    }),
            );
        let approve = Button::new(format!("approval-approve-{id}"))
            .when(primary, |button| button.primary())
            .small()
            .when(!approval.can_remember, |button| button.rounded_full())
            .min_w_12()
            .label(tr("approval_allow_once"))
            .when_some(access, |button, access| button.tooltip(access))
            .disabled(disabled)
            .debug_selector(move || format!("live-approval-approve-{id}"))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.resolve_approval(id, Decision::Approve, window, cx)
            }));
        let approve = if approval.can_remember {
            let owner = cx.weak_entity();
            let prefix = approval.prefix.clone();
            div()
                .debug_selector(move || format!("live-approval-approve-options-{id}"))
                .child(
                    DropdownButton::new(format!("approval-approve-split-{id}"))
                        .button(approve)
                        .small()
                        .disabled(disabled)
                        .dropdown_menu(move |menu, _, _| {
                            let session_owner = owner.clone();
                            menu.item(PopupMenuItem::new(tr("approval_allow_session")).on_click(
                                move |_, window, cx| {
                                    let _ = session_owner.update(cx, |view, cx| {
                                        view.resolve_approval(
                                            id,
                                            Decision::ApproveSession,
                                            window,
                                            cx,
                                        )
                                    });
                                },
                            ))
                            .when_some(
                                prefix.clone(),
                                |menu, prefix| {
                                    let owner = owner.clone();
                                    let label = rust_i18n::t!(
                                        "approval_allow_prefix",
                                        prefix = prefix.as_str()
                                    )
                                    .to_string();
                                    menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, window, cx| {
                                            let prefix = prefix.clone();
                                            let _ = owner.update(cx, |view, cx| {
                                                view.resolve_approval(
                                                    id,
                                                    Decision::ApprovePrefix(prefix),
                                                    window,
                                                    cx,
                                                )
                                            });
                                        },
                                    ))
                                },
                            )
                        }),
                )
                .into_any_element()
        } else {
            approve.into_any_element()
        };
        row.child(reject).child(approve).into_any_element()
    }
}
