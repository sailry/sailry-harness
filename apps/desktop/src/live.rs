//! Presentation selection only. The shared Client owns projection and reconnection.
use crate::{backend::Services, shell::Shell, tr};
use gpui_kit::*;
use sailry_client::{Client, View};
use sailry_link::{CancellationToken, EndpointAddr, Transport};
use sailry_protocol::{Command, HostInfo, NodeId, Output, ProjectId, WorktreeId};
use std::{collections::BTreeMap, sync::Arc};

pub(crate) mod conversation;
mod devices;
#[cfg(test)]
mod file_tests;
pub(crate) mod host;
pub(crate) mod menus;
pub(crate) mod overview;
mod projects;
#[cfg(test)]
pub(crate) use file_tests::observed::Observed as FileTransport;
pub(crate) mod terminal;
#[cfg(test)]
mod tests;
mod view;
mod worktree_selection;

pub(crate) struct State {
    pub hosts: BTreeMap<NodeId, EndpointAddr>,
    pub selected: NodeId,
    pub project: Option<ProjectId>,
    pub(crate) unassigned_worktree: Option<WorktreeId>,
    worktree_choices: BTreeMap<(NodeId, ProjectId), WorktreeId>,
    pub view: View,
    pub info: Option<HostInfo>,
    pending: bool,
    action_error: bool,
    services: Services,
    pub transport: Arc<dyn Transport>,
    #[cfg(test)]
    test_transports: BTreeMap<NodeId, Arc<dyn Transport>>,
    git_reads: Arc<sailry_client::git::Reads>,
    watch: Option<Watch>,
    _peers: Task<()>,
}

struct Watch {
    stop: CancellationToken,
    _task: Task<()>,
    _inspection: Task<()>,
}
impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Shell {
    pub(crate) fn observe_live_errors(window: &Window, cx: &mut Context<Self>) {
        crate::feedback::observe(window, cx, |shell: &Self, _| {
            shell
                .live
                .as_ref()
                .is_some_and(|live| live.action_error)
                .then_some("live_request_failed")
                .into_iter()
                .collect()
        });
        crate::feedback::observe_with(
            window,
            cx,
            |shell: &Self, _| {
                shell
                    .live
                    .as_ref()
                    .and_then(|live| live.view.error.as_ref())
                    .map(|_| "live_sync_failed")
                    .into_iter()
                    .collect()
            },
            |shell, key, _| {
                crate::feedback::diagnostic(
                    shell
                        .live
                        .as_ref()
                        .unwrap()
                        .view
                        .error
                        .as_ref()
                        .unwrap()
                        .message
                        .clone()
                        .into(),
                )
                .title(tr(key))
            },
        );
    }
}

impl State {
    pub(crate) fn client(&self) -> Client {
        self.client_for(self.transport.clone())
    }

    pub(crate) fn client_for(&self, transport: Arc<dyn Transport>) -> Client {
        Client::new(transport).with_git_reads(self.git_reads.clone())
    }

    pub fn attach(cx: &mut Context<Shell>) -> Option<Self> {
        let services = cx.try_global::<Services>()?.clone();
        let local = services.local.target();
        let peers = devices::observe(&services, cx);
        let mut live = Self {
            hosts: BTreeMap::from([(local, services.link.address())]),
            selected: local,
            project: None,
            unassigned_worktree: None,
            worktree_choices: BTreeMap::new(),
            view: View::default(),
            info: None,
            pending: false,
            action_error: false,
            transport: services.local.clone(),
            #[cfg(test)]
            test_transports: BTreeMap::new(),
            git_reads: Default::default(),
            services,
            watch: None,
            _peers: peers,
        };
        live.select(local, cx);
        Some(live)
    }

    pub(crate) fn transport_for(&self, node: NodeId) -> Option<Arc<dyn Transport>> {
        #[cfg(test)]
        if (node == self.services.local.target() || self.hosts.contains_key(&node))
            && let Some(transport) = self.test_transports.get(&node)
        {
            return Some(transport.clone());
        }
        if node == self.services.local.target() {
            Some(self.services.local.clone())
        } else if let Some(address) = self.hosts.get(&node) {
            Some(self.services.link.remote(address.clone()))
        } else {
            None
        }
    }

    #[cfg(test)]
    pub(crate) fn override_transport(&mut self, transport: Arc<dyn Transport>) {
        let node = transport.target();
        self.test_transports.insert(node, transport.clone());
        if self.selected == node {
            self.transport = transport;
        }
    }

    pub fn select(&mut self, node: NodeId, cx: &mut Context<Shell>) {
        let Some(transport) = self.transport_for(node) else {
            return;
        };
        self.watch = None;
        self.selected = node;
        self.project = None;
        self.unassigned_worktree = None;
        self.view = View::default();
        self.info = None;
        self.transport = transport.clone();
        let stop = CancellationToken::new();
        let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
        let cancellation = stop.clone();
        let client = Client::new(transport.clone());
        let inspection_stop = stop.clone();
        let request = self.services.runtime.spawn(async move {
            tokio::select! {
                _ = inspection_stop.cancelled() => None,
                result = client.execute(client.prepare(Command::InspectHost)) => result.ok(),
            }
        });
        let inspection_stop = stop.clone();
        let inspection = cx.spawn(async move |shell, cx| {
            if let Ok(Some(Output::HostInfo(info))) = request.await
                && !inspection_stop.is_cancelled()
            {
                let _ = shell.update(cx, |shell, cx| {
                    if let Some(live) = &mut shell.live
                        && live.selected == info.node
                    {
                        live.info = Some(info);
                        cx.notify();
                    }
                });
            }
        });
        self.services.runtime.spawn(async move {
            let _ = Client::new(transport).watch(sender, cancellation).await;
        });
        let task = cx.spawn(async move |shell, cx| {
            loop {
                let view = receiver.borrow_and_update().clone();
                if shell
                    .update(cx, |shell, cx| {
                        if let Some(live) = &mut shell.live
                            && live.selected == node
                        {
                            live.accept_view(view);
                            if let Some(snapshot) = &live.view.snapshot {
                                shell.terminals.prune(node, snapshot);
                                shell.plugin_panes.prune(node, snapshot);
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
                if receiver.changed().await.is_err() {
                    return;
                }
            }
        });
        self.watch = Some(Watch {
            stop,
            _task: task,
            _inspection: inspection,
        });
        cx.notify();
    }

    pub fn name(&self, node: NodeId) -> SharedString {
        if node == self.services.local.target() {
            tr("composer_host_local")
        } else {
            format!("{} {}", tr("composer_host_remote"), short_id(node)).into()
        }
    }

    pub fn selected_project(&self) -> Option<&sailry_protocol::Project> {
        self.view
            .snapshot
            .as_ref()?
            .projects
            .iter()
            .find(|project| Some(project.id) == self.project)
    }
}

pub(crate) fn short_id(node: NodeId) -> String {
    node.0[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn node_key(node: NodeId) -> String {
    node.0.iter().map(|byte| format!("{byte:02x}")).collect()
}
