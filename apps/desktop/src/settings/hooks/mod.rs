mod requests;
#[cfg(test)]
mod tests;
mod view;

use super::{Workspace, providers::Binding};
use gpui_kit::{
    component::{input::InputState, *},
    *,
};
use sailry_link::Transport;
use sailry_protocol::{
    NodeId, Request, WorktreeId,
    hooks::{Phase, Script, Settings},
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
    enabled: bool,
    fields: Vec<(Phase, Entity<InputState>, Entity<InputState>)>,
    loading: bool,
    pending: bool,
    request: Option<Request>,
    error: Option<&'static str>,
}

impl Panel {
    fn new(
        binding: Binding,
        worktree: WorktreeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::feedback::observe(window, cx, |panel: &Self, _| {
            panel.error.into_iter().collect()
        });
        let fields = [Phase::BeforeTool, Phase::AfterTool, Phase::TurnEnd]
            .into_iter()
            .map(|phase| {
                (
                    phase,
                    cx.new(|cx| InputState::new(window, cx)),
                    cx.new(|cx| InputState::new(window, cx).default_value("60000")),
                )
            })
            .collect();
        Self {
            binding,
            worktree,
            active: false,
            root: None,
            sandboxed: false,
            saved: None,
            enabled: false,
            fields,
            loading: false,
            pending: false,
            request: None,
            error: None,
        }
    }

    fn locked(&self) -> bool {
        self.loading || self.pending || self.request.is_some() || self.saved.is_none()
    }

    fn draft(&self, cx: &App) -> Result<Settings, ()> {
        let mut settings = Settings {
            revision: self.saved.as_ref().map_or(0, |saved| saved.revision),
            enabled: self.enabled,
            ..Default::default()
        };
        for (phase, command, timeout) in &self.fields {
            let command = command.read(cx).value().to_string();
            let script = if command.trim().is_empty() {
                None
            } else {
                let timeout_ms = timeout.read(cx).value().parse::<u64>().map_err(|_| ())?;
                if !(1..=60_000).contains(&timeout_ms) {
                    return Err(());
                }
                Some(Script {
                    command,
                    timeout_ms,
                })
            };
            match phase {
                Phase::BeforeTool => settings.before_tool = script,
                Phase::AfterTool => settings.after_tool = script,
                Phase::TurnEnd => settings.turn_end = script,
            }
        }
        Ok(settings)
    }

    fn apply(&mut self, settings: Settings, window: &mut Window, cx: &mut Context<Self>) {
        self.enabled = settings.enabled;
        for (phase, command, timeout) in &self.fields {
            let script = match phase {
                Phase::BeforeTool => settings.before_tool.as_ref(),
                Phase::AfterTool => settings.after_tool.as_ref(),
                Phase::TurnEnd => settings.turn_end.as_ref(),
            };
            command.update(cx, |input, cx| {
                input.set_value(
                    script
                        .map(|script| script.command.clone())
                        .unwrap_or_default(),
                    window,
                    cx,
                )
            });
            timeout.update(cx, |input, cx| {
                input.set_value(
                    script
                        .map_or(60_000, |script| script.timeout_ms)
                        .to_string(),
                    window,
                    cx,
                )
            });
        }
        self.saved = Some(settings);
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
            if panel.saved.is_none() {
                panel.load(window, cx);
            }
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
