use super::*;
use sailry_protocol::conversation::Input;
use sailry_protocol::{ErrorCode, Fault};

#[derive(Clone)]
pub(super) enum Action {
    Create(Input),
    Send(Input),
    Resend(TurnId),
    Configure,
    Move(WorktreeId),
    ForkAt(WorktreeId),
    Compact,
    Stop,
    Fork(TurnId),
    Rewind(TurnId),
    Replace,
}

#[derive(Clone)]
pub(super) struct Attempt {
    request: Request,
    client: Arc<Client>,
    action: Action,
}

impl Attempt {
    pub(super) fn is_submission(&self) -> bool {
        matches!(
            self.action,
            Action::Send(_) | Action::Resend(_) | Action::Replace
        )
    }

    pub(super) fn is_configure(&self) -> bool {
        matches!(self.action, Action::Configure)
    }

    pub(super) fn is_resend(&self, turn: TurnId) -> bool {
        matches!(self.action, Action::Resend(target) if target == turn)
    }

    pub(super) fn is_fork(&self, turn: TurnId) -> bool {
        matches!(self.action, Action::Fork(target) if target == turn)
    }
    pub(super) fn is_rewind(&self, turn: TurnId) -> bool {
        matches!(self.action, Action::Rewind(target) if target == turn)
    }
}

impl View {
    pub(super) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.update(cx, |input, cx| {
            input.marked_text_range(window, cx).is_some()
        }) {
            return;
        }
        if let Some((mode, _)) = references::commands::leading_mode(&self.input.read(cx).value())
            && mode != self.current_mode()
        {
            self.select_mode(mode, window, cx);
            return;
        }
        if self.readonly()
            || self.busy()
            || !self.connected()
            || !self.configured()
            || !self.attachments.sendable()
            || (self.binding.project.is_some() && self.binding.worktree.is_none())
        {
            return;
        }
        let references = self.active_references(cx);
        if !self.references_valid(&references) || !self.command_valid(cx) {
            self.error = Some("reference_stale");
            cx.notify();
            return;
        }
        let message = Input {
            text: self.input.read(cx).value().to_string(),
            attachments: self.attachments.ids(),
            references,
        };
        if self.normalized_input(message.clone()).is_empty() && !self.has_attachments() {
            return;
        }
        self.references.dismiss();
        self.show_outgoing(message.clone(), cx);
        if (self.session.is_none() && self.binding.worktree.is_none())
            || !self.prepare_attachments(message.clone(), window, cx)
        {
            self.submit(message, window, cx);
        }
    }

    pub(super) fn submit(&mut self, message: Input, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = &self.session {
            self.execute(
                Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: self.normalized_input(message.clone()),
                },
                Action::Send(message),
                window,
                cx,
            );
        } else {
            self.create_session(Action::Create(message), window, cx);
        }
    }

    pub(super) fn create_session(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let config = self.config.clone().map(|mut config| {
            config.resource = self.resource;
            config.assistant = self.assistant.clone();
            config
        });
        let command = if self.config_owner != self.binding.client.target() {
            let config = config.expect("configuration was checked");
            let Some(provider_revision) = self
                .providers()
                .find(|provider| provider.id == config.provider)
                .map(|provider| provider.revision)
            else {
                return;
            };
            Command::CreateSessionAt {
                target: self.binding.client.target(),
                project: self.binding.project,
                worktree: self.binding.worktree,
                config: Box::new(config),
                provider_revision,
            }
        } else {
            Command::CreateSession {
                project: self.binding.project,
                worktree: self.binding.worktree,
                config,
            }
        };
        self.execute(command, action, window, cx);
    }

    pub(super) fn configure(
        &mut self,
        mut config: SessionConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly() || self.busy() {
            return;
        }
        config.resource = self.resource;
        config.assistant = self.assistant.clone();
        if let Some(session) = &self.session {
            if session.config == config {
                return;
            }
            self.execute(
                Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config,
                },
                Action::Configure,
                window,
                cx,
            );
        } else {
            self.remember_config(&config, cx);
            self.config = Some(config);
            cx.notify();
        }
    }

    pub(super) fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || !self.connected() {
            return;
        }
        if let Some(turn) = self.active() {
            self.execute(Command::StopTurn { turn }, Action::Stop, window, cx);
        }
    }

    pub(super) fn execute(
        &mut self,
        command: Command,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let client = if matches!(command, Command::CreateSessionAt { .. }) {
            self.binding.defaults.clone()
        } else {
            self.binding.client.clone()
        };
        let request = client.prepare(command);
        if matches!(&action, Action::Send(_))
            && let Some(outgoing) = &mut self.outgoing
        {
            outgoing.request = Some(request.id);
        }
        self.retry = Some(Attempt {
            request,
            client,
            action,
        });
        self.retry(window, cx);
    }

    pub(super) fn retry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let Some(attempt) = self.retry.clone() else {
            return;
        };
        let client = attempt.client.clone();
        let submission = attempt.is_submission();
        self.pending = true;
        if submission {
            self.scroller.update(cx, |scroller, cx| {
                scroller.remeasure_items(0..self.rows.len(), cx)
            });
        }
        self.error = None;
        let request_id = attempt.request.id;
        let job = self
            .binding
            .runtime
            .spawn(async move { client.execute(attempt.request).await });
        self.action = Some(cx.spawn_in(window, async move |view, cx| {
            let result = job.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.pending = false;
                match result {
                    Ok(Ok(Output::TurnReplaced { history, .. }))
                        if matches!(attempt.action, Action::Replace) =>
                    {
                        view.retry = None;
                        view.editing = None;
                        view.backup = Some(history.backup);
                    }
                    Ok(Ok(Output::Rewound(result)))
                        if matches!(attempt.action, Action::Rewind(_)) =>
                    {
                        let Action::Rewind(through) = attempt.action else {
                            unreachable!()
                        };
                        if Some(result.session) == view.session()
                            && result.through == Some(through)
                            && result.backup.id != result.session
                            && result.backup.project == view.binding.project
                            && Some(result.backup.worktree) == view.binding.worktree
                            && result
                                .backup
                                .fork
                                .as_ref()
                                .is_some_and(|fork| fork.session == result.session)
                        {
                            view.retry = None;
                            view.backup = Some(result.backup);
                        } else {
                            view.error = Some("chat_action_unknown");
                        }
                    }
                    Ok(Ok(Output::Session(session)))
                        if matches!(attempt.action, Action::Move(_) | Action::ForkAt(_)) =>
                    {
                        let (worktree, fork) = match attempt.action {
                            Action::Move(tree) => (tree, false),
                            Action::ForkAt(tree) => (tree, true),
                            _ => unreachable!(),
                        };
                        if session.project == view.binding.project
                            && session.worktree == worktree
                            && (Some(session.id) == view.session()) != fork
                        {
                            view.retry = None;
                            if fork {
                                cx.emit(Event::Forked(Box::new(session)));
                            } else {
                                let current = view
                                    .node
                                    .snapshot
                                    .as_ref()
                                    .and_then(|snapshot| {
                                        snapshot.sessions.iter().find(|current| {
                                            current.id == session.id
                                                && current.revision > session.revision
                                        })
                                    })
                                    .cloned()
                                    .unwrap_or(session);
                                view.session = Some(current);
                                view.sync_location(cx);
                                view.refresh_config(cx);
                            }
                        } else {
                            view.error = Some("chat_action_unknown");
                        }
                    }
                    Ok(Ok(Output::Session(session)))
                        if matches!(attempt.action, Action::Fork(_)) =>
                    {
                        let Action::Fork(through) = attempt.action else {
                            unreachable!()
                        };
                        if session.project == view.binding.project
                            && Some(session.worktree) == view.binding.worktree
                            && session.fork.as_ref().is_some_and(|fork| {
                                Some(fork.session) == view.session() && fork.through == through
                            })
                        {
                            view.retry = None;
                            cx.emit(Event::Forked(Box::new(session)));
                        } else {
                            view.error = Some("chat_action_unknown");
                        }
                    }
                    Ok(Ok(Output::Session(session))) => {
                        view.retry = None;
                        let current = view
                            .node
                            .snapshot
                            .as_ref()
                            .and_then(|snapshot| {
                                snapshot.sessions.iter().find(|current| {
                                    current.id == session.id && current.revision > session.revision
                                })
                            })
                            .unwrap_or(&session);
                        view.config = Some(current.config.clone());
                        view.session = Some(current.clone());
                        view.sync_location(cx);
                        view.refresh_config(cx);
                        if matches!(attempt.action, Action::Configure) {
                            view.remember_config(&session.config, cx);
                        }
                        if matches!(attempt.action, Action::Create(_)) {
                            view.defaults_stop.cancel();
                            view._defaults = None;
                            view.defaults = NodeView::default();
                            let (subscription, older) =
                                watch::history(&view.binding, session.id, &view.stop, cx);
                            view.subscription = Some(subscription);
                            view.older = Some(older);
                            cx.emit(Event::Created(Box::new(session.clone())));
                            let Action::Create(message) = attempt.action else {
                                unreachable!()
                            };
                            if !view.references_valid(&message.references) {
                                view.error = Some("reference_stale");
                                view.outgoing = None;
                                view.sync_rows(cx);
                                cx.notify();
                                return;
                            }
                            if view.prepare_attachments(message.clone(), window, cx) {
                                return;
                            }
                            view.execute(
                                Command::SubmitTurn {
                                    session: session.id,
                                    expected_revision: session.revision,
                                    message: view.normalized_input(message.clone()),
                                },
                                Action::Send(message),
                                window,
                                cx,
                            );
                        }
                    }
                    Ok(Ok(Output::QueuedTurn(turn))) => {
                        if let Some(outgoing) = &mut view.outgoing
                            && outgoing.request == Some(request_id)
                        {
                            outgoing.request = Some(turn.request);
                            outgoing.turn = Some(turn.id);
                        }
                        view.retry = None;
                        if let Action::Send(message) = attempt.action {
                            view.attachments.sent(&message.attachments);
                            let unchanged = view.input.read(cx).value().as_ref() == message.text;
                            if unchanged {
                                view.references.selected.clear();
                                view.references.command_keys.clear();
                                view.input
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                            }
                        }
                        view.scroller
                            .update(cx, |scroller, cx| scroller.scroll_to_end(cx));
                    }
                    Ok(Ok(Output::Run(_))) => view.retry = None,
                    Ok(Ok(Output::PluginResult(result)))
                        if matches!(attempt.action, Action::Send(_)) =>
                    {
                        view.retry = None;
                        view.outgoing = None;
                        if let Action::Send(message) = attempt.action {
                            if view.input.read(cx).value().as_ref() == message.text {
                                view.references.command_keys.clear();
                                view.input
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                            }
                            if let Some(intent) =
                                result.get("ui_intent").and_then(|value| value.as_str())
                            {
                                view.contribution_intent(intent, true.into(), window, cx)
                                    .detach();
                            }
                        }
                    }
                    Ok(Err(error)) => {
                        view.error = Some(
                            if matches!(attempt.action, Action::Move(_) | Action::ForkAt(_)) {
                                match error.code {
                                    ErrorCode::Busy => "location_busy",
                                    ErrorCode::NotFound => "worktree_unavailable",
                                    ErrorCode::RevisionConflict | ErrorCode::Conflict => {
                                        "location_changed"
                                    }
                                    _ => error_key(&error),
                                }
                            } else if matches!(attempt.action, Action::Rewind(_) | Action::Replace)
                            {
                                match error.code {
                                    ErrorCode::RevisionConflict
                                    | ErrorCode::Conflict
                                    | ErrorCode::WrongTarget => "chat_history_changed",
                                    ErrorCode::Busy => "chat_rewind_busy",
                                    _ => error_key(&error),
                                }
                            } else if error.code == ErrorCode::InvalidRequest
                                && matches!(&attempt.action,
                            Action::Send(message) if !message.attachments.is_empty())
                            {
                                "chat_attachment_rejected"
                            } else {
                                error_key(&error)
                            },
                        );
                        if !(matches!(
                            error.code,
                            ErrorCode::OutcomeUnknown | ErrorCode::Unavailable
                        ) || error.code == ErrorCode::Busy
                            && !matches!(
                                attempt.action,
                                Action::Fork(_)
                                    | Action::ForkAt(_)
                                    | Action::Move(_)
                                    | Action::Rewind(_)
                                    | Action::Replace
                            ))
                        {
                            view.retry = None;
                            if matches!(attempt.action, Action::Send(_) | Action::Create(_)) {
                                view.outgoing = None;
                            }
                        }
                    }
                    _ => view.error = Some("chat_action_unknown"),
                }
                view.sync_rows(cx);
                if submission {
                    view.scroller.update(cx, |scroller, cx| {
                        scroller.remeasure_items(0..view.rows.len(), cx)
                    });
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

pub(super) fn error_key(error: &Fault) -> &'static str {
    match error.code {
        ErrorCode::RevisionConflict | ErrorCode::Conflict => "chat_config_changed",
        ErrorCode::NotConfigured | ErrorCode::PermissionDenied => "chat_config_unavailable",
        ErrorCode::OutcomeUnknown | ErrorCode::Unavailable => "chat_action_unknown",
        _ => "chat_action_failed",
    }
}
