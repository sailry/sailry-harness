pub(crate) mod appearance;
mod cli;
mod host;
pub(crate) use host::metrics::Metrics;
#[cfg(test)]
pub(crate) use host::metrics::Snapshot as MetricsSnapshot;
mod menus;
mod model;
mod preview_worktrees;
mod project;
mod project_editor;
mod project_removal;
mod repository;
mod terminal;
mod trust;

pub(crate) use menus::{Command, Dispatch, Target};
#[cfg(test)]
pub(crate) use model::runtime::Snapshot as RuntimeSnapshot;
#[cfg(test)]
pub(crate) use model::updates::State as UpdateSnapshot;
pub(crate) use model::{Owner, Repository, State};
pub(crate) use trust::Pending;

use crate::{preview::Page, shell::Shell};
use gpui_kit::*;

impl Shell {
    pub(crate) fn select_project(
        &mut self,
        project: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self.workspace.project_owner(project) else {
            return;
        };
        self.select_owner(owner, window, cx);
        self.navigate(Page::Project, window, cx);
    }

    pub(crate) fn select_owner(
        &mut self,
        owner: Owner,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.contains(owner) {
            return;
        }
        if self.workspace.selected_owner(self.host) != Some(owner) {
            self.park_session_panel(window, cx);
            self.files = crate::resources::PreviewFiles::new(window, cx);
            self.git = crate::resources::PreviewGit::new(window, cx);
            self.close_resource_panel(cx);
        }
        self.workspace.select(owner);
        self.host = owner.host;
    }

    pub(crate) fn select_session(
        &mut self,
        key: (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.workspace.sessions.get(&key) else {
            return;
        };
        let owner = session.owner;
        self.sidebar.recent_preview.retain(|entry| *entry != key);
        self.sidebar.recent_preview.insert(0, key);
        self.sidebar.recent_preview.truncate(100);
        self.park_session_panel(window, cx);
        self.select_owner(owner, window, cx);
        self.session = key.1;
        self.session_scope
            .open(crate::shell::session_scope::Key::Preview(key.0, key.1));
        self.navigate(Page::Conversation, window, cx);
        self.sync_session_panel(window, cx);
    }

    pub(crate) fn create_session(
        &mut self,
        owner: Owner,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.contains(owner) {
            return;
        }
        if !self.workspace.projects[&owner.project].trusted {
            self.project_trust(owner.project, Some(Pending::NewSession(owner)), window, cx);
            return;
        }
        let key = self.workspace.create_session(owner);
        self.sidebar.open_project(owner.project);
        self.select_session(key, window, cx);
    }

    pub(crate) fn select_terminal(
        &mut self,
        key: (usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(terminal) = self.workspace.terminals.get(&key) else {
            return;
        };
        self.select_owner(terminal.owner, window, cx);
        self.workspace.terminal = Some(key);
        self.navigate(Page::Terminal, window, cx);
    }

    pub(crate) fn create_terminal(
        &mut self,
        owner: Owner,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.contains(owner) {
            return;
        }
        let key = self.workspace.create_terminal(owner);
        self.sidebar.open_project(owner.project);
        self.select_terminal(key, window, cx);
    }
}
