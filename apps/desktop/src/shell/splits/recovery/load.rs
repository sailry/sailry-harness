//! Startup reads are grouped by Node; business commands are never retried here.
use super::*;
use crate::{backend::Services, panes, preview::Page, shell::session_scope::Key};
use sailry_link::{CancellationToken, EndpointAddr, Transport};
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Snapshot};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

impl Shell {
    pub(in crate::shell) fn restore_split_workspaces(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(value) = crate::preferences::data(cx).workspaces else {
            return;
        };
        let saved = match serde_json::from_value::<panes::Saved>(value)
            .map_err(|error| error.to_string())
            .and_then(|saved| saved.validate().map(|_| saved))
        {
            Ok(saved) => saved,
            Err(error) => {
                self.splits.update(cx, |splits, _| splits.suspend_saving());
                cx.defer_in(window, move |_, window, cx| {
                    crate::feedback::toast(
                        window,
                        tr("pane_restore_failed"),
                        crate::feedback::diagnostic(error.into()).title(tr("pane_restore_failed")),
                        cx,
                    );
                });
                return;
            }
        };
        self.splits
            .update(cx, |splits, cx| splits.restore(&saved, window, cx));
        match self.splits.read(cx).active {
            Some(Target::Session(node, id)) => self.session_scope.open(Key::Session(node, id)),
            Some(Target::Terminal(node, tree, id)) => {
                self.page = Page::Terminal;
                self.restore_terminal_scope(node, tree, id, cx);
            }
            _ => {}
        }
        self.restore_targets(saved.targets(), window, cx);
    }

    pub(crate) fn restore_pane(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.recovery.pending(target, cx) {
            self.restore_targets([target], window, cx);
        }
    }

    fn restore_targets(
        &mut self,
        targets: impl IntoIterator<Item = Target>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.live.is_none() {
            return;
        }
        let mut nodes: BTreeMap<NodeId, Vec<(Target, u64)>> = BTreeMap::new();
        for target in targets {
            let node = match target {
                Target::Session(node, _) | Target::Terminal(node, ..) => node,
                Target::Draft => continue,
            };
            if !self.splits.read(cx).contains(target) {
                continue;
            }
            let owner = cx.weak_entity();
            let view = cx.new(|_| View::new(target, owner));
            let generation = self.recovery.begin(target, view.clone());
            self.splits.update(cx, |splits, cx| {
                splits.hydrate(target, tr("chat_new"), "".into(), view.into(), cx)
            });
            nodes.entry(node).or_default().push((target, generation));
        }
        let services = cx.global::<Services>().clone();
        for (node, targets) in nodes {
            let stop = self.recovery.stop.clone();
            let job = services.runtime.spawn(read(services.clone(), node, stop));
            cx.spawn_in(window, async move |shell, cx| {
                let result = job.await.unwrap_or_else(|_| {
                    Err(Fault::new(
                        ErrorCode::Internal,
                        "workspace recovery task failed",
                    ))
                });
                let _ = shell.update_in(cx, |shell, window, cx| {
                    let current: Vec<_> = targets
                        .into_iter()
                        .filter(|(target, generation)| {
                            shell.recovery.current(*target, *generation)
                                && shell.splits.read(cx).contains(*target)
                        })
                        .collect();
                    if current.is_empty() {
                        return;
                    }
                    match result {
                        Ok((transport, snapshot, address)) => {
                            if let Some(address) = address {
                                shell.live.as_mut().unwrap().hosts.insert(node, address);
                            }
                            // Reuse the controller's authenticated transport identity across restored panes.
                            let transport = shell
                                .live
                                .as_ref()
                                .unwrap()
                                .transport_for(node)
                                .unwrap_or(transport);
                            for (target, _) in current {
                                shell.recovery.cancel(target);
                                shell.hydrate_recovery(
                                    target,
                                    transport.clone(),
                                    &snapshot,
                                    window,
                                    cx,
                                );
                            }
                        }
                        Err(error) => {
                            for (target, _) in current {
                                shell.recovery.fail(target, error.clone(), cx);
                            }
                            crate::feedback::toast(
                                window,
                                tr("pane_restore_failed"),
                                crate::feedback::diagnostic(
                                    format!("{:?}: {}", error.code, error.message).into(),
                                )
                                .title(tr("pane_restore_failed"))
                                .id1::<State>(crate::live::node_key(node)),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    fn hydrate_recovery(
        &mut self,
        target: Target,
        transport: Arc<dyn Transport>,
        snapshot: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(target, Target::Terminal(..)) {
            if !self.restore_terminal_pane(target, transport, snapshot, window, cx) {
                self.restore_failed(target, "pane_restore_failed", window, cx);
                return;
            }
        } else if let Target::Session(node, id) = target {
            let Some(session) = snapshot
                .sessions
                .iter()
                .find(|session| session.id == id && session.delegation.is_none())
                .cloned()
            else {
                self.restore_failed(target, "pane_session_missing", window, cx);
                return;
            };
            let services = cx.global::<Services>();
            let project_name = snapshot
                .projects
                .iter()
                .find(|project| Some(project.id) == session.project)
                .map(|project| SharedString::from(project.name.clone()))
                .unwrap_or_default();
            let binding = crate::conversation::live::Binding {
                client: Arc::new(self.live.as_ref().unwrap().client_for(transport)),
                defaults: Arc::new(
                    self.live
                        .as_ref()
                        .unwrap()
                        .client_for(services.local.clone()),
                ),
                runtime: services.runtime.clone(),
                project: session.project,
                worktree: Some(session.worktree),
                host: self.live.as_ref().unwrap().name(node),
                project_name: project_name.clone(),
                branch: snapshot
                    .worktrees
                    .iter()
                    .find(|tree| tree.id == session.worktree)
                    .map(|tree| tree.path.clone().into())
                    .unwrap_or_default(),
            };
            let title = crate::activity::title(&session);
            let view = self
                .chats
                .views
                .get(&(node, id))
                .cloned()
                .unwrap_or_else(|| self.chat_view(binding, Some(session), window, cx));
            self.chats.views.insert((node, id), view.clone());
            self.splits.update(cx, |splits, cx| {
                splits.hydrate(target, title, project_name, view.into(), cx)
            });
        }
        if self.splits.read(cx).active == Some(target) {
            self.focus_pane(target, window, cx);
        }
        cx.notify();
    }

    fn restore_failed(
        &mut self,
        target: Target,
        message: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.splits.read(cx).contains(target) {
            return;
        }
        self.remove_pane(target, window, cx);
        crate::feedback::toast(
            window,
            tr(message),
            gpui_kit::component::notification::Notification::error(tr(message))
                .id1::<Self>((message, cx.entity_id())),
            cx,
        );
    }
}

async fn read(
    services: Services,
    node: NodeId,
    stop: CancellationToken,
) -> Result<(Arc<dyn Transport>, Snapshot, Option<EndpointAddr>), Fault> {
    let (transport, address): (Arc<dyn Transport>, _) = if node == services.local.target() {
        (services.local.clone(), None)
    } else {
        let peers = tokio::select! { biased; _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "workspace recovery cancelled")), peers = services.link.peers() => peers? };
        let address = peers
            .into_iter()
            .find(|address| *address.id.as_bytes() == node.0)
            .ok_or_else(|| Fault::new(ErrorCode::Unavailable, "paired Node is unavailable"))?;
        (services.link.remote(address.clone()), Some(address))
    };
    let client = sailry_client::Client::new(transport.clone());
    let request = client.prepare(Command::Snapshot);
    for attempt in 0..=4 {
        let result = tokio::select! { biased; _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "workspace recovery cancelled")), result = client.execute(request.clone()) => result };
        match result {
            Ok(Output::Snapshot(snapshot)) => return Ok((transport, snapshot, address)),
            Ok(_) => {
                return Err(Fault::new(
                    ErrorCode::Internal,
                    "snapshot response expected",
                ));
            }
            Err(error) if error.code == ErrorCode::Busy && attempt < 4 => {
                tokio::select! { biased; _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "workspace recovery cancelled")), _ = tokio::time::sleep(Duration::from_millis(250 << attempt)) => {} }
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("bounded snapshot retries return a result")
}
