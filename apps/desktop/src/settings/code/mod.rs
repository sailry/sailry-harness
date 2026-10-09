mod consent;
mod editor;
mod requests;
#[cfg(test)]
mod tests;
mod view;
mod watch;

use super::{Workspace, providers::Binding};
use gpui_kit::{component::*, *};
use sailry_link::{CancellationToken, Transport};
use sailry_protocol::{NodeId, Request, WorktreeId, code};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
pub(super) struct State {
    panels: BTreeMap<(NodeId, WorktreeId), Entity<Panel>>,
    selected: Option<(NodeId, WorktreeId)>,
}

pub(super) struct Panel {
    binding: Binding,
    transport: Arc<dyn Transport>,
    worktree: WorktreeId,
    active: bool,
    saved: Option<code::Settings>,
    draft: Option<code::Settings>,
    workspace: Option<code::Workspace>,
    installations: Vec<code::Installation>,
    rules: Option<code::Rules>,
    servers: Vec<code::Server>,
    loading: bool,
    pending: bool,
    cancelling: Option<String>,
    request: Option<Request>,
    error: Option<&'static str>,
    stop: Option<CancellationToken>,
    generation: u64,
}

impl Drop for Panel {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
    }
}

impl Panel {
    fn new(
        binding: Binding,
        transport: Arc<dyn Transport>,
        worktree: WorktreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::feedback::observe(window, cx, |panel: &Self, _| {
            panel.error.into_iter().collect()
        });
        Self {
            binding,
            transport,
            worktree,
            active: false,
            saved: None,
            draft: None,
            workspace: None,
            installations: Vec::new(),
            rules: None,
            servers: Vec::new(),
            loading: false,
            pending: false,
            cancelling: None,
            request: None,
            error: None,
            stop: None,
            generation: 0,
        }
    }

    fn dirty(&self) -> bool {
        self.saved != self.draft
    }

    fn locked(&self) -> bool {
        self.pending || self.request.is_some() || self.loading || self.saved.is_none()
    }
}

impl Workspace {
    pub(crate) fn bind_code(
        &mut self,
        transport: Arc<dyn Transport>,
        runtime: Arc<tokio::runtime::Runtime>,
        label: SharedString,
        worktree: Option<WorktreeId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = worktree.map(|worktree| (transport.target(), worktree));
        if self.code.selected == selected {
            return;
        }
        self.clear_code(cx);
        let Some(key) = selected else { return };
        let panel = self
            .code
            .panels
            .entry(key)
            .or_insert_with(|| {
                let binding = Binding {
                    client: Arc::new(sailry_client::Client::new(transport.clone())),
                    runtime,
                    label,
                };
                cx.new(|cx| Panel::new(binding, transport, key.1, window, cx))
            })
            .clone();
        self.code.selected = Some(key);
        panel.update(cx, |panel, cx| {
            panel.active = true;
            panel.load(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn clear_code(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.code.selected.take()
            && let Some(panel) = self.code.panels.get(&key)
        {
            panel.update(cx, |panel, cx| {
                panel.active = false;
                panel.generation += 1;
                if let Some(stop) = panel.stop.take() {
                    stop.cancel();
                }
                cx.notify();
            });
        }
    }
}

impl State {
    pub(super) fn render(&self, _: &mut Window, _: &mut Context<Workspace>) -> AnyElement {
        match self.selected.and_then(|key| self.panels.get(&key)) {
            Some(panel) => panel.clone().into_any_element(),
            None => crate::settings::group::Group::new("code_languages")
                .empty(IconName::Cpu, "code_project_required")
                .into_any_element(),
        }
    }
}
