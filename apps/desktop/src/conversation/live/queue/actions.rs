use super::*;

#[derive(Clone)]
pub(super) struct Attempt {
    pub(super) request: Request,
    message: Option<sailry_protocol::conversation::Input>,
    pub(super) extract: bool,
    attachments: Vec<sailry_protocol::attachment::Attachment>,
}

impl Panel {
    pub(super) fn edit(
        &mut self,
        turn: TurnId,
        revision: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.child {
            self.execute(Command::ReadQueuedTurn { turn }, window, cx);
            return;
        }
        if self.blocked() {
            return;
        }
        let available = self.owner.update(cx, |view, cx| {
            !view.busy()
                && view.outgoing.is_none()
                && !view.has_attachments()
                && view.input.read(cx).value().is_empty()
                && view.active_references(cx).is_empty()
        });
        if !matches!(available, Ok(true)) {
            self.error = Some("queue_draft_present");
            cx.notify();
            return;
        }
        let Some(item) =
            self.queue.items.iter().find(|item| {
                item.turn == turn && item.revision == revision && item.guidance.is_none()
            })
        else {
            self.error = Some("queue_changed");
            cx.notify();
            return;
        };
        self.retry = Some(Attempt {
            request: self.binding.client.prepare(Command::RemoveQueuedTurn {
                turn,
                expected_revision: revision,
            }),
            message: None,
            extract: true,
            attachments: item.attachments.clone(),
        });
        self.sync_editing(window, cx);
        self.retry(window, cx);
    }

    pub(super) fn execute(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.blocked() || !self.allows(&command) {
            return;
        }
        let attempt = Attempt {
            request: self.binding.client.prepare(command),
            message: None,
            extract: false,
            attachments: Vec::new(),
        };
        if let Command::GuideQueuedTurn {
            turn, recipient, ..
        } = attempt.request.command
            && let Some(item) = self.queue.items.iter().find(|item| item.turn == turn)
        {
            let input = sailry_protocol::conversation::Input {
                text: item.preview.clone(),
                attachments: item
                    .attachments
                    .iter()
                    .map(|attachment| attachment.id)
                    .collect(),
                references: Vec::new(),
            };
            let attachments = item.attachments.clone();
            let request = attempt.request.id;
            let shown = self.owner.update(cx, |view, cx| {
                view.show_guidance(turn, recipient, request, input, attachments, cx)
            });
            if !matches!(shown, Ok(true)) {
                return;
            }
        }
        self.retry = Some(attempt);
        self.sync_guidance(cx);
        self.retry(window, cx);
    }

    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.connected {
            return;
        }
        let Some(attempt) = self.retry.clone() else {
            return;
        };
        if !self.allows(&attempt.request.command) {
            return;
        }
        self.pending = true;
        self.error = None;
        let client = self.binding.client.clone();
        let request = attempt.request.clone();
        let runtime = self.binding.runtime.clone();
        let read_client = client.clone();
        let read_request = request.clone();
        let captured = attempt.message.clone();
        let extract = attempt.extract;
        let read = runtime.spawn(async move {
            if captured.is_some() {
                return Ok(captured);
            }
            let (turn, expected_revision) = match read_request.command {
                Command::GuideQueuedTurn {
                    turn,
                    expected_revision,
                    ..
                } => (turn, expected_revision),
                Command::RemoveQueuedTurn {
                    turn,
                    expected_revision,
                } if extract => (turn, expected_revision),
                _ => return Ok(None),
            };
            match read_client
                .execute(read_client.prepare(Command::ReadQueuedTurn { turn }))
                .await?
            {
                Output::QueuedMessage(message)
                    if message.turn.id == turn && message.revision == expected_revision =>
                {
                    Ok(Some(message.message))
                }
                Output::QueuedMessage(_) => Err(sailry_protocol::Fault::new(
                    ErrorCode::RevisionConflict,
                    "queued message changed before the operation",
                )),
                _ => Err(sailry_protocol::Fault::invalid("queued message expected")),
            }
        });
        self.action = Some(cx.spawn_in(window, async move |panel, cx| {
            let prepared = read.await;
            let result = match prepared {
                Ok(Ok(message)) => {
                    if let Some(message) = message {
                        let valid = panel.update_in(cx, |panel, _, cx| {
                            if extract && super::super::references::inline::content(&message.text, &message.references).is_err() { return false; }
                            if let Some(attempt) = &mut panel.retry { attempt.message = Some(message.clone()); }
                            let _ = panel.owner.update(cx, |view, cx| {
                                if let Some(outgoing) = &mut view.outgoing
                                    && outgoing.request == Some(attempt.request.id)
                                {
                                    outgoing.input = message.clone();
                                }
                                view.sync_rows(cx);
                                cx.notify();
                            });
                            true
                        });
                        if !matches!(valid, Ok(true)) {
                            let _ = panel.update_in(cx, |panel, window, cx| {
                                panel.pending = false;
                                panel.retry = None;
                                panel.error = Some("reference_stale");
                                panel.sync_editing(window, cx);
                                cx.notify();
                            });
                            return;
                        }
                    }
                    runtime.spawn(async move {client.execute(request).await}).await
                }
                Ok(Err(error)) => Ok(Err(error)),
                Err(error) => Err(error),
            };
            let _ = panel.update_in(cx, |panel, window, cx| {
                panel.pending = false;
                match result {
                    Ok(Ok(Output::QueuedMessage(message))) => {
                        panel.retry = None;
                        let input = cx.new(|cx| {
                            TextareaState::new(window, cx)
                                .placeholder(tr("composer"))
                                .auto_grow(1, 3)
                                .submit_on_enter(!panel.child)
                        });
                        let Ok(content) = super::super::references::inline::content(
                            &message.message.text,
                            &message.message.references,
                        ) else {
                            panel.error = Some("reference_stale");
                            cx.notify();
                            return;
                        };
                        input.update(cx, |input, cx| {
                            input.set_value(content, window, cx);
                            input.set_readonly(panel.child, cx);
                        });
                        if !panel.child {
                            cx.subscribe_in(&input, window, |panel, _, event, window, cx| {
                                if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                                    panel.save(window, cx);
                                }
                                cx.notify();
                            })
                            .detach();
                        }
                        cx.observe(&input, |_, _, cx| cx.notify()).detach();
                        input.update(cx, |input, cx| input.focus(window, cx));
                        panel.editing = Some(Editing {
                            turn: message.turn.id,
                            revision: message.revision,
                            input,
                            attachments: message.message.attachments,
                            references: message.message.references,
                        });
                    }
                    Ok(Ok(Output::Queue(update))) => {
                        let extracted = panel.retry.as_ref().filter(|attempt| attempt.extract).and_then(|attempt| attempt.message.clone());
                        panel.retry = None;
                        if let Some(message) = extracted {
                            let content = super::super::references::inline::content(&message.text, &message.references).expect("captured references were validated before removal");
                            let attachments = attempt.attachments.clone();
                            _ = panel.owner.update(cx, |view, cx| {
                                view.references.selected = message.references;
                                view.input.update(cx, |input, cx| input.set_value(content, window, cx));
                                view.restore_attachments(attachments, cx);
                                cx.notify();
                            });
                        }
                        if let Command::GuideQueuedTurn {recipient,..} = attempt.request.command
                            && let Some(item) = update.queue.items.iter().find(|item| item.guidance == Some(recipient))
                        {
                            let _ = panel.owner.update(cx, |view,cx| {
                                if let Some(outgoing) = &mut view.outgoing
                                    && outgoing.request == Some(attempt.request.id) {
                                        outgoing.turn=Some(item.turn);
                                        if let Some(guidance) = &mut outgoing.guidance {guidance.revision=Some(update.queue.revision);}
                                    }
                                view.sync_rows(cx);
                                cx.notify();
                            });
                        }
                        if panel.child
                            && matches!(&attempt.request.command, Command::RemoveQueuedTurn { turn, .. }
                                if panel.editing.as_ref().is_some_and(|editing| editing.turn == *turn))
                        {
                            panel.editing = None;
                            panel.restore_focus(window, cx);
                        }
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
                                if panel.editing.is_some() && !panel.child {
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
                            let _ = panel.owner.update(cx, |view,cx| {
                                if view.outgoing.as_ref().is_some_and(|message|message.request==Some(attempt.request.id)) {
                                    view.outgoing=None;
                                    view.sync_rows(cx);
                                    cx.notify();
                                }
                            });
                        }
                    }
                    _ => panel.error = Some("chat_action_unknown"),
                }
                panel.sync_guidance(cx);
                panel.sync_editing(window, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.child {
            return;
        }
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
        if self.child
            || self.session != Some(source.session)
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
