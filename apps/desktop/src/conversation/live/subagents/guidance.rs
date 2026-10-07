//! Guidance drafts capture a running child, not another execution lifecycle.
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::conversation::{Input, RunKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::conversation::live) struct Recipient {
    pub turn: TurnId,
    pub revision: u64,
}

impl View {
    pub(in crate::conversation::live) fn guide_target(&self) -> Option<Recipient> {
        if !self.readonly() {
            return None;
        }
        self.history
            .snapshot
            .as_ref()?
            .page
            .runs
            .iter()
            .find_map(|run| {
                (Some(run.session) == self.session()
                    && run.kind == RunKind::Task
                    && run.status == Status::Running)
                    .then_some(Recipient {
                        turn: run.turn,
                        revision: run.revision,
                    })
            })
    }

    fn guide_pending(&self, recipient: Recipient) -> bool {
        self.history.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .page
                .queue
                .items
                .iter()
                .any(|item| item.guidance == Some(recipient.turn))
        })
    }

    pub(in crate::conversation::live) fn guide(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(recipient) = self.guide_target() else {
            return;
        };
        if self.busy()
            || !self.connected()
            || self.guide_pending(recipient)
            || !self.attachments.sendable()
            || self.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            return;
        }
        let message = Input {
            text: self.input.read(cx).value().to_string(),
            attachments: self.attachments.ids(),
            references: self.active_references(cx),
        };
        if message.is_empty() && !self.has_attachments() {
            return;
        }
        self.references.dismiss();
        self.show_outgoing(message.clone(), cx);
        // Keep this target while asynchronous staging finishes. A later run must
        // never silently receive an input written for this captured revision.
        self.prepared_guidance = Some(recipient);
        if !self.prepare_attachments(message.clone(), window, cx) {
            self.prepared_guidance = None;
            self.guide_input(recipient, message, window, cx);
        }
    }

    pub(in crate::conversation::live) fn guide_input(
        &mut self,
        recipient: Recipient,
        message: Input,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.execute(
            Command::GuideTurn {
                recipient: recipient.turn,
                expected_revision: recipient.revision,
                message: message.clone(),
            },
            actions::Action::Send(message),
            window,
            cx,
        );
    }

    pub(in crate::conversation::live) fn guidance_composer(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let recipient = self.guide_target();
        let blocked = self.busy()
            || !self.connected()
            || recipient.is_none_or(|recipient| self.guide_pending(recipient))
            || !self.attachments.sendable()
            || (self.input.read(cx).value().trim().is_empty() && !self.has_attachments());
        super::super::super::layout::composer(
            self.notices(cx),
            v_flex()
                .id("child-guide-drop")
                .gap_2()
                .on_drag_move(super::super::attachments::pointer_drag)
                .when(!self.attachments_blocked(), |composer| {
                    composer
                        .drag_over::<ExternalPaths>(|style, _, _, cx| {
                            style.bg(cx.theme().drop_target)
                        })
                        .on_drop(cx.listener(|view, paths: &ExternalPaths, window, cx| {
                            view.attach_paths(paths.paths().to_vec(), window, cx);
                            view.focus(window, cx);
                        }))
                })
                .children(self.attachment_drafts(cx))
                .child(self.input_field("child-guide-input", cx))
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("child-guide-attach")
                                .ghost()
                                .rounded_full()
                                .icon(IconName::Plus)
                                .disabled(self.attachments_blocked())
                                .debug_selector(|| "child-guide-attach".into())
                                .tooltip(tr("composer_attachment"))
                                .accessibility_label(tr("composer_attachment"))
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.choose_attachments(window, cx)
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("child-guide")
                                .primary()
                                .rounded_full()
                                .label(tr("chat_child_guide"))
                                .disabled(blocked)
                                .debug_selector(|| "child-guide".into())
                                .on_click(
                                    cx.listener(|view, _, window, cx| view.guide(window, cx)),
                                ),
                        ),
                )
                .into_any_element(),
            None,
            Some(self.reference_picker(cx)),
            self.sidebar,
            cx,
        )
    }
}
