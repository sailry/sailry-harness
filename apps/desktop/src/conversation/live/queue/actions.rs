use super::*;

#[derive(Clone)]
pub(super) struct Attempt {
    pub(super) request: Request,
}

impl Panel {
    pub(super) fn execute(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.blocked() {
            return;
        }
        self.retry = Some(Attempt {
            request: self.binding.client.prepare(command),
        });
        self.retry(window, cx);
    }

    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.connected {
            return;
        }
        let Some(attempt) = self.retry.clone() else {
            return;
        };
        self.pending = true;
        self.error = None;
        let client = self.binding.client.clone();
        let request = attempt.request.clone();
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(request).await });
        self.action = Some(cx.spawn_in(window, async move |panel, cx| {
            let result = job.await;
            let _ = panel.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok(Ok(Output::QueuedMessage(message))) => {
                        panel.retry = None;
                        let input = cx.new(|cx| {
                            TextareaState::new(window, cx)
                                .placeholder(tr("composer"))
                                .auto_grow(1, 3)
                                .submit_on_enter(true)
                        });
                        let Ok(content) = super::super::references::inline::content(
                            &message.message.text,
                            &message.message.references,
                        ) else {
                            panel.error = Some("reference_stale");
                            cx.notify();
                            return;
                        };
                        input.update(cx, |input, cx| input.set_value(content, window, cx));
                        cx.subscribe_in(&input, window, |panel, _, event, window, cx| {
                            if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                                panel.save(window, cx);
                            }
                            cx.notify();
                        })
                        .detach();
                        cx.observe(&input, |_, _, cx| cx.notify()).detach();
                        if panel
                            .popover
                            .as_ref()
                            .and_then(WeakEntity::upgrade)
                            .is_some_and(|popover| popover.read(cx).is_open())
                        {
                            input.update(cx, |input, cx| input.focus(window, cx));
                        }
                        panel.editing = Some(Editing {
                            turn: message.turn.id,
                            revision: message.revision,
                            input,
                            attachments: message.message.attachments,
                            references: message.message.references,
                        });
                    }
                    Ok(Ok(Output::Queue(update))) => {
                        panel.retry = None;
                        if let Command::EditQueuedTurn { turn, message, .. } =
                            &attempt.request.command
                            && let Some(editing) = &mut panel.editing
                            && editing.turn == *turn
                        {
                            if editing.input.read(cx).value().as_ref() == message.text {
                                panel.editing = None;
                                panel.restore_focus(window, cx);
                            } else if let Some(item) =
                                update.queue.items.iter().find(|item| item.turn == *turn)
                            {
                                // New typing belongs to the next edit, based on the acknowledged revision.
                                editing.revision = item.revision;
                            }
                        }
                    }
                    Ok(Err(error)) => {
                        panel.error = Some(match error.code {
                            ErrorCode::RevisionConflict
                            | ErrorCode::Conflict
                            | ErrorCode::NotFound => {
                                if panel.editing.is_some() {
                                    "queue_draft_changed"
                                } else {
                                    "queue_changed"
                                }
                            }
                            _ => super::super::actions::error_key(&error),
                        });
                        if !matches!(
                            error.code,
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable | ErrorCode::Busy
                        ) {
                            panel.retry = None;
                        }
                    }
                    _ => panel.error = Some("chat_action_unknown"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = &self.editing else { return };
        if self.changed(editing)
            || editing.input.update(cx, |input, cx| {
                input.marked_text_range(window, cx).is_some()
            })
        {
            return;
        }
        let message = editing.input.read(cx).value().to_string();
        let references = super::super::references::inline::active_content(
            &editing.input.read(cx).content(),
            &editing.references,
        );
        if message.trim().is_empty() && editing.attachments.is_empty() && references.is_empty() {
            return;
        }
        self.execute(
            Command::EditQueuedTurn {
                turn: editing.turn,
                expected_revision: editing.revision,
                message: sailry_protocol::conversation::Input {
                    text: message,
                    attachments: editing.attachments.clone(),
                    references,
                },
            },
            window,
            cx,
        );
    }

    pub(super) fn reorder(
        &mut self,
        source: &super::render::Drag,
        target: TurnId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session != Some(source.session)
            || source.node != self.binding.client.target()
            || source.turn == target
        {
            return;
        }
        let from = self
            .queue
            .items
            .iter()
            .position(|item| item.turn == source.turn);
        let to = self.queue.items.iter().position(|item| item.turn == target);
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        let before = if from < to {
            self.queue.items.get(to + 1).map(|item| item.turn)
        } else {
            Some(target)
        };
        self.execute(
            Command::MoveQueuedTurn {
                session: source.session,
                expected_revision: source.revision,
                turn: source.turn,
                before,
            },
            window,
            cx,
        );
    }
}
