use super::*;
use crate::conversation::queue::presentation;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    popover::Popover,
    scroll::ScrollableElement,
};
use gpui_kit::prelude::FluentBuilder as _;

impl View {
    pub(in crate::conversation::live) fn queue_bar(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.queue.update(cx, |panel, cx| panel.sync(self, cx));
        let panel = self.queue.read(cx);
        if panel.queue.items.is_empty()
            && panel.editing.is_none()
            && panel.retry.is_none()
            && panel.error.is_none()
        {
            return None;
        }
        let paused = panel.queue.paused;
        let panel = self.queue.clone();
        Some(
            Popover::new("live-queue-popover")
                .anchor(Anchor::BottomLeft)
                .bottom_2()
                .p_2()
                .trigger(
                    Button::new("live-queue")
                        .outline()
                        .small()
                        .rounded_full()
                        .icon(IconName::Menu)
                        .font_normal()
                        .text_color(cx.theme().muted_foreground)
                        .label(
                            rust_i18n::t!(
                                "queue_count",
                                count = self.queue.read(cx).queue.items.len()
                            )
                            .to_string(),
                        )
                        .when(paused, |button| button.tooltip(tr("queue_paused")))
                        .debug_selector(|| "live-queue".into()),
                )
                .content(move |_, _, cx| {
                    let popover = cx.entity().downgrade();
                    panel.update(cx, |panel, _| panel.popover = Some(popover));
                    panel.clone()
                })
                .into_any_element(),
        )
    }
}

impl Panel {
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
        let edit = Button::new(format!("live-queue-edit-{turn}"))
            .rounded_full()
            .ghost()
            .small()
            .icon(IconName::Replace)
            .disabled(self.blocked())
            .debug_selector(move || format!("queue-edit-{turn}"))
            .tooltip(tr("queue_edit"))
            .accessibility_label(tr("queue_edit"))
            .on_click(cx.listener(move |panel, _, window, cx| {
                cx.stop_propagation();
                panel.execute(Command::ReadQueuedTurn { turn }, window, cx);
            }));
        let send = Button::new(format!("live-queue-send-{turn}"))
            .rounded_full()
            .ghost()
            .small()
            .icon(IconName::ArrowUp)
            .disabled(self.blocked())
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
        let remove = Button::new(format!("live-queue-delete-{turn}"))
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
            }));
        presentation::row(
            format!("live-queue-row-{turn}"),
            key,
            label(item),
            !item.attachments.is_empty(),
            (item.kind == sailry_protocol::conversation::RunKind::Task)
                .then_some(edit.into_any_element())
                .into_iter()
                .chain([send.into_any_element(), remove.into_any_element()]),
            cx,
        )
        .debug_selector(move || format!("queue-row-{turn}"))
        .when(!self.blocked(), |row| {
            row.on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .on_drop(cx.listener(move |panel, source: &Drag, window, cx| {
                    panel.reorder(source, turn, window, cx)
                }))
        })
        .into_any_element()
    }
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            let reload = changed.then(|| {
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
                .label(tr("settings_cancel"))
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
            presentation::editor(
                super::super::references::inline::textarea(&editing.input, &editing.references)
                    .readonly(
                        self.pending
                            && self.retry.as_ref().is_some_and(|attempt| {
                                matches!(attempt.request.command, Command::ReadQueuedTurn { .. })
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
                reload
                    .into_iter()
                    .chain([cancel.into_any_element(), save.into_any_element()]),
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
        presentation::panel(window)
            .when_some(editor, |panel, editor| panel.child(editor))
            .when(self.editing.is_none(), |panel| {
                panel.child(
                    v_flex()
                        .id("queue-list")
                        .flex_grow(1.)
                        .p_1()
                        .overflow_y_scrollbar()
                        .children(self.queue.items.iter().map(|item| self.row(item, cx))),
                )
            })
    }
}

pub(super) fn label(item: &Pending) -> SharedString {
    if item.kind == sailry_protocol::conversation::RunKind::Compaction {
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
