//! Conversation execution locations compose the shared Kit command surface.
use super::*;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::Project;
use sailry_protocol::plugin::ui::Intent;

#[cfg(test)]
mod tests;

#[derive(Clone)]
struct Location {
    owner: WeakEntity<View>,
    binding: Binding,
    session: Option<Session>,
}
impl Location {
    fn current(&self, view: &View) -> bool {
        view.binding.client.target() == self.binding.client.target()
            && view.binding.project == self.binding.project
            && view.binding.worktree == self.binding.worktree
            && view
                .session
                .as_ref()
                .map(|session| (session.id, session.revision))
                == self
                    .session
                    .as_ref()
                    .map(|session| (session.id, session.revision))
    }
}

#[derive(Clone)]
enum Choice {
    Unassigned,
    Project(Project),
    AddProject,
}
struct Picker {
    bound: Location,
    choices: Vec<(String, Choice)>,
    visible: Vec<usize>,
    selected: Option<IndexPath>,
    query: String,
}

impl View {
    pub(crate) fn fork_worktree(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.binding.project.is_some() && self.session.is_some() {
            self.invoke_location(Intent::ForkWorktree, window, cx);
        }
    }

    fn invoke_location(&self, intent: Intent, window: &mut Window, cx: &mut Context<Self>) {
        let task = self.contribution_intent(intent, serde_json::Value::Null, window, cx);
        cx.spawn(async move |view, cx| {
            if task.await.is_err() {
                let _ = view.update(cx, |view, cx| {
                    view.error = Some("plugins_view_unavailable");
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn location(&self, cx: &Context<Self>) -> Location {
        Location {
            owner: cx.weak_entity(),
            binding: self.binding.clone(),
            session: self.session.clone(),
        }
    }

    pub(super) fn turn_worktree(&self, turn: TurnId) -> Option<WorktreeId> {
        self.history
            .snapshot
            .as_ref()?
            .page
            .runs
            .iter()
            .find(|run| run.turn == turn)
            .map(|run| run.worktree)
    }

    pub(super) fn can_move(&self) -> bool {
        self.assistant.is_none()
            && self.connected()
            && !self.readonly()
            && !self.busy()
            && self.active().is_none()
            && self
                .history
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| snapshot.page.queue.items.is_empty())
            && (self.session.is_none() || !self.has_attachments())
    }

    pub(super) fn sync_location(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            if self.resource.is_none()
                && self.assistant.is_none()
                && !self.busy()
                && let Some(snapshot) = self.node.snapshot.as_ref()
                && self.binding.refresh_project(snapshot)
            {
                self.repository = Default::default();
                cx.emit(Event::LocationChanged);
                cx.notify();
            }
            return;
        };
        self.binding.project = session.project;
        if self.resource.is_some() || self.assistant.is_some() || session.project.is_none() {
            self.binding.worktree = Some(session.worktree);
            if session.project.is_none() {
                self.binding.project_name = tr("sessions_unassigned");
                self.binding.branch = SharedString::default();
                self.repository = Default::default();
            }
            return;
        }
        if self.binding.worktree == Some(session.worktree) {
            return;
        }
        self.binding.worktree = Some(session.worktree);
        self.binding.branch = tr("worktree_unavailable");
        self.repository = Default::default();
        self.references.selected.clear();
        self.refresh_repository(repository::Refresh::Location, cx);
        cx.emit(Event::LocationChanged);
        cx.notify();
    }

    pub(super) fn choose_location(
        &mut self,
        projects: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_move() {
            return;
        }
        if !projects && self.binding.project.is_some() {
            self.invoke_location(Intent::Worktrees, window, cx);
            return;
        }
        let Some(snapshot) = &self.node.snapshot else {
            return;
        };
        let bound = self.location(cx);
        let mut choices = snapshot
            .projects
            .iter()
            .map(|project| (project.name.clone(), Choice::Project(project.clone())))
            .collect::<Vec<_>>();
        if self.can_retarget() {
            choices.insert(
                0,
                (tr("composer_no_project").to_string(), Choice::Unassigned),
            );
        }
        choices.push((tr("project_add").to_string(), Choice::AddProject));
        let list = cx.new(|cx| {
            let mut picker = Picker {
                bound: bound.clone(),
                choices,
                visible: Vec::new(),
                selected: None,
                query: String::new(),
            };
            picker.filter();
            ListState::new(picker, window, cx).searchable(true)
        });
        let content = list.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            crate::command_picker::dialog(
                dialog,
                "composer-location-picker",
                &content,
                tr("location_project"),
                window,
                cx,
            )
        });
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::default()), window, cx)
        });
        window.defer(cx, move |window, cx| {
            list.update(cx, |list, cx| list.focus(window, cx))
        });
    }

    pub(super) fn create_worktree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.repository.is_git() {
            self.invoke_location(Intent::CreateWorktree, window, cx);
        }
    }

    fn select_location(&mut self, choice: Choice, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_move() {
            return;
        }
        match choice {
            Choice::Unassigned => {
                if !self.can_retarget() {
                    return;
                }
                let mut binding = self.binding.clone();
                binding.project = None;
                binding.worktree = None;
                binding.project_name = SharedString::default();
                binding.branch = SharedString::default();
                self.retarget(binding, window, cx);
                cx.emit(Event::LocationChanged);
            }
            Choice::AddProject => cx.emit(Event::AddProject(self.binding.client.target())),
            Choice::Project(project) => {
                if !self.can_retarget() {
                    return;
                }
                let Some((project, tree)) = self
                    .node
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| registered_project(snapshot, project.id))
                    .map(|(project, tree)| (project.clone(), tree.clone()))
                else {
                    return;
                };
                let mut binding = self.binding.clone();
                binding.project = Some(project.id);
                binding.project_name = project.name.into();
                binding.worktree = Some(tree.id);
                binding.branch = String::new().into();
                self.retarget(binding, window, cx);
                cx.emit(Event::LocationChanged);
            }
        }
    }

    fn change_worktree(
        &mut self,
        worktree: WorktreeId,
        fork: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_move() {
            return;
        }
        if let Some(session) = &self.session {
            if !fork && session.worktree == worktree {
                return;
            }
            let command = if fork {
                Command::ForkConversationAt {
                    session: session.id,
                    worktree,
                    expected_revision: session.revision,
                }
            } else {
                Command::MoveConversation {
                    session: session.id,
                    worktree,
                    expected_revision: session.revision,
                }
            };
            self.execute(
                command,
                if fork {
                    actions::Action::ForkAt(worktree)
                } else {
                    actions::Action::Move(worktree)
                },
                window,
                cx,
            );
        } else {
            let mut binding = self.binding.clone();
            binding.worktree = Some(worktree);
            binding.branch = String::new().into();
            self.retarget(binding, window, cx);
            cx.emit(Event::LocationChanged);
        }
    }
}

impl Binding {
    fn location_projects(
        &self,
        snapshot: Option<&sailry_protocol::Snapshot>,
        can_retarget: bool,
    ) -> Vec<serde_json::Value> {
        snapshot
            .filter(|snapshot| can_retarget && snapshot.node == self.client.target())
            .into_iter()
            .flat_map(|snapshot| &snapshot.projects)
            .map(|project| serde_json::json!({"id":project.id,"name":project.name}))
            .collect()
    }

    fn project_choice(
        &self,
        snapshot: Option<&sailry_protocol::Snapshot>,
        project: Option<ProjectId>,
        can_retarget: bool,
    ) -> Result<Choice, String> {
        if !can_retarget {
            return Err("conversation project is fixed".into());
        }
        let Some(project) = project else {
            return Ok(Choice::Unassigned);
        };
        snapshot
            .filter(|snapshot| snapshot.node == self.client.target())
            .and_then(|snapshot| registered_project(snapshot, project))
            .map(|(project, _)| Choice::Project(project.clone()))
            .ok_or_else(|| "project is outside the captured node".into())
    }

    pub(super) fn project_available(&self, snapshot: Option<&sailry_protocol::Snapshot>) -> bool {
        self.project.is_none_or(|project| {
            snapshot.is_some_and(|snapshot| {
                snapshot.node == self.client.target()
                    && snapshot.projects.iter().any(|entry| entry.id == project)
            })
        })
    }

    fn refresh_project(&mut self, snapshot: &sailry_protocol::Snapshot) -> bool {
        if snapshot.node != self.client.target() {
            return false;
        }
        let Some(project) = self.project else {
            return false;
        };
        if let Some(project) = snapshot.projects.iter().find(|entry| entry.id == project) {
            self.project_name = project.name.clone().into();
            return false;
        }
        self.project = None;
        self.worktree = None;
        self.project_name = SharedString::default();
        self.branch = SharedString::default();
        true
    }
}

fn registered_project(
    snapshot: &sailry_protocol::Snapshot,
    project: sailry_protocol::ProjectId,
) -> Option<(&Project, &sailry_protocol::Worktree)> {
    let project = snapshot.projects.iter().find(|entry| entry.id == project)?;
    let tree = snapshot
        .worktrees
        .iter()
        .find(|tree| tree.project == Some(project.id) && tree.main)?;
    Some((project, tree))
}

impl Picker {
    fn filter(&mut self) {
        let query = self.query.to_lowercase();
        self.visible = self
            .choices
            .iter()
            .enumerate()
            .filter(|(_, (label, _))| label.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect();
        self.selected = None;
    }
}
impl ListDelegate for Picker {
    type Item = ListItem;
    fn items_count(&self, _: usize, _: &App) -> usize {
        self.visible.len()
    }
    fn set_selected_index(
        &mut self,
        selected: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = selected;
    }
    fn perform_search(
        &mut self,
        query: &str,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.to_owned();
        self.filter();
        cx.notify();
        Task::ready(())
    }
    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let (label, choice) = self.choices.get(*self.visible.get(index.row)?)?;
        let (id, icon, checked) = match choice {
            Choice::Unassigned => (
                "unassigned".into(),
                IconName::Inbox,
                self.bound.binding.project.is_none(),
            ),
            Choice::Project(project) => (
                format!("project-{}", project.id),
                IconName::Folder,
                self.bound.binding.project == Some(project.id),
            ),
            Choice::AddProject => ("add-project".into(), IconName::Plus, false),
        };
        Some(
            ListItem::new(index.row)
                .text_sm()
                .rounded(cx.theme().radius)
                .on_mouse_enter(cx.listener(move |list, _, window, cx| {
                    if list.selected_index() != Some(index) {
                        list.set_selected_index(Some(index), window, cx);
                        cx.notify();
                    }
                }))
                .h(px(36.))
                .px_2()
                .selected(self.selected == Some(index))
                .debug_selector(move || format!("location-{id}"))
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(Icon::new(icon).small())
                        .child(div().flex_1().min_w_0().truncate().child(label.clone()))
                        .when(checked, |row| {
                            row.child(
                                Icon::new(IconName::Check)
                                    .small()
                                    .text_color(cx.theme().foreground),
                            )
                        }),
                ),
        )
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div().p_3().text_sm().child(tr("git_no_matches"))
    }
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(choice) = self
            .selected
            .and_then(|index| self.visible.get(index.row))
            .and_then(|index| self.choices.get(*index))
            .map(|(_, choice)| choice.clone())
        else {
            return;
        };
        let bound = self.bound.clone();
        window.close_dialog(cx);
        window.defer(cx, move |window, cx| {
            let _ = bound.owner.update(cx, |view, cx| {
                if bound.current(view) {
                    view.select_location(choice, window, cx);
                } else {
                    view.error = Some("worktree_context_changed");
                    cx.notify();
                }
            });
        });
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        window.close_dialog(cx);
    }
}

impl View {
    pub(crate) fn location_state(&self) -> serde_json::Value {
        let main = self
            .node
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .worktrees
                    .iter()
                    .find(|tree| Some(tree.id) == self.binding.worktree)
            })
            .is_none_or(|tree| tree.main);
        serde_json::json!({"surface":"composer","node":crate::live::node_key(self.binding.client.target()),"project":self.binding.project,"worktree":self.binding.worktree,
            "session":self.session.as_ref().map(|session|session.id),"revision":self.session.as_ref().map(|session|session.revision.to_string()),
            "main":main,"project_name":self.binding.project_name.as_ref(),"branch":self.binding.branch.as_ref(),
            "projects":self.binding.location_projects(self.node.snapshot.as_ref(),self.can_retarget()),
            "can_move":self.can_move(),"can_retarget":self.can_retarget(),"git":self.repository.is_git(),"git_status":self.repository.kind(),"connected":self.connected(),
            "base":self.session.is_none().then_some(self.base.as_ref()).flatten()})
    }

    /// Records where a new conversation starts; the worktree is created on send.
    pub(crate) fn select_session_base(
        &mut self,
        base: SessionBase,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.can_move()
            || self.session.is_some()
            || self.binding.project.is_none()
            || self.binding.worktree.is_none()
            || !self.repository.is_git()
        {
            return Err("conversation location is unavailable".into());
        }
        if !base.valid() {
            return Err("invalid session base".into());
        }
        self.base = Some(base);
        cx.notify();
        Ok(())
    }

    pub(crate) fn select_registered_project(
        &mut self,
        project: Option<ProjectId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.can_move() {
            return Err("conversation location is unavailable".into());
        }
        let choice = self.binding.project_choice(
            self.node.snapshot.as_ref(),
            project,
            self.can_retarget(),
        )?;
        self.base = None;
        self.select_location(choice, window, cx);
        Ok(())
    }

    pub(crate) fn add_location_project(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.can_move() {
            return Err("conversation location is unavailable".into());
        }
        self.select_location(Choice::AddProject, window, cx);
        Ok(())
    }

    pub(crate) fn select_registered_location(
        &mut self,
        worktree: Option<WorktreeId>,
        fork: bool,
        confirmed: Option<&sailry_protocol::Worktree>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.can_move() {
            return Err("conversation location is unavailable".into());
        }
        self.base = None;
        if let Some(worktree) = worktree {
            if !confirmed
                .is_some_and(|tree| tree.id == worktree && tree.project == self.binding.project)
                && !self.node.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .worktrees
                        .iter()
                        .any(|tree| tree.id == worktree && tree.project == self.binding.project)
                })
            {
                return Err("worktree is outside the captured project".into());
            }
            if fork && self.session.is_none() {
                return Err("fork requires a conversation".into());
            }
            self.change_worktree(worktree, fork, window, cx);
        } else {
            if !self.can_retarget() {
                return Err("conversation project is fixed".into());
            }
            self.choose_location(true, window, cx);
        }
        Ok(())
    }
}
