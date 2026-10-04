use super::*;
use crate::plugins::contributions::{Registry, native};
use sailry_protocol::plugin::ui::Slot;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Project,
    Create,
    Repository,
    Path,
    Remove,
}

impl Shell {
    pub(crate) fn project_plugin_action(
        &mut self,
        action: &native::Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (node, worktree) = action.scope();
        let Some(live) = &self.live else {
            return;
        };
        if live.selected != node {
            return;
        }
        let Some(tree) = live
            .view
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .worktrees
                    .iter()
                    .find(|tree| Some(tree.id) == worktree)
            })
            .cloned()
        else {
            return;
        };
        let deferred = action.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.project_plugin_action(&deferred, window, cx)
        }) {
            return;
        }
        let live = self.live.as_mut().unwrap();
        live.project = tree.project;
        live.unassigned_worktree = tree.project.is_none().then_some(tree.id);
        if let Some(project) = tree.project {
            live.worktree_choices.insert((node, project), tree.id);
        }
        action.dispatch(window, cx);
    }

    pub(crate) fn show_live_project_menu(
        &mut self,
        project: ProjectId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let node = self.live.as_ref().unwrap().selected;
        // Core project actions remain available without a worktree contribution registry.
        let Some(registry) = self.project_plugins(project, window, cx) else {
            self.render_project_menu(project, node, None, position, window, cx);
            return;
        };
        if registry.read(cx).ready(cx) {
            self.render_project_menu(project, node, Some(registry), position, window, cx);
            return;
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let mut sender = Some(sender);
        let observation = cx.observe_in(&registry, window, move |shell, registry, window, cx| {
            if shell.live.as_ref().is_none_or(|live| live.selected != node) {
                return;
            }
            shell.project_plugins(project, window, cx);
            if registry.read(cx).ready(cx)
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(());
            }
        });
        cx.spawn_in(window, async move |shell, cx| {
            let _observation = observation;
            let timeout = cx
                .background_executor()
                .timer(std::time::Duration::from_secs(10));
            futures::pin_mut!(receiver, timeout);
            let ready = matches!(
                futures::future::select(receiver, timeout).await,
                futures::future::Either::Left((Ok(()), _))
            );
            let _ = shell.update_in(cx, |shell, window, cx| {
                if shell.live.as_ref().is_none_or(|live| live.selected != node) {
                    return;
                }
                if !ready {
                    crate::feedback::toast(
                        window,
                        tr("plugins_failed"),
                        Notification::error(tr("plugins_failed")),
                        cx,
                    );
                }
                shell.render_project_menu(project, node, Some(registry), position, window, cx);
            });
        })
        .detach();
    }

    fn render_project_menu(
        &self,
        project: ProjectId,
        node: NodeId,
        registry: Option<Entity<Registry>>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = self.live.as_ref().filter(|live| live.selected == node) else {
            return;
        };
        if !live
            .view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.projects.iter().any(|entry| entry.id == project))
        {
            return;
        }
        let mut menu = NativeMenu::new();
        #[cfg(test)]
        let mut actions = Vec::new();
        let mut previous = None;
        for (command, label) in items(Target::Project(project), false) {
            if !command.available(live.view.snapshot.as_ref()) {
                continue;
            }
            let group = match command {
                Command::Open | Command::Edit => Group::Project,
                Command::NewSession => Group::Create,
                Command::Worktrees => Group::Repository,
                #[cfg(test)]
                Command::Branches => Group::Repository,
                Command::Copy => Group::Path,
                _ => Group::Remove,
            };
            if previous.is_some_and(|previous| previous != group) {
                menu = menu.separator();
            }
            previous = Some(group);
            let action = Dispatch {
                node,
                target: Target::Project(project),
                command,
            };
            #[cfg(test)]
            actions.push(action.clone());
            menu = menu.menu(tr(label), Box::new(action));
            if command == Command::NewSession
                && let Some(registry) = &registry
            {
                menu = native::append(menu, registry.clone(), Slot::ProjectMenu, cx);
            }
        }
        #[cfg(test)]
        cx.default_global::<Captures>()
            .0
            .insert(window.window_handle().window_id(), actions);
        menu.show(position, window, cx);
    }
}

#[cfg(test)]
#[derive(Default)]
struct Captures(std::collections::HashMap<WindowId, Vec<Dispatch>>);
#[cfg(test)]
impl Global for Captures {}

#[cfg(test)]
pub(super) fn opened(window: &Window, cx: &App, node: NodeId, project: ProjectId) -> bool {
    cx.try_global::<Captures>()
        .and_then(|captures| captures.0.get(&window.window_handle().window_id()))
        .is_some_and(|actions| {
            !actions.is_empty()
                && actions
                    .iter()
                    .all(|action| action.node == node && action.target == Target::Project(project))
        })
}

#[cfg(test)]
pub(super) fn captured(window: &mut Window, cx: &mut App) -> Vec<Dispatch> {
    cx.default_global::<Captures>()
        .0
        .get(&window.window_handle().window_id())
        .cloned()
        .expect("project native menu was not opened")
}

#[cfg(test)]
pub(super) fn choose(command: Command, window: &mut Window, cx: &mut App) {
    let action = captured(window, cx)
        .into_iter()
        .find(|action| action.command == command)
        .expect("project native menu action is absent");
    window.dispatch_action(Box::new(action), cx);
}
