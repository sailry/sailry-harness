mod editor;
mod requests;
#[cfg(test)]
mod tests;
mod view;

use super::{Workspace, providers::Binding};
use gpui_kit::{component::*, *};
use sailry_link::Transport;
use sailry_protocol::{
    NodeId, Request, WorktreeId,
    hooks::{Entry, Phase, Script, Settings},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
pub(super) struct State {
    panels: BTreeMap<(NodeId, WorktreeId), Entity<Panel>>,
    selected: Option<(NodeId, WorktreeId)>,
}

pub(super) struct Panel {
    binding: Binding,
    worktree: WorktreeId,
    active: bool,
    root: Option<String>,
    sandboxed: bool,
    saved: Option<Settings>,
    draft: Option<Settings>,
    conflict: bool,
    loading: bool,
    pending: bool,
    request: Option<Request>,
    error: Option<&'static str>,
    #[cfg(test)]
    editor: Option<WeakEntity<editor::Editor>>,
}

impl Panel {
    fn new(
        binding: Binding,
        worktree: WorktreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |panel: &Self, _| panel.error.into_iter().collect(),
            |_, key, cx| {
                let owner = cx.weak_entity();
                notification::Notification::error(crate::tr(key)).action(move |_, _, cx| {
                    let owner = owner.clone();
                    button::Button::new("hooks-recover")
                        .debug_selector(|| "hooks-recover".into())
                        .label(crate::tr(if key == "hooks_unknown" {
                            "hooks_retry"
                        } else {
                            "hooks_reload"
                        }))
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |panel, cx| {
                                if key == "hooks_unknown" {
                                    panel.retry(window, cx);
                                } else {
                                    panel.conflict = false;
                                    panel.draft = None;
                                    panel.load(window, cx);
                                }
                            });
                        }))
                })
            },
        );
        Self {
            binding,
            worktree,
            active: false,
            root: None,
            sandboxed: false,
            saved: None,
            draft: None,
            conflict: false,
            loading: false,
            pending: false,
            request: None,
            error: None,
            #[cfg(test)]
            editor: None,
        }
    }

    fn locked(&self) -> bool {
        !self.active
            || self.loading
            || self.pending
            || self.request.is_some()
            || self.saved.is_none()
            || self.conflict
    }

    fn current(&self) -> Option<&Settings> {
        self.draft.as_ref().or(self.saved.as_ref())
    }

    fn entry(&self, id: &str) -> Option<(Phase, Script)> {
        self.current()?
            .entries()
            .find(|(key, _, _)| *key == id)
            .map(|(_, phase, script)| (phase, script.clone()))
    }

    fn replace(settings: &mut Settings, id: &str, phase: Phase, script: Option<Script>) {
        if let Some(slot) = Phase::ALL.into_iter().find(|phase| phase.name() == id) {
            match slot {
                Phase::BeforeTool => settings.before_tool = script,
                Phase::AfterTool => settings.after_tool = script,
                Phase::TurnEnd => settings.turn_end = script,
            }
        } else if let Some(index) = settings.additional.iter().position(|entry| entry.id == id) {
            if let Some(script) = script {
                settings.additional[index] = Entry {
                    id: id.into(),
                    phase,
                    script,
                };
            } else {
                settings.additional.remove(index);
            }
        } else if let Some(script) = script {
            settings.additional.push(Entry {
                id: id.into(),
                phase,
                script,
            });
        }
    }

    fn edit(
        &mut self,
        id: &str,
        previous: &Option<(Phase, Script)>,
        phase: Phase,
        script: Script,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.active || self.loading || self.entry(id) != *previous {
            self.error = Some("hooks_target_changed");
            cx.notify();
            return false;
        }
        let Some(mut settings) = self.current().cloned() else {
            return false;
        };
        Self::replace(&mut settings, id, phase, Some(script));
        self.draft = Some(settings);
        self.save(window, cx);
        true
    }

    fn apply(&mut self, settings: Settings) {
        self.saved = Some(settings.clone());
        self.draft = Some(settings);
    }
}

impl Workspace {
    pub(crate) fn bind_hooks(
        &mut self,
        transport: Arc<dyn Transport>,
        runtime: Arc<tokio::runtime::Runtime>,
        label: SharedString,
        worktree: Option<WorktreeId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = worktree.map(|worktree| (transport.target(), worktree));
        if self.hooks.selected == key {
            return;
        }
        self.clear_hooks(cx);
        let Some(key) = key else {
            return;
        };
        let panel = self
            .hooks
            .panels
            .entry(key)
            .or_insert_with(|| {
                let binding = Binding {
                    client: Arc::new(sailry_client::Client::new(transport)),
                    runtime,
                    label,
                };
                cx.new(|cx| Panel::new(binding, key.1, window, cx))
            })
            .clone();
        self.hooks.selected = Some(key);
        panel.update(cx, |panel, cx| {
            panel.active = true;
            panel.load(window, cx);
            cx.notify();
        });
        cx.notify();
    }

    pub(crate) fn clear_hooks(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.hooks.selected.take()
            && let Some(panel) = self.hooks.panels.get(&key)
        {
            panel.update(cx, |panel, cx| {
                panel.active = false;
                cx.notify();
            });
        }
    }
}

impl State {
    pub(super) fn render(&self, _: &mut Window, _: &mut Context<Workspace>) -> AnyElement {
        match self.selected.and_then(|key| self.panels.get(&key)) {
            Some(panel) => panel.clone().into_any_element(),
            None => super::group::Group::new("hooks_settings")
                .empty(IconName::Settings2, "hooks_project_required")
                .into_any_element(),
        }
    }
}
