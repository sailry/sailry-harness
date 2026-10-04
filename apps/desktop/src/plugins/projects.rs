//! Worktree actions keep their captured execution owner across UI navigation.
use super::contributions::{self, Registry};
use crate::{conversation::live::Binding, shell::Shell};
use gpui_kit::*;
use sailry_client::Client;
use sailry_protocol::{NodeId, ProjectId, WorktreeId};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
pub(crate) struct State {
    registries: BTreeMap<(NodeId, WorktreeId), Entity<Registry>>,
}

impl Shell {
    pub(crate) fn project_contribution(
        &mut self,
        project: ProjectId,
        intent: sailry_protocol::plugin::ui::Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(registry) = self.project_plugins(project, window, cx) else {
            return;
        };
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let node = live.selected;
        let Some(tree) = live.project_worktree(project).map(|tree| tree.id) else {
            return;
        };
        let next = std::rc::Rc::new(
            move |shell: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                let Some(live) = shell.live.as_mut().filter(|live| live.selected == node) else {
                    return;
                };
                live.select_project_worktree(project, tree);
                let task = contributions::intents::request(
                    registry.clone(),
                    intent,
                    Some(serde_json::Value::Null),
                    window,
                    cx,
                );
                cx.spawn(async move |_, _| {
                    let _ = task.await;
                })
                .detach();
                cx.notify();
            },
        );
        if !self.guard_document_navigation(window, cx, next.clone()) {
            next(self, window, cx);
        }
    }

    pub(crate) fn project_plugins(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Registry>> {
        let worktree = self.live.as_ref()?.project_worktree(project)?.id;
        self.worktree_plugins(worktree, window, cx)
    }

    pub(crate) fn worktree_plugins(
        &mut self,
        worktree: WorktreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<Registry>> {
        let live = self.live.as_ref()?;
        let snapshot = live.view.snapshot.as_ref()?;
        let tree = snapshot.worktrees.iter().find(|tree| tree.id == worktree)?;
        let key = (live.selected, tree.id);
        let services = cx.global::<crate::backend::Services>();
        let binding = Binding {
            client: Arc::new(live.client()),
            defaults: Arc::new(Client::new(services.local.clone())),
            runtime: services.runtime.clone(),
            project: tree.project,
            worktree: Some(tree.id),
            host: live.name(live.selected),
            project_name: snapshot
                .projects
                .iter()
                .find(|entry| Some(entry.id) == tree.project)
                .map(|project| project.name.clone().into())
                .unwrap_or_default(),
            branch: tree.path.clone().into(),
        };
        let registry = self
            .project_plugins
            .registries
            .entry(key)
            .or_insert_with(|| {
                let registry = cx.new(|cx| Registry::project(&binding, cx));
                cx.observe(&registry, |_, _, cx| cx.notify()).detach();
                cx.subscribe_in(
                    &registry,
                    window,
                    |shell, _, event: &contributions::Event, window, cx| match event {
                        contributions::Event::Mounted(panel) => {
                            Self::observe_plugin_conversations(panel, window, cx);
                        }
                        contributions::Event::Action(action) => {
                            shell.project_plugin_action(action, window, cx);
                        }
                    },
                )
                .detach();
                registry
            })
            .clone();
        registry.update(cx, |registry, cx| {
            registry.sync(
                &binding,
                contributions::ViewState {
                    session: None,
                    assistant: None,
                    snapshot: Some(snapshot),
                    connected: live.view.connected,
                },
                window,
                cx,
            );
        });
        Some(registry)
    }

    pub(crate) fn plugin_intent(
        &mut self,
        binding: Binding,
        source: Option<WeakEntity<crate::conversation::live::View>>,
        intent: sailry_protocol::plugin::ui::Intent,
        value: Option<serde_json::Value>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<serde_json::Value, String>> {
        if let Some(source) = source.and_then(|source| source.upgrade()) {
            return source.update(cx, |source, cx| match value {
                Some(value) => source.contribution_intent(intent, value, window, cx),
                None => source.query_contribution(intent, window, cx),
            });
        }
        let Some(tree) = binding.worktree else {
            return Task::ready(Err("contribution requires a worktree".into()));
        };
        if self
            .live
            .as_ref()
            .is_none_or(|live| live.selected != binding.client.target())
        {
            return Task::ready(Err("contribution context changed".into()));
        }
        let Some(registry) = self.worktree_plugins(tree, window, cx) else {
            return Task::ready(Err("contribution is unavailable".into()));
        };
        if value.is_none() {
            return contributions::intents::request(registry, intent, None, window, cx);
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let sender = std::rc::Rc::new(std::cell::RefCell::new(Some(sender)));
        let action = std::rc::Rc::new(
            move |shell: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                let Some(sender) = sender.borrow_mut().take() else {
                    return;
                };
                let Some(live) = shell
                    .live
                    .as_mut()
                    .filter(|live| live.selected == binding.client.target())
                else {
                    let _ = sender.send(Err("contribution context changed".into()));
                    return;
                };
                if let Some(project) = binding.project {
                    live.select_project_worktree(project, tree);
                }
                let task = contributions::intents::request(
                    registry.clone(),
                    intent,
                    value.clone(),
                    window,
                    cx,
                );
                cx.spawn(async move |_, _| {
                    let _ = sender.send(task.await);
                })
                .detach();
                cx.notify();
            },
        );
        if !self.guard_document_navigation(window, cx, action.clone()) {
            action(self, window, cx);
        }
        cx.spawn(async move |_, _| {
            receiver
                .await
                .map_err(|_| "contribution was cancelled".to_owned())?
        })
    }

    pub(crate) fn project_plugin_overlays(&self, window: &Window, cx: &App) -> Vec<AnyElement> {
        let Some(live) = &self.live else {
            return vec![];
        };
        let Some(tree) = live.selected_worktree() else {
            return vec![];
        };
        self.project_plugins
            .registries
            .get(&(live.selected, tree.id))
            .map(|registry| registry.read(cx).overlays(window, cx))
            .unwrap_or_default()
    }

    pub(crate) fn sync_project_plugins(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let worktrees: std::collections::BTreeSet<_> = self
            .live
            .as_ref()
            .into_iter()
            .flat_map(|live| {
                live.selected_worktree()
                    .map(|tree| tree.id)
                    .into_iter()
                    .chain(
                        live.view
                            .snapshot
                            .iter()
                            .flat_map(|snapshot| snapshot.terminals.iter())
                            .filter(|info| info.status != sailry_protocol::terminal::Status::Closed)
                            .filter_map(|info| info.worktree),
                    )
            })
            .collect();
        for worktree in worktrees {
            self.worktree_plugins(worktree, window, cx);
        }
    }
}
