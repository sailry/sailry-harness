use super::*;
use sailry_client::Client;

impl Shell {
    pub(crate) fn bind_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(live) = &self.live else { return };
        self.activity
            .observers
            .retain(|node, observer| live.hosts.get(node) == Some(&observer.address));
        let runtime = cx.global::<crate::backend::Services>().runtime.clone();
        for (node, address) in &live.hosts {
            if self.activity.observers.contains_key(node) {
                continue;
            }
            let Some(transport) = live.transport_for(*node) else {
                continue;
            };
            let node = *node;
            let stop = CancellationToken::new();
            let cancellation = stop.clone();
            let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
            runtime.spawn(async move {
                let _ = Client::new(transport).watch(sender, cancellation).await;
            });
            let cancellation = stop.clone();
            let task = cx.spawn_in(window, async move |shell, cx| {
                while receiver.changed().await.is_ok() && !cancellation.is_cancelled() {
                    let view = receiver.borrow_and_update().clone();
                    if shell
                        .update_in(cx, |shell, window, cx| {
                            if cancellation.is_cancelled() {
                                return;
                            }
                            let notices = view.notifications.clone();
                            if let Some(observer) = shell.activity.observers.get_mut(&node) {
                                observer.view = view;
                            }
                            shell.receive_notices(node, &notices, window, cx);
                            cx.notify();
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            self.activity.observers.insert(
                node,
                Observer {
                    address: address.clone(),
                    view: View::default(),
                    stop,
                    _task: task,
                },
            );
        }
    }
}
