//! Observe delegated histories through Client's existing ordered projections.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Child {
    pub history: History,
    stop: CancellationToken,
    _task: Task<()>,
}

impl Drop for Child {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl View {
    pub(in crate::conversation::live) fn sync_child_approvals(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.session() else {
            return;
        };
        let Some(snapshot) = &self.node.snapshot else {
            return;
        };
        let mut descendants = BTreeSet::from([root]);
        loop {
            let previous = descendants.len();
            for session in &snapshot.sessions {
                if session
                    .delegation
                    .as_ref()
                    .is_some_and(|origin| descendants.contains(&origin.session))
                {
                    descendants.insert(session.id);
                }
            }
            if previous == descendants.len() {
                break;
            }
        }
        descendants.remove(&root);
        self.approvals
            .children
            .retain(|id, _| descendants.contains(id));
        for id in descendants {
            if self.approvals.children.contains_key(&id) {
                continue;
            }
            let (sender, mut receiver) = tokio::sync::watch::channel(History::default());
            let (_, requests) = tokio::sync::mpsc::channel(1);
            let stop = self.stop.child_token();
            let cancellation = stop.clone();
            let client = self.binding.client.clone();
            self.binding.runtime.spawn(async move {
                let _ = client
                    .watch_conversation(id, sender, cancellation, requests)
                    .await;
            });
            let task = cx.spawn(async move |view, cx| {
                loop {
                    let history = receiver.borrow_and_update().clone();
                    if view
                        .update(cx, |view, cx| {
                            if let Some(child) = view.approvals.children.get_mut(&id) {
                                child.history = history;
                                view.approvals.observe(&view.history);
                                view.reconcile_details();
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                    if receiver.changed().await.is_err() {
                        break;
                    }
                }
            });
            self.approvals.children.insert(
                id,
                Child {
                    history: History::default(),
                    stop,
                    _task: task,
                },
            );
        }
        self.approvals.observe(&self.history);
        self.reconcile_details();
    }
}
