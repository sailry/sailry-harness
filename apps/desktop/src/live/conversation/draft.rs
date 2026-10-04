//! One desktop draft; execution-host selection is an editable UI option.
use super::*;

impl Shell {
    pub(crate) fn new_unassigned_conversation(
        &mut self,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.live.as_ref().is_none_or(|live| live.selected != node)
            || self
                .chats
                .draft
                .as_ref()
                .is_some_and(|view| !view.read(cx).can_retarget())
        {
            return;
        }
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.new_unassigned_conversation(node, window, cx);
        }) {
            return;
        }
        let live = self.live.as_mut().unwrap();
        live.project = None;
        live.unassigned_worktree = None;
        if let Some(view) = self.chats.draft.clone() {
            let binding = self.draft_binding();
            view.update(cx, |view, cx| view.retarget(binding, window, cx));
        }
        self.new_live_conversation(window, cx);
    }

    fn draft_binding(&self) -> Binding {
        let live = self.live.as_ref().unwrap();
        let snapshot = live.view.snapshot.as_ref();
        let project = live.selected_project();
        let tree = project
            .and_then(|project| live.project_worktree(project.id))
            .or_else(|| {
                snapshot?.worktrees.iter().find(|tree| {
                    project.is_some()
                        && tree.project == project.map(|project| project.id)
                        && tree.main
                })
            });
        Binding {
            client: Arc::new(live.client()),
            defaults: Arc::new(Client::new(live.services.local.clone())),
            runtime: live.services.runtime.clone(),
            project: project.map(|project| project.id),
            worktree: tree.map(|tree| tree.id),
            host: live.name(live.selected),
            project_name: project
                .map(|project| project.name.clone().into())
                .unwrap_or_default(),
            branch: tree
                .map(|tree| tree.path.clone().into())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn draft_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Chat> {
        let view = if let Some(view) = &self.chats.draft {
            view.clone()
        } else {
            let view = self.chat_view(self.draft_binding(), None, window, cx);
            self.chats.draft = Some(view.clone());
            view
        };
        let binding = view.read(cx).binding();
        if let Some(live) = &mut self.live
            && live.selected == binding.client.target()
            && live.project.is_none()
        {
            live.project = binding.project;
        }
        let live = self.live.as_ref().unwrap();
        let hosts = std::iter::once(live.services.local.target())
            .chain(
                live.hosts
                    .keys()
                    .copied()
                    .filter(|node| *node != live.services.local.target()),
            )
            .map(|node| (node, live.name(node)))
            .collect();
        view.update(cx, |view, cx| view.set_hosts(hosts, cx));
        view
    }

    pub(super) fn select_draft_host(
        &mut self,
        view: &Entity<Chat>,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chats.draft.as_ref() != Some(view) || !view.read(cx).can_retarget() {
            return;
        }
        if view.read(cx).binding().client.target() == node {
            return;
        }
        let selected = view.clone();
        if self.guard_file_navigation(window, cx, move |shell, window, cx| {
            shell.select_draft_host(&selected, node, window, cx);
        }) {
            return;
        }
        let live = self.live.as_mut().unwrap();
        if live.transport_for(node).is_none() {
            return;
        }
        live.select(node, cx);
        let binding = self.draft_binding();
        view.update(cx, |view, cx| view.retarget(binding, window, cx));
        self.close_resource_panel(cx);
        self.page = Page::Conversation;
        cx.notify();
    }

    pub(super) fn select_draft_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.chats.draft.clone() {
            let binding = self.draft_binding();
            if binding.project.is_some() {
                view.update(cx, |view, cx| view.retarget(binding, window, cx));
            } else {
                let binding = view.read(cx).binding();
                let live = self.live.as_mut().unwrap();
                if live.selected != binding.client.target() {
                    live.select(binding.client.target(), cx);
                }
                live.project = binding.project;
                if let (Some(project), Some(worktree)) = (binding.project, binding.worktree) {
                    live.worktree_choices
                        .insert((live.selected, project), worktree);
                }
            }
        }
    }

    pub(crate) fn restore_session_scope(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.current_chat() else {
            return;
        };
        let binding = view.read(cx).binding();
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if live.selected != binding.client.target() {
            live.select(binding.client.target(), cx);
        }
        live.project = binding.project;
        live.unassigned_worktree = if binding.project.is_none() {
            binding.worktree
        } else {
            None
        };
        if let (Some(project), Some(worktree)) = (binding.project, binding.worktree) {
            live.worktree_choices
                .insert((live.selected, project), worktree);
        }
    }
}
