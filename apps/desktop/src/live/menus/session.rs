use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};
use sailry_protocol::{ErrorCode, Request, Session};

pub(super) fn items(archived: bool) -> Vec<(Command, &'static str)> {
    vec![
        (Command::Open, "workspace_open_session"),
        if archived {
            (Command::Unarchive, "session_unarchive")
        } else {
            (Command::Archive, "workspace_archive")
        },
        (Command::Delete, "session_delete"),
    ]
}

impl Shell {
    pub(crate) fn live_session_archive(
        &self,
        node: NodeId,
        session: &Session,
        cx: &mut Context<Self>,
    ) -> Button {
        let id = session.id;
        let archived = session.archived;
        Button::new(format!("session-archive-{id}"))
            .text()
            .text_color(cx.theme().muted_foreground)
            .xsmall()
            .size_5()
            .icon(IconName::Inbox)
            .accessibility_label(tr(if archived {
                "session_unarchive"
            } else {
                "workspace_archive"
            }))
            .debug_selector(move || format!("session-archive-{id}"))
            .on_click(cx.listener(move |shell, _, window, cx| {
                cx.stop_propagation();
                shell.live_resource_action(
                    &Dispatch {
                        node,
                        target: Target::Session(id),
                        command: if archived {
                            Command::Unarchive
                        } else {
                            Command::Archive
                        },
                    },
                    window,
                    cx,
                );
            }))
    }

    pub(crate) fn live_session_actions(
        &self,
        node: NodeId,
        session: &Session,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = session.id;
        let archived = session.archived;
        let owner = cx.weak_entity();
        h_flex()
            .gap_0p5()
            .child(self.live_session_archive(node, session, cx))
            .child(
                Button::new(format!("session-more-{id}"))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Ellipsis)
                    .accessibility_label(tr("more"))
                    .debug_selector(move || format!("session-more-{id}"))
                    .dropdown_menu(move |menu, _, _| {
                        items(archived)
                            .into_iter()
                            .fold(menu, |menu, (command, label)| {
                                let menu =
                                    if matches!(command, Command::Archive | Command::Unarchive) {
                                        menu.separator()
                                    } else {
                                        menu
                                    };
                                let owner = owner.clone();
                                menu.item(PopupMenuItem::new(tr(label)).on_click(
                                    move |_, window, cx| {
                                        let _ = owner.update(cx, |shell, cx| {
                                            shell.live_resource_action(
                                                &Dispatch {
                                                    node,
                                                    target: Target::Session(id),
                                                    command,
                                                },
                                                window,
                                                cx,
                                            );
                                        });
                                    },
                                ))
                            })
                    }),
            )
            .into_any_element()
    }

    pub(super) fn change_live_session(
        &self,
        session: &Session,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let live = self.live.as_ref().unwrap();
        let client = Arc::new(Client::new(live.transport.clone()));
        let request = client.prepare(if command == Command::Delete {
            sailry_protocol::Command::RemoveSession {
                session: session.id,
                expected_revision: session.revision,
            }
        } else if command == Command::Fork {
            sailry_protocol::Command::ForkConversationAt {
                session: session.id,
                worktree: session.worktree,
                expected_revision: session.revision,
            }
        } else {
            sailry_protocol::Command::SetSessionArchived {
                session: session.id,
                expected_revision: session.revision,
                archived: command == Command::Archive,
            }
        });
        let owner = cx.weak_entity();
        let form = cx.new(|_| Change {
            owner,
            client,
            request,
            title: crate::activity::title(session).to_string(),
            pending: false,
            error: None,
        });
        if command != Command::Delete {
            form.update(cx, |form, cx| form.submit(window, cx));
            return;
        }
        form.update(cx, |form, cx| form.prompt(window, cx));
    }
}

struct Change {
    owner: WeakEntity<Shell>,
    client: Arc<Client>,
    request: Request,
    title: String,
    pending: bool,
    error: Option<&'static str>,
}

impl Change {
    fn prompt(&self, window: &mut Window, cx: &mut Context<Self>) {
        let detail = rust_i18n::t!("session_delete_named", name = self.title).to_string();
        if let Some(error) = self.error {
            use gpui_kit::component::notification::Notification;
            crate::feedback::toast(
                window,
                tr(error),
                Notification::error(tr(error)).id1::<Self>(("error", cx.entity_id())),
                cx,
            );
        }
        let form = cx.entity();
        crate::prompts::confirm(
            &tr("session_delete"),
            &detail,
            tr("session_delete"),
            window,
            cx,
            move |window, cx| form.update(cx, |form, cx| form.submit(window, cx)),
        );
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        self.pending = true;
        let node = self.request.target;
        let client = self.client.clone();
        let request = self.request.clone();
        let forking = matches!(
            request.command,
            sailry_protocol::Command::ForkConversationAt { .. }
        );
        let deleting = matches!(
            request.command,
            sailry_protocol::Command::RemoveSession { .. }
        );
        let job = cx
            .global::<Services>()
            .runtime
            .spawn(async move { client.execute(request).await });
        // Keep an immediate archive operation alive after its popup closes.
        let form = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let result = job.await;
            let _ = form.update_in(cx, |form, window, cx| {
                form.pending = false;
                let removed = match result {
                    Ok(Ok(Output::SessionsRemoved(ids))) => Some(ids),
                    Ok(Ok(Output::Session(session))) => {
                        if forking {
                            let _ = form.owner.update(cx, |shell, cx| {
                                if shell
                                    .live
                                    .as_ref()
                                    .is_some_and(|live| live.selected == node)
                                {
                                    shell.reveal_session(session, window, cx);
                                }
                            });
                        }
                        Some(Vec::new())
                    }
                    Ok(Err(error)) => {
                        form.error = Some(match error.code {
                            ErrorCode::Busy if forking => "session_fork_busy",
                            ErrorCode::Busy => "session_delete_busy",
                            ErrorCode::RevisionConflict | ErrorCode::NotFound => "session_changed",
                            ErrorCode::Unavailable | ErrorCode::OutcomeUnknown => {
                                "project_outcome_unknown"
                            }
                            _ => "live_request_failed",
                        });
                        if !matches!(
                            error.code,
                            ErrorCode::Unavailable | ErrorCode::OutcomeUnknown
                        ) {
                            form.request = form.client.prepare(form.request.command.clone());
                        }
                        None
                    }
                    _ => {
                        form.error = Some("project_outcome_unknown");
                        None
                    }
                };
                if let Some(ids) = removed {
                    let _ = form.owner.update(cx, |shell, cx| {
                        for id in ids {
                            shell.close_session_view(Key::Session(node, id), window, cx);
                        }
                        cx.notify();
                    });
                } else if deleting {
                    form.prompt(window, cx);
                } else {
                    crate::feedback::toast(
                        window,
                        tr(form.error.unwrap()),
                        Notification::error(tr(form.error.unwrap())),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
