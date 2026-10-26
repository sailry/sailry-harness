use super::*;
use crate::conversation::queue::presentation;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    clipboard::Clipboard,
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;

impl View {
    pub(in crate::conversation::live) fn queue_tray(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.queue.update(cx, |panel, cx| panel.sync(self, cx));
        let panel = self.queue.read(cx);
        if panel.items().next().is_none() && panel.editing.is_none() && panel.outgoing.is_none() {
            return None;
        }
        Some(self.queue.clone().into_any_element())
    }
}

impl Panel {
    fn items(&self) -> impl Iterator<Item = &Pending> {
        self.queue
            .items
            .iter()
            .filter(|item| Some(item.turn) != self.represented)
    }

    fn row(&self, item: &Pending, cx: &mut Context<Self>) -> AnyElement {
        let turn = item.turn;
        let revision = item.revision;
        let key: SharedString = turn.to_string().into();
        let drag = Drag {
            node: self.binding.client.target(),
            session: self.session.expect("queue has a session"),
            turn,
            revision: self.queue.revision,
            text: label(item),
        };
        let action = if self.child { "read" } else { "edit" };
        let label_key = if self.child {
            "queue_read"
        } else {
            "queue_edit"
        };
        let edit = Button::new(format!("live-queue-edit-{turn}"))
            .rounded_full()
            .ghost()
            .small()
            .icon(if self.child {
                IconName::FileText
            } else {
                IconName::Replace
            })
            .disabled(self.blocked() || item.guidance.is_some())
            .debug_selector(move || format!("queue-{action}-{turn}"))
            .tooltip(tr(label_key))
            .accessibility_label(tr(label_key))
            .on_click(cx.listener(move |panel, _, window, cx| {
                cx.stop_propagation();
                panel.edit(turn, revision, window, cx);
            }));
        let send = Button::new(format!("live-queue-send-{turn}"))
            .rounded_full()
            .ghost()
            .small()
            .icon(IconName::ArrowUp)
            .disabled(self.blocked() || item.guidance.is_some())
            .debug_selector(move || format!("queue-send-{turn}"))
            .tooltip(tr("queue_send_now"))
            .accessibility_label(tr("queue_send_now"))
            .on_click(cx.listener(move |panel, _, window, cx| {
                cx.stop_propagation();
                panel.execute(
                    Command::SendQueuedTurn {
                        turn,
                        expected_revision: revision,
                    },
                    window,
                    cx,
                );
            }));
        let guide = self
            .recipient
            .filter(|_| {
                !self.child
                    && !item.notice
                    && item.kind == sailry_protocol::conversation::RunKind::Task
            })
            .map(|recipient| {
                Button::new(format!("live-queue-guide-{turn}"))
                    .rounded_full()
                    .ghost()
                    .small()
                    .icon(IconName::Redo2)
                    .tooltip(tr(if item.guidance.is_some() {
                        "queue_guided"
                    } else {
                        "chat_queue_guide"
                    }))
                    .accessibility_label(tr("chat_queue_guide"))
                    .disabled(self.blocked() || self.guidance_blocked || item.guidance.is_some())
                    .debug_selector(move || format!("queue-guide-{turn}"))
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        cx.stop_propagation();
                        panel.execute(
                            Command::GuideQueuedTurn {
                                turn,
                                recipient,
                                expected_revision: revision,
                            },
                            window,
                            cx,
                        );
                    }))
                    .into_any_element()
            });
        let remove = self
            .remove_button(turn, revision, cx)
            .disabled(self.blocked() || item.guidance.is_some());
        let withdraw = item.guidance.is_some().then(|| {
            Button::new(format!("live-queue-withdraw-{turn}"))
                .rounded_full()
                .ghost()
                .small()
                .label(tr("queue_withdraw"))
                .disabled(self.blocked())
                .debug_selector(move || format!("queue-withdraw-{turn}"))
                .on_click(cx.listener(move |panel, _, window, cx| {
                    cx.stop_propagation();
                    panel.execute(
                        Command::WithdrawGuidance {
                            turn,
                            expected_revision: revision,
                        },
                        window,
                        cx,
                    );
                }))
                .into_any_element()
        });
        presentation::row(
            format!("live-queue-row-{turn}"),
            key,
            label(item),
            !item.attachments.is_empty(),
            !self.child && !item.notice,
            (!item.notice && item.kind == sailry_protocol::conversation::RunKind::Task)
                .then_some(edit.into_any_element())
                .into_iter()
                .chain(guide)
                .chain(withdraw)
                .chain((!self.child && !item.notice).then_some(send.into_any_element()))
                .chain([remove.into_any_element()]),
            cx,
        )
        .debug_selector(move || format!("queue-row-{turn}"))
        .when(
            !self.child && !item.notice && !self.blocked() && item.guidance.is_none(),
            |row| {
                row.on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                    .on_drop(cx.listener(move |panel, source: &Drag, window, cx| {
                        panel.reorder(source, turn, window, cx)
                    }))
            },
        )
        .into_any_element()
    }

    fn remove_button(&self, turn: TurnId, revision: u64, cx: &mut Context<Self>) -> Button {
        Button::new(format!("live-queue-delete-{turn}"))
            .rounded_full()
            .ghost()
            .small()
            .icon(IconName::Delete)
            .disabled(self.blocked())
            .debug_selector(move || format!("queue-delete-{turn}"))
            .tooltip(tr("queue_delete"))
            .accessibility_label(tr("queue_delete"))
            .on_click(cx.listener(move |panel, _, window, cx| {
                cx.stop_propagation();
                panel.execute(
                    Command::RemoveQueuedTurn {
                        turn,
                        expected_revision: revision,
                    },
                    window,
                    cx,
                );
            }))
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let changed = self
            .editing
            .as_ref()
            .is_some_and(|editing| self.changed(editing));
        let editor = self.editing.as_ref().map(|editing| {
            let turn = editing.turn;
            let attachments: Vec<_> = self
                .queue
                .items
                .iter()
                .filter(|item| item.turn == turn)
                .flat_map(|item| &item.attachments)
                .filter(|item| editing.attachments.contains(&item.id))
                .cloned()
                .collect();
            let reload = (changed && !self.child).then(|| {
                Button::new("queue-edit-reload")
                    .rounded_full()
                    .small()
                    .label(tr("queue_reload"))
                    .tooltip(tr("queue_reload_draft"))
                    .debug_selector(|| "queue-edit-reload".into())
                    .disabled(
                        self.blocked() || !self.queue.items.iter().any(|item| item.turn == turn),
                    )
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel.execute(Command::ReadQueuedTurn { turn }, window, cx)
                    }))
                    .into_any_element()
            });
            let cancel = Button::new("queue-edit-cancel")
                .rounded_full()
                .small()
                .label(tr(if self.child {
                    "close"
                } else {
                    "settings_cancel"
                }))
                .debug_selector(|| "queue-edit-cancel".into())
                .disabled(self.pending || self.retry.is_some())
                .on_click(cx.listener(|panel, _, window, cx| panel.cancel(window, cx)));
            let save = Button::new("queue-edit-save")
                .rounded_full()
                .small()
                .primary()
                .label(tr("queue_save"))
                .debug_selector(|| "queue-edit-save".into())
                .disabled(
                    self.blocked()
                        || changed
                        || (editing.input.read(cx).value().trim().is_empty()
                            && editing.attachments.is_empty()),
                )
                .on_click(cx.listener(|panel, _, window, cx| panel.save(window, cx)));
            let copy = self.child.then(|| {
                div()
                    .debug_selector(|| "queue-read-copy".into())
                    .child(
                        Clipboard::new("queue-read-copy")
                            .value(editing.input.read(cx).value())
                            .tooltip(tr("message_copy")),
                    )
                    .into_any_element()
            });
            let remove = self.child.then(|| {
                self.remove_button(turn, editing.revision, cx)
                    .disabled(self.blocked() || changed)
                    .into_any_element()
            });
            presentation::editor(
                gpui_kit::component::input::Textarea::new(&editing.input)
                    .readonly(
                        self.child
                            || self.pending
                                && self.retry.as_ref().is_some_and(|attempt| {
                                    matches!(
                                        attempt.request.command,
                                        Command::ReadQueuedTurn { .. }
                                    )
                                }),
                    )
                    .on_token_click(cx.listener(|panel, event, window, cx| {
                        if panel.blocked() {
                            return;
                        }
                        let Some(editing) = &panel.editing else {
                            return;
                        };
                        let input = editing.input.clone();
                        let references = editing.references.clone();
                        _ = panel.owner.update(cx, |view, cx| {
                            view.activate_input_token(&input, &references, event, window, cx)
                        });
                    })),
                self.child,
                reload
                    .into_iter()
                    .chain(copy)
                    .chain([cancel.into_any_element()])
                    .chain((!self.child).then_some(save.into_any_element()))
                    .chain(remove),
            )
            .when(!editing.attachments.is_empty(), |editor| {
                if attachments.len() == editing.attachments.len() {
                    editor.child(super::super::attachments::links(
                        &attachments,
                        &self.binding,
                        &self.images,
                        cx,
                    ))
                } else {
                    editor.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                rust_i18n::t!(
                                    "chat_attachment_count",
                                    count = editing.attachments.len()
                                )
                                .to_string(),
                            ),
                    )
                }
            })
        });
        presentation::tray(cx)
            .when_some(editor, |tray, editor| tray.child(editor))
            .when(self.editing.is_none(), |tray| {
                tray.child(
                    v_flex()
                        .id("queue-list")
                        .max_h(px(132.))
                        .overflow_y_scrollbar()
                        .children(self.items().map(|item| self.row(item, cx)))
                        .children(self.outgoing.clone().map(|text| {
                            let actions = [
                                ("edit", IconName::Replace, "queue_edit"),
                                ("guide", IconName::Redo2, "chat_queue_guide"),
                                ("send", IconName::ArrowUp, "queue_send_now"),
                                ("delete", IconName::Delete, "queue_delete"),
                            ]
                            .into_iter()
                            .filter(|(action, _, _)| *action != "guide" || self.recipient.is_some())
                            .map(|(action, icon, label)| {
                                Button::new(format!("queue-outgoing-{action}"))
                                    .rounded_full()
                                    .ghost()
                                    .small()
                                    .icon(icon)
                                    .disabled(true)
                                    .debug_selector(move || format!("queue-outgoing-{action}"))
                                    .tooltip(tr(label))
                                    .accessibility_label(tr(label))
                                    .into_any_element()
                            });
                            presentation::pending(
                                "live-queue-outgoing",
                                "outgoing".into(),
                                text,
                                self.outgoing_attached,
                                actions,
                                cx,
                            )
                            .debug_selector(|| "queue-outgoing".into())
                        })),
                )
            })
    }
}

pub(super) fn label(item: &Pending) -> SharedString {
    if item.notice {
        tr("background_notice_title")
    } else if item.kind == sailry_protocol::conversation::RunKind::Compaction {
        tr("chat_compact")
    } else if item.preview.is_empty() {
        item.attachments
            .iter()
            .map(|item| item.spec.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
            .into()
    } else {
        item.preview.clone().into()
    }
}

#[derive(Clone)]
pub(super) struct Drag {
    pub node: NodeId,
    pub session: SessionId,
    pub turn: TurnId,
    pub revision: u64,
    pub text: SharedString,
}

impl Render for Drag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        presentation::drag(self.text.clone(), cx)
    }
}
