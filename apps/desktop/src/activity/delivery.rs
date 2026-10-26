//! Node-backed actions for plugin notifications, independent of the active host.
use super::*;
use sailry_client::activity::Id;
use sailry_protocol::Command;

impl Shell {
    pub(super) fn read_notice(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .activity
            .inbox
            .notices()
            .iter()
            .any(|notice| notice.id == id && notice.read)
        {
            return;
        }
        if let Target::Notification(notification) = id.target {
            self.notification_command(
                id.node,
                Command::MarkNotificationRead { id: notification },
                window,
                cx,
            );
        } else {
            self.activity.inbox.mark_read(id);
        }
    }

    pub(super) fn update_notices(
        &mut self,
        remove: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.activity.pending {
            return;
        }
        let batch = self.activity.inbox.prepare(if remove {
            sailry_client::activity::Change::DismissRead
        } else {
            sailry_client::activity::Change::ReadAll
        });
        if batch.is_empty() {
            return;
        }
        let clients: BTreeMap<_, _> = batch
            .nodes()
            .filter_map(|node| {
                self.live.as_ref().and_then(|live| {
                    live.transport_for(node)
                        .map(|transport| (node, std::sync::Arc::new(live.client_for(transport))))
                })
            })
            .collect();
        self.activity.pending = true;
        let job = cx
            .global::<crate::backend::Services>()
            .runtime
            .spawn(async move {
                let report = batch.run(&clients).await;
                (batch, report)
            });
        cx.spawn_in(window, async move |shell, cx| {
            let result = job.await;
            let _ = shell.update_in(cx, |shell, window, cx| {
                shell.activity.pending = false;
                let (completed, failed, unknown) = match result {
                    Ok((batch, report)) => {
                        shell.activity.inbox.complete(&batch, &report);
                        (report.completed, report.failed(), report.unknown())
                    }
                    Err(_) => (0, true, true),
                };
                let key = if unknown {
                    "notification_update_unknown"
                } else if failed && completed > 0 {
                    "notification_update_partial"
                } else if failed {
                    "notification_update_failed"
                } else if remove {
                    "notification_deleted_read"
                } else {
                    "notifications_read_all"
                };
                let text = tr(key);
                let toast = if failed {
                    gpui_kit::component::notification::Notification::error(text.clone())
                } else {
                    gpui_kit::component::notification::Notification::success(text.clone())
                };
                crate::feedback::toast(window, text, toast, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn read_session_notices(
        &mut self,
        node: NodeId,
        session: sailry_protocol::SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids: Vec<_> = self
            .activity
            .inbox
            .notices()
            .iter()
            .filter(|notice| {
                !notice.read
                    && notice.id.node == node
                    && notice.target() == Target::Session(session)
                    && matches!(
                        notice.kind,
                        sailry_client::activity::Kind::Completed
                            | sailry_client::activity::Kind::Failed
                    )
            })
            .map(|notice| notice.id)
            .collect();
        for id in ids {
            self.read_notice(id, window, cx);
        }
    }

    fn notification_command(
        &mut self,
        node: NodeId,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let client = self.live.as_ref().and_then(|live| {
            live.transport_for(node)
                .map(|transport| live.client_for(transport))
        });
        let Some(client) = client else {
            crate::feedback::toast(
                window,
                tr("notification_update_failed"),
                gpui_kit::component::notification::Notification::error(tr(
                    "notification_update_failed",
                )),
                cx,
            );
            return;
        };
        let request = client.prepare(command);
        let job = cx
            .global::<crate::backend::Services>()
            .runtime
            .spawn(async move { client.execute(request).await });
        cx.spawn_in(window, async move |shell, cx| {
            let result = job.await;
            let _ = shell.update_in(cx, |_, window, cx| {
                if !matches!(result, Ok(Ok(_))) {
                    crate::feedback::toast(
                        window,
                        tr("live_request_failed"),
                        gpui_kit::component::notification::Notification::error(tr(
                            "live_request_failed",
                        )),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn open_notification(
        &mut self,
        node: NodeId,
        id: sailry_protocol::NotificationId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(notice) = self
            .activity_snapshot(node)
            .and_then(|snapshot| snapshot.notifications.iter().find(|notice| notice.id == id))
            .cloned()
        else {
            return;
        };
        if let Some(sailry_protocol::notification::Details::Terminal(terminal)) =
            &notice.content.details
        {
            self.open_activity(node, Target::Terminal(terminal.id), window, cx);
            return;
        }
        if let Some(session) = notice.content.session {
            self.open_activity(node, Target::Session(session), window, cx);
            return;
        }
        if let Some(entry) = self
            .extension_entries(cx)
            .into_iter()
            .find(|entry| entry.node == node && entry.package.name == notice.package)
        {
            self.open_extension(entry, window, cx);
            return;
        }
        // A captured notice can switch hosts before the shared manifest cache is ready.
        self.sync_extensions(window, cx);
        let Some(metadata) = self.extensions.as_ref().map(|state| state.metadata.clone()) else {
            return;
        };
        if metadata.read(cx).settled() {
            return;
        }
        let package = notice.package;
        let mut pending = true;
        cx.observe_in(&metadata, window, move |shell, metadata, window, cx| {
            if !pending || !metadata.read(cx).settled() {
                return;
            }
            pending = false;
            if shell.live.as_ref().is_none_or(|live| live.selected != node) {
                return;
            }
            if let Some(entry) = shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.node == node && entry.package.name == package)
            {
                shell.open_extension(entry, window, cx);
            }
        })
        .detach();
    }
}
