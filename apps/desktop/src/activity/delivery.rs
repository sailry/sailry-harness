//! Node-backed actions for plugin notifications, independent of the active host.
use super::*;
use sailry_client::activity::Id;
use sailry_protocol::Command;

impl Shell {
    pub(super) fn read_notice(&mut self, id: Id, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(super) fn clear_notices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut nodes = BTreeMap::<NodeId, Vec<sailry_protocol::NotificationId>>::new();
        for notice in self.activity.inbox.notices() {
            if let Target::Notification(id) = notice.id.target {
                nodes.entry(notice.id.node).or_default().push(id);
            }
        }
        self.activity.inbox.clear_local();
        for (node, ids) in nodes {
            self.notification_command(node, Command::DismissNotifications { ids }, window, cx);
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
        let Some(client) = client else { return };
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
