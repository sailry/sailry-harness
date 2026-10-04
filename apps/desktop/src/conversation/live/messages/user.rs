//! Sent-message controls and an inline draft; Node atomically replaces history on submit.
use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Enter, Escape},
};
use sailry_protocol::conversation::Input;

pub(in crate::conversation::live) struct Editing {
    turn: TurnId,
    input: Entity<TextareaState>,
    command: Command,
    _events: Subscription,
}

impl View {
    pub(super) fn editing_turn(&self, turn: TurnId) -> bool {
        self.editing
            .as_ref()
            .is_some_and(|editing| editing.turn == turn)
    }
    fn can_edit(&self) -> bool {
        !self.readonly()
            && !self.busy()
            && self.connected()
            && self.history.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.page.queue.items.is_empty()
                    && snapshot
                        .page
                        .runs
                        .iter()
                        .all(|run| !frame::active(run.status))
            })
    }

    fn edit_message(&mut self, turn: TurnId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() || self.editing.is_some() {
            return;
        }
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let Some(snapshot) = self.history.snapshot.as_ref() else {
            return;
        };
        let Some(head) = snapshot.page.runs.last() else {
            return;
        };
        let message = sent_input(&snapshot.page, turn);
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(tr("composer"))
                .auto_grow(1, 12)
                .submit_on_enter(true)
        });
        let Ok(content) = references::inline::content(&message.text, &message.references) else {
            self.error = Some("reference_stale");
            cx.notify();
            return;
        };
        input.update(cx, |input, cx| input.set_value(content, window, cx));
        let events = cx.subscribe_in(&input, window, |view, _, event, window, cx| match event {
            InputEvent::PressEnter { shift: false, .. } => view.regenerate(window, cx),
            InputEvent::Blur => view.cancel_edit(cx),
            _ => {}
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.editing = Some(Editing {
            turn,
            input,
            _events: events,
            command: Command::ReplaceTurn {
                session: session.id,
                turn,
                expected_head: head.turn,
                expected_history_revision: snapshot.page.revision,
                expected_revision: session.revision,
                message,
            },
        });
        self.error = None;
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if !self.busy() {
            self.editing = None;
            self.error = None;
            cx.notify();
        }
    }

    fn regenerate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        let Some(editing) = self.editing.as_ref() else {
            return;
        };
        let mut command = editing.command.clone();
        let Command::ReplaceTurn { message, .. } = &mut command else {
            unreachable!()
        };
        if editing.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        message.text = editing.input.read(cx).value().to_string();
        message.references = references::inline::active_content(
            &editing.input.read(cx).content(),
            &message.references,
        );
        if message.is_empty() {
            return;
        }
        self.execute(command, actions::Action::Replace, window, cx);
    }

    pub(in crate::conversation::live) fn user_text(
        &self,
        turn: TurnId,
        text: String,
        references: &[sailry_protocol::conversation::reference::Reference],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(editing) = self.editing.as_ref().filter(|editing| editing.turn == turn) else {
            let links = references::inline::links(&text, references);
            let references = references.to_vec();
            let owner = cx.entity().downgrade();
            let selectable = gpui_kit::base::SelectableText::new(format!("{turn}-prompt"), text)
                .links(links, cx.theme().link, move |link, window, cx| {
                    if let Some(reference) = references
                        .iter()
                        .find(|reference| references::inline::marker(reference) == link.id)
                    {
                        _ = owner.update(cx, |view, cx| {
                            view.open_reference(reference.clone(), Some(turn), window, cx)
                        });
                    }
                });
            return div()
                .debug_selector(move || format!("live-user-text-{turn}"))
                .min_w_0()
                .text_sm()
                .line_height(relative(1.6))
                .child(selectable)
                .into_any_element();
        };
        let Command::ReplaceTurn { message, .. } = &editing.command else {
            unreachable!()
        };
        let input = editing.input.clone();
        let targets = message.references.clone();
        div()
            .id("message-edit")
            .w_full()
            .debug_selector(|| "message-edit-input".into())
            .on_action(cx.listener(|view, _: &Escape, _, cx| view.cancel_edit(cx)))
            .on_action(|action: &Enter, _, cx| {
                if action.shift {
                    cx.propagate();
                }
            })
            .child(
                references::inline::textarea(&editing.input, &message.references)
                    .appearance(false)
                    .bordered(false)
                    .p_0()
                    .readonly(self.busy())
                    .aria_label(tr("message_edit"))
                    .on_token_click(cx.listener(move |view, event, window, cx| {
                        view.activate_input_token(&input, &targets, event, window, cx)
                    })),
            )
            .into_any_element()
    }

    pub(in crate::conversation::live) fn user_footer(
        &self,
        turn: TurnId,
        text: &str,
        sent_ms: Option<i64>,
        cx: &mut Context<Self>,
    ) -> MessageFooter {
        MessageFooter::new().content_inset(false).w_full().child(
            h_flex()
                .id(format!("user-footer-{turn}"))
                .w_full()
                .justify_end()
                .group(format!("user-footer-{turn}"))
                .gap_2()
                .debug_selector(move || format!("live-user-footer-{turn}"))
                .when_some(sent_ms.and_then(frame::at), |row, time| {
                    let full = sent_ms
                        .and_then(frame::full_at)
                        .unwrap_or_else(|| time.clone());
                    row.child(
                        h_flex()
                            .id(format!("user-time-{turn}"))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .opacity(0.)
                            .group_hover(format!("user-footer-{turn}"), |style| style.opacity(1.))
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(full.clone())
                                    .build(window, cx)
                            })
                            .child(time),
                    )
                })
                .when(!text.is_empty(), |row| {
                    row.child(
                        h_flex()
                            .debug_selector(move || format!("live-user-copy-{turn}"))
                            .child(
                                Clipboard::new(format!("user-copy-{turn}"))
                                    .value(text.to_owned())
                                    .tooltip(tr("message_copy")),
                            ),
                    )
                })
                .when(!self.readonly(), |row| {
                    row.child(
                        Button::new(format!("user-edit-{turn}"))
                            .ghost()
                            .small()
                            .icon(Icon::default().path("icons/reicon/writing.svg"))
                            .disabled(!self.can_edit() || self.editing.is_some())
                            .tooltip(tr("message_edit"))
                            .accessibility_label(tr("message_edit"))
                            .debug_selector(move || format!("live-user-edit-{turn}"))
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.edit_message(turn, window, cx)
                            })),
                    )
                }),
        )
    }
}

#[cfg(test)]
mod tests;

pub(in crate::conversation::live) fn sent_input(
    page: &sailry_protocol::conversation::Page,
    turn: TurnId,
) -> Input {
    let mut message = Input::default();
    for entry in page
        .entries
        .iter()
        .filter(|entry| entry.turn == turn && entry.author == "user")
    {
        for part in &entry.parts {
            match part {
                Part::Text(text) => message.text.push_str(text),
                Part::Attachment(attachment) => message.attachments.push(attachment.id),
                Part::Reference(reference) => message.references.push(reference.clone()),
                _ => {}
            }
        }
    }
    message
}
