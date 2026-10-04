//! Transient package-authored resource titles share sidebar and dock presentation.
use super::*;

pub(super) struct Title {
    owner: WeakEntity<Panel>,
    resource: Resource,
    value: SharedString,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::plugins) struct ResourceTitle {
    pub(super) resource: Resource,
    pub(super) title: String,
}

impl State {
    pub(crate) fn prune(&mut self, node: NodeId, snapshot: &sailry_protocol::Snapshot) {
        self.titles.retain(|(target, package), title| {
            let Target::Terminal(owner, tree, id) = target else {
                return false;
            };
            *owner != node
                || (title.owner.upgrade().is_some()
                    && snapshot
                        .plugins
                        .iter()
                        .any(|info| info.name == *package && info.enabled)
                    && snapshot.terminals.iter().any(|info| {
                        info.id == *id
                            && info.worktree == Some(*tree)
                            && info.status != sailry_protocol::terminal::Status::Closed
                    }))
        });
    }
}

impl Panel {
    pub(super) fn permits_title(&self, resource: Resource, cx: &App) -> bool {
        self.permits_pane(resource, cx)
            && self.selected.as_ref().is_some_and(|package| {
                self.metadata
                    .read(cx)
                    .entries
                    .get(&package.name)
                    .and_then(|info| info.extension.as_ref())
                    .and_then(|extension| extension.desktop.as_ref())
                    .is_some_and(|desktop| {
                        desktop.renderers.iter().any(|renderer| {
                            matches!(
                                (renderer.resource, resource),
                                (
                                    sailry_protocol::plugin::desktop::ResourceKind::Terminal,
                                    Resource::Terminal(_)
                                )
                            )
                        })
                    })
            })
    }
}

impl Shell {
    pub(super) fn observe_resource_titles(panel: &Entity<Panel>, cx: &mut Context<Self>) {
        cx.observe(panel, |shell, panel, cx| {
            let owner = panel.downgrade();
            let state = panel.read(cx);
            let before = shell.plugin_panes.titles.len();
            shell.plugin_panes.titles.retain(|_, title| {
                title.owner != owner
                    || (state.connected
                        && !state.loading
                        && state
                            .mounted
                            .as_ref()
                            .is_some_and(|mounted| mounted.active())
                        && state
                            .selected
                            .as_ref()
                            .is_some_and(|package| state.available(package))
                        && state.permits_title(title.resource, cx))
            });
            if shell.plugin_panes.titles.len() != before {
                cx.notify();
            }
        })
        .detach();
    }

    pub(super) fn publish_resource_titles(
        &mut self,
        panel: &Entity<Panel>,
        package: &Reference,
        titles: &[ResourceTitle],
        cx: &mut Context<Self>,
    ) {
        let owner = panel.downgrade();
        self.plugin_panes
            .titles
            .retain(|_, title| title.owner != owner);
        for title in titles {
            let Some(target) = panel.read(cx).target(title.resource) else {
                continue;
            };
            self.plugin_panes.titles.insert(
                (target, package.name.clone()),
                Title {
                    owner: owner.clone(),
                    resource: title.resource,
                    value: title.title.clone().into(),
                },
            );
            if let Some(pane) = self
                .splits
                .read(cx)
                .pane(target)
                .filter(|pane| {
                    pane.read(cx)
                        .extension
                        .as_ref()
                        .is_some_and(|extension| extension.package == package.name)
                })
                .cloned()
            {
                pane.update(cx, |pane, cx| {
                    pane.title = title.title.clone().into();
                    cx.notify();
                });
            }
        }
        cx.notify();
    }

    pub(crate) fn terminal_title(
        &self,
        node: NodeId,
        tree: WorktreeId,
        id: TerminalId,
        cx: &App,
    ) -> SharedString {
        let target = Target::Terminal(node, tree, id);
        if let Some(pane) = self.splits.read(cx).pane(target)
            && pane.read(cx).extension.is_some()
        {
            return pane.read(cx).title.clone();
        }
        self.plugin_panes
            .titles
            .iter()
            .find_map(|((key, _), title)| {
                (*key == target && title.owner.upgrade().is_some()).then(|| title.value.clone())
            })
            .or_else(|| {
                self.splits
                    .read(cx)
                    .pane(target)
                    .map(|pane| pane.read(cx).title.clone())
            })
            .unwrap_or_else(|| crate::tr("terminal"))
    }
}
